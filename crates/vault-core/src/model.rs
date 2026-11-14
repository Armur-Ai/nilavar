//! In-memory vault model.
//!
//! Model types deliberately implement neither `serde` traits nor `Display`.
//! The only way to turn a vault into bytes is [`crate::format::encode`], which
//! is only called from the encryption path.

use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::ids::{DocId, FolderId, HistoryId, RevisionId, VaultId};
use crate::limits;
use crate::secret::SecretText;
use crate::time::now_unix;

/// Current payload schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// Editing/rendering mode of a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DocMode {
    /// Markdown with highlighting and preview.
    #[default]
    Markdown,
    /// Plain text, no highlighting or preview.
    Plain,
}

/// Why a history entry was recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKind {
    /// Recorded by autosave; may be coalesced with the next autosave.
    Auto,
    /// Recorded by an explicit save; never coalesced.
    Checkpoint,
    /// Recorded when a previous revision was restored.
    Restore,
}

/// A folder. Folder names are encrypted like everything else in the payload.
#[derive(Debug)]
pub struct Folder {
    /// Identifier.
    pub id: FolderId,
    /// Display name.
    pub name: SecretText,
    /// Parent folder, or `None` for the vault root.
    pub parent: Option<FolderId>,
    /// Creation time (Unix seconds).
    pub created_at: i64,
}

/// One retained historical state of a document.
#[derive(Debug)]
pub struct HistoryEntry {
    /// Identifier.
    pub id: HistoryId,
    /// When this state was saved.
    pub saved_at: i64,
    /// What recorded it.
    pub kind: HistoryKind,
    /// Title at that time.
    pub title: SecretText,
    /// Text at that time.
    pub text: SecretText,
}

/// A document.
#[derive(Debug)]
pub struct Document {
    /// Identifier.
    pub id: DocId,
    /// Title.
    pub title: SecretText,
    /// Body text (UTF-8).
    pub text: SecretText,
    /// Containing folder, or `None` for the root.
    pub folder: Option<FolderId>,
    /// Tags.
    pub tags: Vec<SecretText>,
    /// Editing mode.
    pub mode: DocMode,
    /// Pinned to the top of the document list.
    pub pinned: bool,
    /// Creation time.
    pub created_at: i64,
    /// Last content modification time.
    pub updated_at: i64,
    /// Retained history, oldest first. The newest entry mirrors the most
    /// recently saved state of the document.
    pub history: Vec<HistoryEntry>,
    /// Transient: the next history entry records a restore. Not persisted.
    pub(crate) restore_pending: bool,
}

impl Document {
    /// Creates an empty document.
    pub fn new(title: &str, folder: Option<FolderId>) -> Self {
        let now = now_unix();
        Self {
            id: DocId::random(),
            title: SecretText::from(title),
            text: SecretText::new(),
            folder,
            tags: Vec::new(),
            mode: DocMode::Markdown,
            pinned: false,
            created_at: now,
            updated_at: now,
            history: Vec::new(),
            restore_pending: false,
        }
    }

    /// Whether `tag` (exact match) is present.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t.expose() == tag)
    }

    /// Finds a history entry.
    pub fn history_entry(&self, id: HistoryId) -> Option<&HistoryEntry> {
        self.history.iter().find(|h| h.id == id)
    }

    /// Approximate payload bytes this document contributes.
    pub fn approx_bytes(&self) -> usize {
        let mut n = self.title.len() + self.text.len() + 256;
        n += self.tags.iter().map(|t| t.len() + 8).sum::<usize>();
        n += self
            .history
            .iter()
            .map(|h| h.title.len() + h.text.len() + 128)
            .sum::<usize>();
        n
    }
}

/// A deleted document awaiting restore or purge.
#[derive(Debug)]
pub struct TrashEntry {
    /// The document, including its history.
    pub document: Document,
    /// When it was deleted.
    pub deleted_at: i64,
}

/// Vault-specific preferences (stored encrypted inside the payload).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultPrefs {
    /// Idle time before automatic lock, in seconds (60..=86400).
    pub idle_lock_secs: u32,
    /// Delay before the app clears clipboard text it placed, in seconds (5..=600).
    pub clipboard_clear_secs: u32,
    /// Encrypted autosave enabled.
    pub autosave: bool,
    /// Lock when the system sleeps or the screen locks (where supported).
    pub lock_on_sleep: bool,
}

impl Default for VaultPrefs {
    fn default() -> Self {
        Self {
            idle_lock_secs: 300,
            clipboard_clear_secs: 30,
            autosave: true,
            lock_on_sleep: true,
        }
    }
}

impl VaultPrefs {
    /// Inclusive bounds for `idle_lock_secs`.
    pub const IDLE_RANGE: (u32, u32) = (60, 86_400);
    /// Inclusive bounds for `clipboard_clear_secs`.
    pub const CLIPBOARD_RANGE: (u32, u32) = (5, 600);

    /// Clamps values into their documented ranges.
    pub fn clamped(mut self) -> Self {
        self.idle_lock_secs = self
            .idle_lock_secs
            .clamp(Self::IDLE_RANGE.0, Self::IDLE_RANGE.1);
        self.clipboard_clear_secs = self
            .clipboard_clear_secs
            .clamp(Self::CLIPBOARD_RANGE.0, Self::CLIPBOARD_RANGE.1);
        self
    }
}

/// The complete decrypted vault.
#[derive(Debug)]
pub struct Vault {
    /// Stable vault identifier.
    pub vault_id: VaultId,
    /// Identifier of this saved state.
    pub revision_id: RevisionId,
    /// Identifier of the state this one was derived from.
    pub parent_revision_id: Option<RevisionId>,
    /// Number of successful saves (monotonic within one vault lineage).
    pub save_counter: u64,
    /// Creation time.
    pub created_at: i64,
    /// Time of the save that produced this state.
    pub saved_at: i64,
    /// Folders.
    pub folders: Vec<Folder>,
    /// Live documents.
    pub documents: Vec<Document>,
    /// Deleted documents.
    pub trash: Vec<TrashEntry>,
    /// Vault preferences.
    pub prefs: VaultPrefs,
}

impl Vault {
    /// A fresh, empty vault.
    pub fn new() -> Self {
        let now = now_unix();
        Self {
            vault_id: VaultId::random(),
            revision_id: RevisionId::random(),
            parent_revision_id: None,
            save_counter: 0,
            created_at: now,
            saved_at: now,
            folders: Vec::new(),
            documents: Vec::new(),
            trash: Vec::new(),
            prefs: VaultPrefs::default(),
        }
    }

    /// A fresh vault containing a short synthetic welcome note.
    pub fn new_with_welcome() -> Self {
        let mut v = Self::new();
        let mut d = Document::new("Welcome", None);
        d.text.set(WELCOME_TEXT);
        v.documents.push(d);
        v
    }

    // ----- lookup -------------------------------------------------------

    /// Finds a live document.
    pub fn document(&self, id: DocId) -> Option<&Document> {
        self.documents.iter().find(|d| d.id == id)
    }

    /// Finds a live document mutably.
    pub fn document_mut(&mut self, id: DocId) -> Option<&mut Document> {
        self.documents.iter_mut().find(|d| d.id == id)
    }

    /// Finds a folder.
    pub fn folder(&self, id: FolderId) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// Depth of a folder (root-level folders have depth 1). Returns `None`
    /// on a missing folder or cycle.
    pub fn folder_depth(&self, id: FolderId) -> Option<usize> {
        let mut depth = 0;
        let mut cur = Some(id);
        while let Some(c) = cur {
            depth += 1;
            if depth > self.folders.len() + 1 {
                return None;
            }
            cur = self.folder(c)?.parent;
        }
        Some(depth)
    }

    /// Path components (names) from the root to `id`, inclusive.
    pub fn folder_path(&self, id: FolderId) -> Vec<&SecretText> {
        let mut out = Vec::new();
        let mut cur = Some(id);
        let mut guard = 0;
        while let Some(c) = cur {
            guard += 1;
            if guard > limits::MAX_FOLDER_DEPTH + 1 {
                break;
            }
            match self.folder(c) {
                Some(f) => {
                    out.push(&f.name);
                    cur = f.parent;
                }
                None => break,
            }
        }
        out.reverse();
        out
    }

    /// Children folders of `parent`, sorted by name.
    pub fn child_folders(&self, parent: Option<FolderId>) -> Vec<&Folder> {
        let mut v: Vec<&Folder> = self.folders.iter().filter(|f| f.parent == parent).collect();
        v.sort_by(|a, b| {
            a.name
                .expose()
                .to_lowercase()
                .cmp(&b.name.expose().to_lowercase())
        });
        v
    }

    /// Whether `ancestor` is `id` or one of its ancestors.
    pub fn folder_is_within(&self, id: FolderId, ancestor: FolderId) -> bool {
        let mut cur = Some(id);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            guard += 1;
            if guard > limits::MAX_FOLDER_DEPTH + 1 {
                return false;
            }
            cur = self.folder(c).and_then(|f| f.parent);
        }
        false
    }

    /// Total history entries across live documents and trash.
    pub fn total_history_entries(&self) -> usize {
        self.documents
            .iter()
            .map(|d| d.history.len())
            .sum::<usize>()
            + self
                .trash
                .iter()
                .map(|t| t.document.history.len())
                .sum::<usize>()
    }

    /// Approximate decrypted payload size (for warnings before saving).
    pub fn approx_payload_bytes(&self) -> usize {
        let docs: usize = self.documents.iter().map(Document::approx_bytes).sum();
        let trash: usize = self.trash.iter().map(|t| t.document.approx_bytes()).sum();
        let folders: usize = self.folders.iter().map(|f| f.name.len() + 128).sum();
        docs + trash + folders + 1024
    }

    // ----- documents ----------------------------------------------------

    /// Creates a document and returns its id.
    pub fn create_document(&mut self, title: &str, folder: Option<FolderId>) -> VaultResult<DocId> {
        if self.documents.len() >= limits::MAX_DOCUMENTS {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        validate_name(title)?;
        if let Some(f) = folder {
            self.folder(f)
                .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        }
        let doc = Document::new(title, folder);
        let id = doc.id;
        self.documents.push(doc);
        Ok(id)
    }

    /// Adds a fully-formed document (used by import).
    pub fn add_document(&mut self, doc: Document) -> VaultResult<DocId> {
        if self.documents.len() >= limits::MAX_DOCUMENTS {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        if doc.text.len() > limits::MAX_DOCUMENT_BYTES {
            return Err(VaultError::new(ErrorCode::DocumentTooLarge));
        }
        validate_name(doc.title.expose())?;
        let id = doc.id;
        self.documents.push(doc);
        Ok(id)
    }

    /// Renames a document.
    pub fn rename_document(&mut self, id: DocId, title: &str) -> VaultResult<()> {
        validate_name(title)?;
        let d = self
            .document_mut(id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        d.title.set(title);
        d.updated_at = now_unix();
        Ok(())
    }

    /// Moves a document to a folder (or the root).
    pub fn move_document(&mut self, id: DocId, folder: Option<FolderId>) -> VaultResult<()> {
        if let Some(f) = folder {
            self.folder(f)
                .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        }
        let d = self
            .document_mut(id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        d.folder = folder;
        Ok(())
    }

    /// Replaces a document's tags (deduplicated, empty tags dropped).
    pub fn set_tags(&mut self, id: DocId, tags: &[&str]) -> VaultResult<()> {
        let mut clean: Vec<SecretText> = Vec::new();
        for t in tags {
            if t.is_empty() {
                continue;
            }
            validate_tag(t)?;
            if !clean.iter().any(|c| c.expose() == *t) {
                clean.push(SecretText::from(*t));
            }
        }
        if clean.len() > limits::MAX_TAGS_PER_DOCUMENT {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        let d = self
            .document_mut(id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        d.tags = clean;
        Ok(())
    }

    /// All distinct tags across live documents, sorted.
    pub fn all_tags(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .documents
            .iter()
            .flat_map(|d| d.tags.iter().map(|t| t.expose()))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Moves a document to the trash, keeping its history.
    pub fn trash_document(&mut self, id: DocId) -> VaultResult<()> {
        if self.trash.len() >= limits::MAX_TRASH {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        let idx = self
            .documents
            .iter()
            .position(|d| d.id == id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        let document = self.documents.remove(idx);
        self.trash.push(TrashEntry {
            document,
            deleted_at: now_unix(),
        });
        Ok(())
    }

    /// Restores a trashed document. If its folder no longer exists it is
    /// restored to the root.
    pub fn restore_from_trash(&mut self, id: DocId) -> VaultResult<()> {
        if self.documents.len() >= limits::MAX_DOCUMENTS {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        let idx = self
            .trash
            .iter()
            .position(|t| t.document.id == id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        let mut doc = self.trash.remove(idx).document;
        if let Some(f) = doc.folder
            && self.folder(f).is_none()
        {
            doc.folder = None;
        }
        self.documents.push(doc);
        Ok(())
    }

    /// Permanently removes one trashed document (and its history) from the
    /// vault. Old vault files, recovery copies and backups may still hold it.
    pub fn purge_from_trash(&mut self, id: DocId) -> VaultResult<()> {
        let idx = self
            .trash
            .iter()
            .position(|t| t.document.id == id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        self.trash.remove(idx);
        Ok(())
    }

    /// Permanently removes every trashed document.
    pub fn empty_trash(&mut self) -> usize {
        let n = self.trash.len();
        self.trash.clear();
        n
    }

    // ----- folders ------------------------------------------------------

    /// Creates a folder.
    pub fn create_folder(&mut self, name: &str, parent: Option<FolderId>) -> VaultResult<FolderId> {
        if self.folders.len() >= limits::MAX_FOLDERS {
            return Err(VaultError::new(ErrorCode::TooManyItems));
        }
        validate_name(name)?;
        if name.trim().is_empty() {
            return Err(VaultError::new(ErrorCode::InvalidName));
        }
        let depth = match parent {
            Some(p) => {
                self.folder_depth(p)
                    .ok_or(VaultError::new(ErrorCode::NoSuchItem))?
                    + 1
            }
            None => 1,
        };
        if depth > limits::MAX_FOLDER_DEPTH {
            return Err(VaultError::new(ErrorCode::FolderTooDeep));
        }
        let id = FolderId::random();
        self.folders.push(Folder {
            id,
            name: SecretText::from(name),
            parent,
            created_at: now_unix(),
        });
        Ok(id)
    }

    /// Renames a folder.
    pub fn rename_folder(&mut self, id: FolderId, name: &str) -> VaultResult<()> {
        validate_name(name)?;
        if name.trim().is_empty() {
            return Err(VaultError::new(ErrorCode::InvalidName));
        }
        let f = self
            .folders
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        f.name.set(name);
        Ok(())
    }

    /// Deletes an empty folder (no documents and no subfolders). Trashed
    /// documents that lived in it will be restored to the root.
    pub fn delete_folder(&mut self, id: FolderId) -> VaultResult<()> {
        self.folder(id)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        let has_docs = self.documents.iter().any(|d| d.folder == Some(id));
        let has_children = self.folders.iter().any(|f| f.parent == Some(id));
        if has_docs || has_children {
            return Err(VaultError::new(ErrorCode::FolderNotEmpty));
        }
        self.folders.retain(|f| f.id != id);
        for t in &mut self.trash {
            if t.document.folder == Some(id) {
                t.document.folder = None;
            }
        }
        Ok(())
    }

    // ----- history --------------------------------------------------------

    /// Replaces the current content of a document with a historical entry.
    /// The previous content remains in history; the restored state becomes a
    /// new revision on the next save.
    pub fn restore_history_entry(&mut self, doc: DocId, entry: HistoryId) -> VaultResult<()> {
        let d = self
            .document_mut(doc)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        let h = d
            .history_entry(entry)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        let title = h.title.duplicate();
        let text = h.text.duplicate();
        d.title = title;
        d.text = text;
        d.updated_at = now_unix();
        crate::history::mark_restore(d);
        Ok(())
    }

    /// Removes all history entries of a document except the most recent.
    pub fn purge_history(&mut self, doc: DocId) -> VaultResult<usize> {
        let d = self
            .document_mut(doc)
            .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
        let n = d.history.len().saturating_sub(1);
        if n > 0 {
            d.history.drain(..n);
        }
        Ok(n)
    }
}

impl Default for Vault {
    fn default() -> Self {
        Self::new()
    }
}

/// Validates a document title or folder name.
pub fn validate_name(name: &str) -> VaultResult<()> {
    if name.len() > limits::MAX_NAME_BYTES || name.chars().any(|c| c.is_control()) {
        return Err(VaultError::new(ErrorCode::InvalidName));
    }
    Ok(())
}

/// Validates a tag.
pub fn validate_tag(tag: &str) -> VaultResult<()> {
    if tag.is_empty()
        || tag.len() > limits::MAX_TAG_BYTES
        || tag
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == ',')
    {
        return Err(VaultError::new(ErrorCode::InvalidName));
    }
    Ok(())
}

const WELCOME_TEXT: &str = "# Welcome\n\
\n\
This vault is encrypted with a passphrase using the age file format.\n\
Everything you see here - titles, folders, tags, text and history - is\n\
stored only inside the encrypted file.\n\
\n\
## Getting around\n\
\n\
- `Cmd/Ctrl+N` new document\n\
- `Cmd/Ctrl+P` quick switcher, `Cmd/Ctrl+Shift+P` command palette\n\
- `Cmd/Ctrl+F` find and replace, `Cmd/Ctrl+Shift+F` search the vault\n\
- `Cmd/Ctrl+Shift+L` lock now\n\
\n\
## Remember\n\
\n\
- There is no password reset. Without the passphrase this vault cannot be opened.\n\
- Copy the vault file somewhere safe (for example a cloud drive) as a backup.\n\
- Read the recovery checklist in Settings before you rely on this app.\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_rules() {
        let mut v = Vault::new();
        let a = v.create_folder("A", None).unwrap();
        let b = v.create_folder("B", Some(a)).unwrap();
        assert_eq!(v.folder_depth(b), Some(2));
        assert!(v.folder_is_within(b, a));
        assert!(v.create_folder("", None).is_err());
        assert!(v.create_folder("bad\nname", None).is_err());
        assert_eq!(
            v.delete_folder(a).unwrap_err().code(),
            ErrorCode::FolderNotEmpty
        );
        v.delete_folder(b).unwrap();
        v.delete_folder(a).unwrap();
        assert!(v.folders.is_empty());
    }

    #[test]
    fn depth_limit_enforced() {
        let mut v = Vault::new();
        let mut parent = None;
        for i in 0..limits::MAX_FOLDER_DEPTH {
            parent = Some(v.create_folder(&format!("f{i}"), parent).unwrap());
        }
        assert_eq!(
            v.create_folder("too deep", parent).unwrap_err().code(),
            ErrorCode::FolderTooDeep
        );
    }

    #[test]
    fn trash_roundtrip_and_folder_fallback() {
        let mut v = Vault::new();
        let f = v.create_folder("F", None).unwrap();
        let d = v.create_document("Doc", Some(f)).unwrap();
        v.trash_document(d).unwrap();
        assert!(v.document(d).is_none());
        v.delete_folder(f).unwrap();
        v.restore_from_trash(d).unwrap();
        assert_eq!(v.document(d).unwrap().folder, None);
        v.trash_document(d).unwrap();
        v.purge_from_trash(d).unwrap();
        assert!(v.trash.is_empty());
    }

    #[test]
    fn tags_are_validated_and_deduplicated() {
        let mut v = Vault::new();
        let d = v.create_document("Doc", None).unwrap();
        v.set_tags(d, &["work", "work", "", "keys"]).unwrap();
        assert_eq!(v.document(d).unwrap().tags.len(), 2);
        assert!(v.set_tags(d, &["has space"]).is_err());
        assert_eq!(v.all_tags(), vec!["keys", "work"]);
    }
}
