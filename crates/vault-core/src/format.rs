//! Versioned JSON payload stored inside the age encryption (see
//! `docs/FORMAT.md`).
//!
//! This module is the *only* serialization path for vault content. Encoding
//! writes into a zeroizing, size-bounded buffer; decoding checks size and
//! nesting before parsing, rejects unknown schemas and unknown fields, then
//! validates every identifier, relationship and limit before handing back a
//! [`Vault`].

use std::collections::HashSet;
use std::io;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::ids::{DocId, FolderId, HistoryId, RevisionId, VaultId};
use crate::limits;
use crate::model::{
    validate_name, validate_tag, DocMode, Document, Folder, HistoryEntry, HistoryKind, TrashEntry, Vault,
    VaultPrefs, SCHEMA_VERSION,
};
use crate::secret::SecretText;

/// Magic value of the `format` field.
pub const FORMAT_TAG: &str = "nilavar-vault";

// ---------------------------------------------------------------------------
// Encoding

#[derive(Serialize)]
struct SerVault<'a> {
    format: &'static str,
    schema_version: u32,
    vault_id: String,
    revision_id: String,
    parent_revision_id: Option<String>,
    save_counter: u64,
    created_at: i64,
    saved_at: i64,
    prefs: WirePrefs,
    folders: Vec<SerFolder<'a>>,
    documents: Vec<SerDoc<'a>>,
    trash: Vec<SerTrash<'a>>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct WirePrefs {
    idle_lock_secs: u32,
    clipboard_clear_secs: u32,
    autosave: bool,
    lock_on_sleep: bool,
}

#[derive(Serialize)]
struct SerFolder<'a> {
    id: String,
    name: &'a str,
    parent: Option<String>,
    created_at: i64,
}

#[derive(Serialize)]
struct SerDoc<'a> {
    id: String,
    title: &'a str,
    text: &'a str,
    folder: Option<String>,
    tags: Vec<&'a str>,
    mode: WireMode,
    pinned: bool,
    created_at: i64,
    updated_at: i64,
    history: Vec<SerHistory<'a>>,
}

#[derive(Serialize)]
struct SerHistory<'a> {
    id: String,
    saved_at: i64,
    kind: WireKind,
    title: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct SerTrash<'a> {
    deleted_at: i64,
    document: SerDoc<'a>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum WireMode {
    Markdown,
    Plain,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum WireKind {
    Auto,
    Checkpoint,
    Restore,
}

fn ser_doc(d: &Document) -> SerDoc<'_> {
    SerDoc {
        id: d.id.to_hex(),
        title: d.title.expose(),
        text: d.text.expose(),
        folder: d.folder.map(|f| f.to_hex()),
        tags: d.tags.iter().map(|t| t.expose()).collect(),
        mode: match d.mode {
            DocMode::Markdown => WireMode::Markdown,
            DocMode::Plain => WireMode::Plain,
        },
        pinned: d.pinned,
        created_at: d.created_at,
        updated_at: d.updated_at,
        history: d
            .history
            .iter()
            .map(|h| SerHistory {
                id: h.id.to_hex(),
                saved_at: h.saved_at,
                kind: match h.kind {
                    HistoryKind::Auto => WireKind::Auto,
                    HistoryKind::Checkpoint => WireKind::Checkpoint,
                    HistoryKind::Restore => WireKind::Restore,
                },
                title: h.title.expose(),
                text: h.text.expose(),
            })
            .collect(),
    }
}

/// A `Write` sink that never lets plaintext outlive a reallocation and
/// refuses to grow past a hard limit.
pub(crate) struct ZeroizingSink {
    buf: Zeroizing<Vec<u8>>,
    limit: usize,
    overflowed: bool,
}

impl ZeroizingSink {
    pub(crate) fn new(initial: usize, limit: usize) -> Self {
        Self {
            buf: Zeroizing::new(Vec::with_capacity(initial.min(limit))),
            limit,
            overflowed: false,
        }
    }

    pub(crate) fn into_inner(self) -> Zeroizing<Vec<u8>> {
        self.buf
    }
}

impl io::Write for ZeroizingSink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let needed = self.buf.len().saturating_add(data.len());
        if needed > self.limit {
            self.overflowed = true;
            return Err(io::Error::other("payload limit"));
        }
        if needed > self.buf.capacity() {
            let cap = needed.max(self.buf.capacity().saturating_mul(2)).min(self.limit);
            let mut fresh = Vec::with_capacity(cap);
            fresh.extend_from_slice(&self.buf);
            // Dropping the old Zeroizing buffer wipes it.
            self.buf = Zeroizing::new(fresh);
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Validates and serializes a vault into its JSON payload.
///
/// Fails with [`ErrorCode::SaveWouldExceedLimit`] if the payload would exceed
/// [`limits::MAX_PAYLOAD_BYTES`]; the vault is not modified.
pub fn encode(vault: &Vault) -> VaultResult<Zeroizing<Vec<u8>>> {
    validate(vault)?;
    let ser = SerVault {
        format: FORMAT_TAG,
        schema_version: SCHEMA_VERSION,
        vault_id: vault.vault_id.to_hex(),
        revision_id: vault.revision_id.to_hex(),
        parent_revision_id: vault.parent_revision_id.map(|r| r.to_hex()),
        save_counter: vault.save_counter,
        created_at: vault.created_at,
        saved_at: vault.saved_at,
        prefs: WirePrefs {
            idle_lock_secs: vault.prefs.idle_lock_secs,
            clipboard_clear_secs: vault.prefs.clipboard_clear_secs,
            autosave: vault.prefs.autosave,
            lock_on_sleep: vault.prefs.lock_on_sleep,
        },
        folders: vault
            .folders
            .iter()
            .map(|f| SerFolder {
                id: f.id.to_hex(),
                name: f.name.expose(),
                parent: f.parent.map(|p| p.to_hex()),
                created_at: f.created_at,
            })
            .collect(),
        documents: vault.documents.iter().map(ser_doc).collect(),
        trash: vault
            .trash
            .iter()
            .map(|t| SerTrash {
                deleted_at: t.deleted_at,
                document: ser_doc(&t.document),
            })
            .collect(),
    };
    let estimate = vault.approx_payload_bytes().saturating_mul(9) / 8;
    let mut sink = ZeroizingSink::new(estimate, limits::MAX_PAYLOAD_BYTES);
    match serde_json::to_writer(&mut sink, &ser) {
        Ok(()) => Ok(sink.into_inner()),
        Err(_) if sink.overflowed => Err(VaultError::new(ErrorCode::SaveWouldExceedLimit)),
        Err(_) => Err(VaultError::new(ErrorCode::Internal)),
    }
}

// ---------------------------------------------------------------------------
// Decoding

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    schema_version: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeVault {
    format: String,
    schema_version: u32,
    vault_id: String,
    revision_id: String,
    parent_revision_id: Option<String>,
    save_counter: u64,
    created_at: i64,
    saved_at: i64,
    prefs: WirePrefs,
    folders: Vec<DeFolder>,
    documents: Vec<DeDoc>,
    trash: Vec<DeTrash>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeFolder {
    id: String,
    name: String,
    parent: Option<String>,
    created_at: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeDoc {
    id: String,
    title: String,
    text: String,
    folder: Option<String>,
    tags: Vec<String>,
    mode: WireMode,
    pinned: bool,
    created_at: i64,
    updated_at: i64,
    history: Vec<DeHistory>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeHistory {
    id: String,
    saved_at: i64,
    kind: WireKind,
    title: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeTrash {
    deleted_at: i64,
    document: DeDoc,
}

// Owned plaintext strings in the intermediate structs are wiped when dropped
// without being moved into a SecretText.
impl Drop for DeDoc {
    fn drop(&mut self) {
        self.title.zeroize();
        self.text.zeroize();
        for t in &mut self.tags {
            t.zeroize();
        }
    }
}
impl Drop for DeHistory {
    fn drop(&mut self) {
        self.title.zeroize();
        self.text.zeroize();
    }
}
impl Drop for DeFolder {
    fn drop(&mut self) {
        self.name.zeroize();
    }
}

/// Checks that JSON nesting stays within `max_depth` without parsing.
pub fn check_nesting(bytes: &[u8], max_depth: usize) -> VaultResult<()> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &b in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > max_depth {
                    return Err(VaultError::new(ErrorCode::ExcessiveNesting));
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Parses and validates a decrypted payload.
pub fn decode(bytes: &[u8]) -> VaultResult<Vault> {
    if bytes.len() > limits::MAX_PAYLOAD_BYTES {
        return Err(VaultError::new(ErrorCode::PayloadTooLarge));
    }
    if std::str::from_utf8(bytes).is_err() {
        return Err(VaultError::new(ErrorCode::MalformedPayload));
    }
    check_nesting(bytes, limits::MAX_JSON_DEPTH)?;

    let probe: Probe =
        serde_json::from_slice(bytes).map_err(|_| VaultError::new(ErrorCode::MalformedPayload))?;
    if probe.format.as_deref() != Some(FORMAT_TAG) {
        return Err(VaultError::new(ErrorCode::MalformedPayload));
    }
    if probe.schema_version != Some(u64::from(SCHEMA_VERSION)) {
        return Err(VaultError::new(ErrorCode::UnsupportedSchema));
    }

    let mut de: DeVault =
        serde_json::from_slice(bytes).map_err(|_| VaultError::new(ErrorCode::MalformedPayload))?;
    if de.format != FORMAT_TAG || de.schema_version != SCHEMA_VERSION {
        return Err(VaultError::new(ErrorCode::UnsupportedSchema));
    }

    if de.documents.len() > limits::MAX_DOCUMENTS
        || de.folders.len() > limits::MAX_FOLDERS
        || de.trash.len() > limits::MAX_TRASH
    {
        return Err(VaultError::new(ErrorCode::TooManyItems));
    }

    let folders = de
        .folders
        .iter_mut()
        .map(|f| {
            Ok(Folder {
                id: FolderId::parse(&f.id)?,
                name: SecretText::from_string(std::mem::take(&mut f.name)),
                parent: f.parent.as_deref().map(FolderId::parse).transpose()?,
                created_at: f.created_at,
            })
        })
        .collect::<VaultResult<Vec<_>>>()?;
    let documents = de
        .documents
        .iter_mut()
        .map(convert_doc)
        .collect::<VaultResult<Vec<_>>>()?;
    let trash = de
        .trash
        .iter_mut()
        .map(|t| {
            Ok(TrashEntry {
                deleted_at: t.deleted_at,
                document: convert_doc(&mut t.document)?,
            })
        })
        .collect::<VaultResult<Vec<_>>>()?;

    let vault = Vault {
        vault_id: VaultId::parse(&de.vault_id)?,
        revision_id: RevisionId::parse(&de.revision_id)?,
        parent_revision_id: de.parent_revision_id.as_deref().map(RevisionId::parse).transpose()?,
        save_counter: de.save_counter,
        created_at: de.created_at,
        saved_at: de.saved_at,
        folders,
        documents,
        trash,
        prefs: VaultPrefs {
            idle_lock_secs: de.prefs.idle_lock_secs,
            clipboard_clear_secs: de.prefs.clipboard_clear_secs,
            autosave: de.prefs.autosave,
            lock_on_sleep: de.prefs.lock_on_sleep,
        }
        .clamped(),
    };
    validate(&vault)?;
    Ok(vault)
}

fn convert_doc(d: &mut DeDoc) -> VaultResult<Document> {
    if d.tags.len() > limits::MAX_TAGS_PER_DOCUMENT || d.history.len() > limits::MAX_REVISIONS_PER_DOCUMENT {
        return Err(VaultError::new(ErrorCode::TooManyItems));
    }
    let history = d
        .history
        .iter_mut()
        .map(|h| {
            Ok(HistoryEntry {
                id: HistoryId::parse(&h.id)?,
                saved_at: h.saved_at,
                kind: match h.kind {
                    WireKind::Auto => HistoryKind::Auto,
                    WireKind::Checkpoint => HistoryKind::Checkpoint,
                    WireKind::Restore => HistoryKind::Restore,
                },
                title: SecretText::from_string(std::mem::take(&mut h.title)),
                text: SecretText::from_string(std::mem::take(&mut h.text)),
            })
        })
        .collect::<VaultResult<Vec<_>>>()?;
    let mut doc = Document::new("", None);
    doc.id = DocId::parse(&d.id)?;
    doc.title = SecretText::from_string(std::mem::take(&mut d.title));
    doc.text = SecretText::from_string(std::mem::take(&mut d.text));
    doc.folder = d.folder.as_deref().map(FolderId::parse).transpose()?;
    doc.tags = d
        .tags
        .iter_mut()
        .map(|t| SecretText::from_string(std::mem::take(t)))
        .collect();
    doc.mode = match d.mode {
        WireMode::Markdown => DocMode::Markdown,
        WireMode::Plain => DocMode::Plain,
    };
    doc.pinned = d.pinned;
    doc.created_at = d.created_at;
    doc.updated_at = d.updated_at;
    doc.history = history;
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Validation

/// Validates every structural invariant of a vault.
pub fn validate(vault: &Vault) -> VaultResult<()> {
    if vault.documents.len() > limits::MAX_DOCUMENTS
        || vault.folders.len() > limits::MAX_FOLDERS
        || vault.trash.len() > limits::MAX_TRASH
        || vault.total_history_entries() > limits::MAX_TOTAL_REVISIONS
    {
        return Err(VaultError::new(ErrorCode::TooManyItems));
    }
    if vault.prefs != vault.prefs.clamped() {
        return Err(VaultError::new(ErrorCode::MalformedPayload));
    }

    // Folders: unique ids, names valid, parents exist, no cycles, depth bounded.
    let mut folder_ids = HashSet::with_capacity(vault.folders.len());
    for f in &vault.folders {
        if !folder_ids.insert(f.id) {
            return Err(VaultError::new(ErrorCode::DuplicateId));
        }
        validate_name(f.name.expose())?;
        if f.name.expose().trim().is_empty() {
            return Err(VaultError::new(ErrorCode::InvalidName));
        }
    }
    for f in &vault.folders {
        if let Some(p) = f.parent {
            if !folder_ids.contains(&p) {
                return Err(VaultError::new(ErrorCode::DanglingReference));
            }
            if p == f.id {
                return Err(VaultError::new(ErrorCode::FolderCycle));
            }
        }
    }
    for f in &vault.folders {
        let mut depth = 0usize;
        let mut cur = Some(f.id);
        while let Some(c) = cur {
            depth += 1;
            if depth > vault.folders.len() {
                return Err(VaultError::new(ErrorCode::FolderCycle));
            }
            cur = vault.folder(c).and_then(|x| x.parent);
        }
        if depth > limits::MAX_FOLDER_DEPTH {
            return Err(VaultError::new(ErrorCode::FolderTooDeep));
        }
    }

    // Documents (live and trashed): unique ids, folder refs, sizes, tags, history.
    let mut doc_ids = HashSet::with_capacity(vault.documents.len() + vault.trash.len());
    let mut history_ids = HashSet::new();
    let all_docs = vault
        .documents
        .iter()
        .chain(vault.trash.iter().map(|t| &t.document));
    for d in all_docs {
        if !doc_ids.insert(d.id) {
            return Err(VaultError::new(ErrorCode::DuplicateId));
        }
        if let Some(f) = d.folder
            && !folder_ids.contains(&f)
        {
            return Err(VaultError::new(ErrorCode::DanglingReference));
        }
        validate_name(d.title.expose())?;
        if d.text.len() > limits::MAX_DOCUMENT_BYTES {
            return Err(VaultError::new(ErrorCode::DocumentTooLarge));
        }
        if d.tags.len() > limits::MAX_TAGS_PER_DOCUMENT {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        let mut seen_tags = HashSet::new();
        for t in &d.tags {
            validate_tag(t.expose())?;
            if !seen_tags.insert(t.expose()) {
                return Err(VaultError::new(ErrorCode::DuplicateId));
            }
        }
        if d.history.len() > limits::MAX_REVISIONS_PER_DOCUMENT {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        for h in &d.history {
            if !history_ids.insert(h.id) {
                return Err(VaultError::new(ErrorCode::DuplicateId));
            }
            validate_name(h.title.expose())?;
            if h.text.len() > limits::MAX_DOCUMENT_BYTES {
                return Err(VaultError::new(ErrorCode::DocumentTooLarge));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vault {
        let mut v = Vault::new();
        let f = v.create_folder("Földer ✓", None).unwrap();
        let d = v.create_document("Tïtle 🔐", Some(f)).unwrap();
        v.document_mut(d).unwrap().text.set("línea 1\n\"quoted\" \\ back\n\u{0}nul\n");
        v.set_tags(d, &["ключ", "api"]).unwrap();
        crate::history::record_save(&mut v, 1, crate::history::SaveTrigger::Explicit).unwrap();
        let e = v.create_document("", None).unwrap();
        v.trash_document(e).unwrap();
        v
    }

    #[test]
    fn roundtrip_preserves_everything() {
        let v = sample();
        let bytes = encode(&v).unwrap();
        let back = decode(&bytes).unwrap();
        assert_eq!(back.vault_id, v.vault_id);
        assert_eq!(back.documents.len(), 1);
        assert_eq!(back.trash.len(), 1);
        let (a, b) = (&v.documents[0], &back.documents[0]);
        assert_eq!(a.text, b.text);
        assert_eq!(a.title, b.title);
        assert_eq!(a.tags, b.tags);
        assert_eq!(a.history.len(), b.history.len());
        assert_eq!(back.folders[0].name.expose(), "Földer ✓");
        // Encoding is deterministic for the same state.
        assert_eq!(&*encode(&back).unwrap(), &*bytes);
    }

    #[test]
    fn rejects_unknown_schema_and_fields() {
        let v = sample();
        let bytes = encode(&v).unwrap();
        let s = std::str::from_utf8(&bytes).unwrap();
        let newer = s.replace("\"schema_version\":1", "\"schema_version\":2");
        assert_eq!(decode(newer.as_bytes()).unwrap_err().code(), ErrorCode::UnsupportedSchema);
        let extra = s.replacen('{', "{\"surprise\":true,", 1);
        assert_eq!(decode(extra.as_bytes()).unwrap_err().code(), ErrorCode::MalformedPayload);
        assert_eq!(decode(b"{}").unwrap_err().code(), ErrorCode::MalformedPayload);
        assert_eq!(decode(b"\xff\xfe").unwrap_err().code(), ErrorCode::MalformedPayload);
    }

    #[test]
    fn rejects_duplicate_and_dangling() {
        let mut v = sample();
        let dup = v.documents[0].id;
        v.trash[0].document.id = dup;
        assert_eq!(validate(&v).unwrap_err().code(), ErrorCode::DuplicateId);

        let mut v = sample();
        v.documents[0].folder = Some(FolderId::random());
        assert_eq!(validate(&v).unwrap_err().code(), ErrorCode::DanglingReference);

        let mut v = sample();
        let a = v.folders[0].id;
        let b = v.create_folder("child", Some(a)).unwrap();
        v.folders[0].parent = Some(b);
        assert_eq!(validate(&v).unwrap_err().code(), ErrorCode::FolderCycle);
    }

    #[test]
    fn rejects_deep_nesting_before_parsing() {
        let deep = "[".repeat(limits::MAX_JSON_DEPTH + 1);
        assert_eq!(decode(deep.as_bytes()).unwrap_err().code(), ErrorCode::ExcessiveNesting);
        // Brackets inside strings don't count.
        assert!(check_nesting(b"{\"a\":\"[[[[[[[[[[[[[[[[[[[[[[[[\"}", 2).is_ok());
    }

    #[test]
    fn oversized_document_refused_on_encode() {
        let mut v = Vault::new();
        let d = v.create_document("big", None).unwrap();
        v.document_mut(d)
            .unwrap()
            .text
            .set(&"x".repeat(limits::MAX_DOCUMENT_BYTES + 1));
        assert_eq!(encode(&v).unwrap_err().code(), ErrorCode::DocumentTooLarge);
    }

    #[test]
    fn payload_limit_refuses_without_mutation() {
        let mut v = Vault::new();
        // 17 docs x 2 MiB > 32 MiB.
        let big = "y".repeat(limits::MAX_DOCUMENT_BYTES);
        for i in 0..17 {
            let d = v.create_document(&format!("d{i}"), None).unwrap();
            v.document_mut(d).unwrap().text.set(&big);
        }
        assert_eq!(encode(&v).unwrap_err().code(), ErrorCode::SaveWouldExceedLimit);
        assert_eq!(v.documents.len(), 17);
    }
}
