//! Import of user-selected text files and export of documents.
//!
//! * Import reads one user-selected file, bounded and UTF-8 validated, and
//!   leaves the original untouched. Existing plaintext copies are not erased.
//! * Encrypted export writes a standard age passphrase file containing only
//!   the document's UTF-8 text, decryptable with the reference `age`/`rage`
//!   tools (`age -d file.age`).
//! * Plaintext export requires a [`PlaintextExportConfirmation`], which can
//!   only be constructed by an explicit confirmation call naming the exact
//!   destination.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::crypto::{self, WorkFactor};
use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::limits;
use crate::model::{DocMode, Document};
use crate::secret::{Passphrase, SecretText};
use crate::storage::{self, Faults};

/// Reads a user-selected UTF-8 text file into a new document (not yet added
/// to any vault). The title is the file stem.
pub fn import_text_file(path: &Path) -> VaultResult<Document> {
    if !storage::check_target(path)? {
        return Err(VaultError::new(ErrorCode::NotFound));
    }
    let file = File::open(path).map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?;
    let len = file
        .metadata()
        .map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?
        .len();
    if len > limits::MAX_IMPORT_BYTES as u64 {
        return Err(VaultError::new(ErrorCode::ImportTooLarge));
    }
    let mut buf = Zeroizing::new(Vec::with_capacity(len as usize + 1));
    file.take(limits::MAX_IMPORT_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?;
    if buf.len() > limits::MAX_IMPORT_BYTES {
        return Err(VaultError::new(ErrorCode::ImportTooLarge));
    }
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_markdown = path
        .extension()
        .map(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            e == "md" || e == "markdown"
        })
        .unwrap_or(false);
    document_from_bytes(&title, &buf, is_markdown)
}

/// Builds a document from UTF-8 bytes (BOM stripped, CRLF preserved).
pub fn document_from_bytes(title: &str, bytes: &[u8], markdown: bool) -> VaultResult<Document> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| VaultError::new(ErrorCode::ImportNotUtf8))?;
    let mut title: String = title.chars().filter(|c| !c.is_control()).collect();
    while title.len() > limits::MAX_NAME_BYTES {
        title.pop();
    }
    let mut doc = Document::new(&title, None);
    doc.text = SecretText::from(text);
    doc.mode = if markdown {
        DocMode::Markdown
    } else {
        DocMode::Plain
    };
    Ok(doc)
}

/// Exports a document's text as a standalone age passphrase-encrypted file.
/// Replaces a regular file at `dest` (the caller confirms overwrite); never
/// follows symlinks.
pub fn export_encrypted(
    text: &SecretText,
    dest: &Path,
    passphrase: &Passphrase,
    work: WorkFactor,
) -> VaultResult<()> {
    if passphrase.is_empty() {
        return Err(VaultError::new(ErrorCode::PassphraseEmpty));
    }
    let ciphertext = crypto::encrypt(passphrase, work, text.expose().as_bytes())?;
    storage::write_atomic(dest, &ciphertext, None, Faults::NONE)?;
    Ok(())
}

/// Proof that the user explicitly confirmed a plaintext export to a specific
/// destination. Cannot be constructed outside [`confirm_plaintext_export`].
#[derive(Debug)]
pub struct PlaintextExportConfirmation {
    dest: PathBuf,
}

/// The phrase a user types to confirm a plaintext export in the CLI.
pub const PLAINTEXT_CONFIRM_PHRASE: &str = "export plaintext";

/// Confirms a plaintext export of `dest`. `acknowledged` must be the result
/// of an explicit user action (a dedicated dialog checkbox + button, or the
/// typed phrase in the CLI) that showed the destination.
pub fn confirm_plaintext_export(
    dest: &Path,
    acknowledged: bool,
) -> VaultResult<PlaintextExportConfirmation> {
    if !acknowledged {
        return Err(VaultError::new(ErrorCode::PlaintextExportNotConfirmed));
    }
    Ok(PlaintextExportConfirmation {
        dest: dest.to_path_buf(),
    })
}

/// Writes plaintext to the confirmed destination. The confirmation is
/// consumed and only valid for the exact path it names.
pub fn export_plaintext(
    text: &SecretText,
    confirmation: PlaintextExportConfirmation,
) -> VaultResult<PathBuf> {
    let dest = confirmation.dest;
    storage::write_atomic(&dest, text.expose().as_bytes(), None, Faults::NONE)?;
    Ok(dest)
}

/// Decrypts a standalone encrypted export (or any age passphrase file with
/// UTF-8 text) into a new document.
pub fn import_encrypted_text(path: &Path, passphrase: &Passphrase) -> VaultResult<Document> {
    let ciphertext = storage::read_bounded(path, limits::MAX_ENCRYPTED_BYTES)?;
    let plain = crypto::decrypt(passphrase, &ciphertext, limits::MAX_DOCUMENT_BYTES)?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = name.strip_suffix(".age").unwrap_or(&name);
    let title = title
        .strip_suffix(".txt")
        .or_else(|| title.strip_suffix(".md"))
        .unwrap_or(title);
    document_from_bytes(title, &plain, true)
}

/// A filesystem-safe default filename for exporting a document.
pub fn suggested_filename(title: &str, extension: &str) -> String {
    let mut s: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s = s.trim().to_owned();
    if s.is_empty() {
        s = "document".to_owned();
    }
    s.truncate(s.char_indices().nth(60).map(|(i, _)| i).unwrap_or(s.len()));
    format!("{s}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_validates_utf8_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("note.md");
        std::fs::write(&ok, "\u{feff}# Héllo\r\nworld").unwrap();
        let d = import_text_file(&ok).unwrap();
        assert_eq!(d.title.expose(), "note");
        assert_eq!(d.text.expose(), "# Héllo\r\nworld");
        assert_eq!(d.mode, DocMode::Markdown);
        // Original untouched.
        assert_eq!(
            std::fs::read(&ok).unwrap().len(),
            "\u{feff}# Héllo\r\nworld".len()
        );

        let bad = dir.path().join("bin.txt");
        std::fs::write(&bad, [0xffu8, 0xfe, 0x00]).unwrap();
        assert_eq!(
            import_text_file(&bad).unwrap_err().code(),
            ErrorCode::ImportNotUtf8
        );

        let big = dir.path().join("big.txt");
        std::fs::write(&big, vec![b'a'; limits::MAX_IMPORT_BYTES + 1]).unwrap();
        assert_eq!(
            import_text_file(&big).unwrap_err().code(),
            ErrorCode::ImportTooLarge
        );
    }

    #[test]
    fn plaintext_export_requires_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.txt");
        assert_eq!(
            confirm_plaintext_export(&dest, false).unwrap_err().code(),
            ErrorCode::PlaintextExportNotConfirmed
        );
        assert!(!dest.exists(), "cancellation must create no output");
        let c = confirm_plaintext_export(&dest, true).unwrap();
        export_plaintext(&SecretText::from("synthetic"), c).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "synthetic");
    }

    #[test]
    fn encrypted_export_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("doc.txt.age");
        let pass = Passphrase::from("export pass");
        export_encrypted(
            &SecretText::from("ünïcode body"),
            &dest,
            &pass,
            WorkFactor::TESTING,
        )
        .unwrap();
        let back = import_encrypted_text(&dest, &pass).unwrap();
        assert_eq!(back.text.expose(), "ünïcode body");
        assert_eq!(back.title.expose(), "doc");
    }

    #[test]
    fn filename_suggestions_are_safe() {
        assert_eq!(
            suggested_filename("../../etc/passwd", "txt"),
            "______etc_passwd.txt"
        );
        assert_eq!(suggested_filename("   ", "md"), "document.md");
    }
}
