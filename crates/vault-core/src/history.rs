//! Encrypted per-document history.
//!
//! History is recorded at save time (never per keystroke). The newest entry
//! of each document mirrors its last saved state:
//!
//! * a document whose title/text differ from its newest entry gets a new
//!   entry;
//! * consecutive *autosave* entries within [`COALESCE_WINDOW_SECS`] are
//!   coalesced into one, so typing with autosave on does not churn history;
//! * explicit saves record a [`HistoryKind::Checkpoint`] that is never
//!   coalesced, and restores record a [`HistoryKind::Restore`];
//! * each document keeps at most [`limits::MAX_REVISIONS_PER_DOCUMENT`]
//!   entries (oldest dropped first — a documented retention policy).

use std::time::Duration;

use similar::{ChangeTag, TextDiff};
use zeroize::Zeroizing;

use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::ids::HistoryId;
use crate::limits;
use crate::model::{Document, HistoryEntry, HistoryKind, Vault};
use crate::secret::SecretText;

/// Autosave entries closer together than this are merged.
pub const COALESCE_WINDOW_SECS: i64 = 10 * 60;

/// How a save was triggered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveTrigger {
    /// Debounced autosave.
    Auto,
    /// Explicit user save (Cmd/Ctrl+S, lock with save, etc.).
    Explicit,
}

/// Records history for every changed live document. Returns the number of
/// documents whose history changed.
pub fn record_save(vault: &mut Vault, now: i64, trigger: SaveTrigger) -> VaultResult<usize> {
    let mut changed = 0;
    for doc in &mut vault.documents {
        if record_document(doc, now, trigger) {
            changed += 1;
        }
    }
    if vault.total_history_entries() > limits::MAX_TOTAL_REVISIONS {
        return Err(VaultError::new(ErrorCode::SaveWouldExceedLimit));
    }
    Ok(changed)
}

/// Records history for one document; returns whether anything changed.
pub fn record_document(doc: &mut Document, now: i64, trigger: SaveTrigger) -> bool {
    let kind = if doc.restore_pending {
        HistoryKind::Restore
    } else {
        match trigger {
            SaveTrigger::Auto => HistoryKind::Auto,
            SaveTrigger::Explicit => HistoryKind::Checkpoint,
        }
    };

    let history_len = doc.history.len();
    if let Some(last) = doc.history.last_mut() {
        let same = last.title == doc.title && last.text == doc.text;
        if same {
            // Promote an autosave entry to a checkpoint on explicit save so
            // it will not be coalesced away later.
            if kind == HistoryKind::Checkpoint && last.kind == HistoryKind::Auto {
                last.kind = HistoryKind::Checkpoint;
                return true;
            }
            doc.restore_pending = false;
            return false;
        }
        let coalesce = kind == HistoryKind::Auto
            && last.kind == HistoryKind::Auto
            && now.saturating_sub(last.saved_at) < COALESCE_WINDOW_SECS
            && history_len > 1;
        if coalesce {
            last.title.set(doc.title.expose());
            last.text.set(doc.text.expose());
            last.saved_at = now;
            return true;
        }
    }

    doc.history.push(HistoryEntry {
        id: HistoryId::random(),
        saved_at: now,
        kind,
        title: doc.title.duplicate(),
        text: doc.text.duplicate(),
    });
    doc.restore_pending = false;
    let excess = doc.history.len().saturating_sub(limits::MAX_REVISIONS_PER_DOCUMENT);
    if excess > 0 {
        doc.history.drain(..excess);
    }
    true
}

/// Marks that the next recorded entry is a restore.
pub(crate) fn mark_restore(doc: &mut Document) {
    doc.restore_pending = true;
}

/// Kind of a diff line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Present in both.
    Same,
    /// Only in the older text.
    Removed,
    /// Only in the newer text.
    Added,
}

/// One line of a line-based diff. Holds plaintext; drop it when done.
pub struct DiffLine {
    /// Kind of change.
    pub kind: DiffKind,
    /// Line content without the trailing newline.
    pub text: Zeroizing<String>,
}

impl std::fmt::Debug for DiffLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DiffLine({:?}, <redacted>)", self.kind)
    }
}

/// Computes a line diff between two texts with a bounded time budget.
pub fn diff_lines(old: &SecretText, new: &SecretText) -> Vec<DiffLine> {
    let diff = TextDiff::configure()
        .timeout(Duration::from_millis(400))
        .diff_lines(old.expose(), new.expose());
    diff.iter_all_changes()
        .map(|c| {
            let kind = match c.tag() {
                ChangeTag::Equal => DiffKind::Same,
                ChangeTag::Delete => DiffKind::Removed,
                ChangeTag::Insert => DiffKind::Added,
            };
            let v: &str = c.value();
            let trimmed = v.strip_suffix('\n').unwrap_or(v);
            let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
            DiffLine {
                kind,
                text: Zeroizing::new(trimmed.to_owned()),
            }
        })
        .collect()
}

/// Summary counts for a diff.
pub fn diff_stats(lines: &[DiffLine]) -> (usize, usize) {
    lines.iter().fold((0, 0), |(a, r), l| match l.kind {
        DiffKind::Added => (a + 1, r),
        DiffKind::Removed => (a, r + 1),
        DiffKind::Same => (a, r),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(text: &str) -> Document {
        let mut d = Document::new("T", None);
        d.text.set(text);
        d
    }

    #[test]
    fn unchanged_document_records_once() {
        let mut d = doc_with("a");
        assert!(record_document(&mut d, 100, SaveTrigger::Auto));
        assert!(!record_document(&mut d, 101, SaveTrigger::Auto));
        assert_eq!(d.history.len(), 1);
    }

    #[test]
    fn autosaves_coalesce_but_checkpoints_do_not() {
        let mut d = doc_with("v1");
        record_document(&mut d, 0, SaveTrigger::Explicit);
        d.text.set("v2");
        record_document(&mut d, 10, SaveTrigger::Auto);
        d.text.set("v3");
        record_document(&mut d, 20, SaveTrigger::Auto);
        d.text.set("v4");
        record_document(&mut d, 30, SaveTrigger::Auto);
        // v1 checkpoint + one coalesced autosave entry (now v4).
        assert_eq!(d.history.len(), 2);
        assert_eq!(d.history[1].text.expose(), "v4");
        // An explicit save after autosave promotes when unchanged…
        record_document(&mut d, 40, SaveTrigger::Explicit);
        assert_eq!(d.history[1].kind, HistoryKind::Checkpoint);
        // …so the next autosave starts a new entry.
        d.text.set("v5");
        record_document(&mut d, 50, SaveTrigger::Auto);
        assert_eq!(d.history.len(), 3);
        // Outside the window, autosaves do not coalesce.
        d.text.set("v6");
        record_document(&mut d, 50 + COALESCE_WINDOW_SECS + 1, SaveTrigger::Auto);
        assert_eq!(d.history.len(), 4);
    }

    #[test]
    fn retention_bound_drops_oldest() {
        let mut d = doc_with("0");
        for i in 0..(limits::MAX_REVISIONS_PER_DOCUMENT + 5) {
            d.text.set(&format!("rev {i}"));
            record_document(&mut d, i as i64, SaveTrigger::Explicit);
        }
        assert_eq!(d.history.len(), limits::MAX_REVISIONS_PER_DOCUMENT);
        assert_eq!(
            d.history.last().unwrap().text.expose(),
            format!("rev {}", limits::MAX_REVISIONS_PER_DOCUMENT + 4)
        );
    }

    #[test]
    fn restore_creates_restore_entry() {
        let mut v = Vault::new();
        let id = v.create_document("T", None).unwrap();
        v.document_mut(id).unwrap().text.set("first");
        record_save(&mut v, 0, SaveTrigger::Explicit).unwrap();
        v.document_mut(id).unwrap().text.set("second");
        record_save(&mut v, 1, SaveTrigger::Explicit).unwrap();
        let first = v.document(id).unwrap().history[0].id;
        v.restore_history_entry(id, first).unwrap();
        assert_eq!(v.document(id).unwrap().text.expose(), "first");
        record_save(&mut v, 2, SaveTrigger::Auto).unwrap();
        let d = v.document(id).unwrap();
        assert_eq!(d.history.len(), 3);
        assert_eq!(d.history[2].kind, HistoryKind::Restore);
        assert_eq!(d.history[1].text.expose(), "second");
    }

    #[test]
    fn diff_reports_changes() {
        let a = SecretText::from("one\ntwo\nthree\n");
        let b = SecretText::from("one\n2\nthree\nfour\n");
        let lines = diff_lines(&a, &b);
        assert_eq!(diff_stats(&lines), (2, 1));
        assert!(lines.iter().any(|l| l.kind == DiffKind::Removed && l.text.as_str() == "two"));
    }
}
