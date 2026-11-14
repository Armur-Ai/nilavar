//! Closed error vocabulary.
//!
//! Every failure is mapped to a fixed [`ErrorCode`]. Codes have stable string
//! identifiers and fixed, human-readable messages. Errors never carry
//! passphrases, document text, titles, filesystem paths or raw error chains
//! from dependencies, so they are always safe to display or include in a
//! diagnostic report.

use std::fmt;
use std::io;

/// Stable, non-sensitive error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    // --- Opening / decrypting (1xx)
    /// Wrong passphrase, or the file was not encrypted with a passphrase we can open.
    WrongPassphraseOrNotAVault,
    /// The age header is malformed or truncated.
    InvalidHeader,
    /// The header authentication code did not verify (tampered or corrupted header).
    HeaderTampered,
    /// The encrypted body failed authentication (tampered, reordered or corrupted).
    BodyTampered,
    /// The encrypted file ended early.
    Truncated,
    /// The file is an age file encrypted to public-key recipients, not a passphrase.
    NotPassphraseEncrypted,
    /// The file declares a newer or unknown age format version.
    UnknownAgeFormat,
    /// The file requests an scrypt work factor above the accepted maximum.
    ExcessiveWorkFactor,
    /// The encrypted input exceeds the accepted size.
    EncryptedInputTooLarge,
    /// The decrypted payload exceeds the accepted size.
    PayloadTooLarge,

    // --- Payload validation (2xx)
    /// The payload is not valid UTF-8 JSON of the expected shape.
    MalformedPayload,
    /// The payload schema version is not supported by this build.
    UnsupportedSchema,
    /// The payload nests deeper than allowed.
    ExcessiveNesting,
    /// Two objects share an identifier.
    DuplicateId,
    /// An identifier is not well-formed.
    InvalidId,
    /// A reference points at a missing object.
    DanglingReference,
    /// The folder graph contains a cycle.
    FolderCycle,
    /// The folder graph is nested too deeply.
    FolderTooDeep,
    /// A document exceeds the per-document size limit.
    DocumentTooLarge,
    /// A name, title or tag exceeds the size limit or contains forbidden characters.
    InvalidName,
    /// Too many documents, folders, tags, revisions or trash entries.
    TooManyItems,

    // --- Storage (3xx)
    /// The vault file could not be read.
    ReadFailed,
    /// Writing the temporary encrypted file failed.
    WriteFailed,
    /// Flushing data to disk failed.
    FlushFailed,
    /// Atomically replacing the vault file failed.
    ReplaceFailed,
    /// The disk is full.
    DiskFull,
    /// Permission denied.
    PermissionDenied,
    /// The target is a symbolic link; refusing to follow it.
    UnexpectedSymlink,
    /// The target path is not a regular file.
    NotARegularFile,
    /// The vault file changed on disk since it was opened.
    ConflictDetected,
    /// The vault is open in another cooperating application instance.
    VaultInUse,
    /// The destination already exists.
    AlreadyExists,
    /// The vault file does not exist.
    NotFound,

    // --- Session / workflow (4xx)
    /// The two passphrase entries do not match.
    PassphraseMismatch,
    /// The passphrase is empty.
    PassphraseEmpty,
    /// The operation requires an unlocked vault.
    Locked,
    /// The requested document, folder or revision does not exist.
    NoSuchItem,
    /// The folder is not empty.
    FolderNotEmpty,
    /// The imported file is not valid UTF-8 text.
    ImportNotUtf8,
    /// The imported file exceeds the import size limit.
    ImportTooLarge,
    /// A plaintext export was attempted without explicit confirmation.
    PlaintextExportNotConfirmed,
    /// Saving would exceed the payload size limit; edits were kept in memory.
    SaveWouldExceedLimit,
    /// Encryption failed inside the crypto library.
    EncryptFailed,
    /// The work queue is busy; try again.
    Busy,
    /// An operation was cancelled.
    Cancelled,
    /// An internal invariant was violated.
    Internal,
}

impl ErrorCode {
    /// Stable identifier, e.g. `NV-101`.
    pub const fn id(self) -> &'static str {
        use ErrorCode::*;
        match self {
            WrongPassphraseOrNotAVault => "NV-101",
            InvalidHeader => "NV-102",
            HeaderTampered => "NV-103",
            BodyTampered => "NV-104",
            Truncated => "NV-105",
            NotPassphraseEncrypted => "NV-106",
            UnknownAgeFormat => "NV-107",
            ExcessiveWorkFactor => "NV-108",
            EncryptedInputTooLarge => "NV-109",
            PayloadTooLarge => "NV-110",
            MalformedPayload => "NV-201",
            UnsupportedSchema => "NV-202",
            ExcessiveNesting => "NV-203",
            DuplicateId => "NV-204",
            InvalidId => "NV-205",
            DanglingReference => "NV-206",
            FolderCycle => "NV-207",
            FolderTooDeep => "NV-208",
            DocumentTooLarge => "NV-209",
            InvalidName => "NV-210",
            TooManyItems => "NV-211",
            ReadFailed => "NV-301",
            WriteFailed => "NV-302",
            FlushFailed => "NV-303",
            ReplaceFailed => "NV-304",
            DiskFull => "NV-305",
            PermissionDenied => "NV-306",
            UnexpectedSymlink => "NV-307",
            NotARegularFile => "NV-308",
            ConflictDetected => "NV-309",
            VaultInUse => "NV-310",
            AlreadyExists => "NV-311",
            NotFound => "NV-312",
            PassphraseMismatch => "NV-401",
            PassphraseEmpty => "NV-402",
            Locked => "NV-403",
            NoSuchItem => "NV-404",
            FolderNotEmpty => "NV-405",
            ImportNotUtf8 => "NV-406",
            ImportTooLarge => "NV-407",
            PlaintextExportNotConfirmed => "NV-408",
            SaveWouldExceedLimit => "NV-409",
            EncryptFailed => "NV-410",
            Busy => "NV-411",
            Cancelled => "NV-412",
            Internal => "NV-499",
        }
    }

    /// Fixed user-facing message. Never interpolates user data.
    pub const fn message(self) -> &'static str {
        use ErrorCode::*;
        match self {
            WrongPassphraseOrNotAVault => {
                "The passphrase is incorrect, or this file is not a vault that can be opened with a passphrase."
            }
            InvalidHeader => {
                "The file is not a valid encrypted vault (the header is malformed or truncated)."
            }
            HeaderTampered => {
                "The vault header failed authentication. The file is corrupted or was modified."
            }
            BodyTampered => {
                "The encrypted contents failed authentication. The file is corrupted or was modified."
            }
            Truncated => "The encrypted file ends early. It may be incomplete or still syncing.",
            NotPassphraseEncrypted => {
                "This age file is encrypted to keys, not to a passphrase, and cannot be opened here."
            }
            UnknownAgeFormat => {
                "This file uses an encryption format version this build does not support."
            }
            ExcessiveWorkFactor => {
                "The file demands an unreasonably expensive passphrase check and was rejected."
            }
            EncryptedInputTooLarge => "The encrypted file is larger than this build accepts.",
            PayloadTooLarge => "The decrypted vault is larger than this build accepts.",
            MalformedPayload => {
                "The vault decrypted, but its contents are not in the expected format."
            }
            UnsupportedSchema => {
                "The vault was written by an incompatible version. It was not modified."
            }
            ExcessiveNesting => "The vault contents are nested too deeply.",
            DuplicateId => "The vault contents contain duplicate identifiers.",
            InvalidId => "The vault contents contain a malformed identifier.",
            DanglingReference => "The vault contents reference an item that does not exist.",
            FolderCycle => "The vault's folders form a cycle.",
            FolderTooDeep => "Folders are nested deeper than allowed.",
            DocumentTooLarge => "A document is larger than the 2 MiB per-document limit.",
            InvalidName => "A name, title or tag is too long or contains forbidden characters.",
            TooManyItems => "The vault contains more items than this build allows.",
            ReadFailed => "The file could not be read.",
            WriteFailed => {
                "Writing the encrypted file failed. The previous vault file is unchanged."
            }
            FlushFailed => {
                "Flushing the encrypted file to disk failed. The previous vault file is unchanged."
            }
            ReplaceFailed => {
                "Replacing the vault file failed. The previous vault file is unchanged."
            }
            DiskFull => "The disk is full. The previous vault file is unchanged.",
            PermissionDenied => "Permission was denied for this location.",
            UnexpectedSymlink => "The location is a symbolic link. For safety it was not followed.",
            NotARegularFile => "The location is not a regular file.",
            ConflictDetected => {
                "The vault file changed on disk after it was opened. Nothing was overwritten; save a conflict copy instead."
            }
            VaultInUse => "This vault is already open in another window or instance of the app.",
            AlreadyExists => "A file already exists at the destination.",
            NotFound => "The file does not exist.",
            PassphraseMismatch => "The passphrase entries do not match.",
            PassphraseEmpty => "The passphrase must not be empty.",
            Locked => "The vault is locked.",
            NoSuchItem => "The item no longer exists.",
            FolderNotEmpty => "The folder is not empty.",
            ImportNotUtf8 => "The file is not UTF-8 text and was not imported.",
            ImportTooLarge => "The file is larger than the 2 MiB import limit.",
            PlaintextExportNotConfirmed => "Plaintext export requires explicit confirmation.",
            SaveWouldExceedLimit => {
                "Saving would exceed the 32 MiB vault limit. Your edits are still open; nothing was overwritten. Purge history or trash to free space."
            }
            EncryptFailed => "Encryption failed. Nothing was written.",
            Busy => "Another operation is still running. Try again in a moment.",
            Cancelled => "The operation was cancelled.",
            Internal => "An internal error occurred.",
        }
    }

    /// All codes, for documentation and diagnostics schema tests.
    pub const ALL: &'static [ErrorCode] = {
        use ErrorCode::*;
        &[
            WrongPassphraseOrNotAVault,
            InvalidHeader,
            HeaderTampered,
            BodyTampered,
            Truncated,
            NotPassphraseEncrypted,
            UnknownAgeFormat,
            ExcessiveWorkFactor,
            EncryptedInputTooLarge,
            PayloadTooLarge,
            MalformedPayload,
            UnsupportedSchema,
            ExcessiveNesting,
            DuplicateId,
            InvalidId,
            DanglingReference,
            FolderCycle,
            FolderTooDeep,
            DocumentTooLarge,
            InvalidName,
            TooManyItems,
            ReadFailed,
            WriteFailed,
            FlushFailed,
            ReplaceFailed,
            DiskFull,
            PermissionDenied,
            UnexpectedSymlink,
            NotARegularFile,
            ConflictDetected,
            VaultInUse,
            AlreadyExists,
            NotFound,
            PassphraseMismatch,
            PassphraseEmpty,
            Locked,
            NoSuchItem,
            FolderNotEmpty,
            ImportNotUtf8,
            ImportTooLarge,
            PlaintextExportNotConfirmed,
            SaveWouldExceedLimit,
            EncryptFailed,
            Busy,
            Cancelled,
            Internal,
        ]
    };
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.id(), self.message())
    }
}

/// A vault error: a fixed code plus, at most, the non-sensitive kind of an
/// underlying I/O error. It never stores messages from dependencies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultError {
    code: ErrorCode,
    io_kind: Option<io::ErrorKind>,
}

impl VaultError {
    /// Creates an error from a code.
    pub const fn new(code: ErrorCode) -> Self {
        Self {
            code,
            io_kind: None,
        }
    }

    /// Maps an I/O error to a storage code, keeping only its kind.
    pub fn io(default: ErrorCode, err: &io::Error) -> Self {
        let code = match err.kind() {
            io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            io::ErrorKind::NotFound => ErrorCode::NotFound,
            io::ErrorKind::AlreadyExists => ErrorCode::AlreadyExists,
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => ErrorCode::DiskFull,
            _ => default,
        };
        Self {
            code,
            io_kind: Some(err.kind()),
        }
    }

    /// The error code.
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// The kind of the underlying I/O error, if any.
    pub const fn io_kind(&self) -> Option<io::ErrorKind> {
        self.io_kind
    }
}

impl From<ErrorCode> for VaultError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.code, f)
    }
}

impl std::error::Error for VaultError {}

/// Result alias used throughout the core.
pub type VaultResult<T> = Result<T, VaultError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn codes_are_unique_and_messages_nonempty() {
        let mut ids = HashSet::new();
        for code in ErrorCode::ALL {
            assert!(ids.insert(code.id()), "duplicate id {}", code.id());
            assert!(!code.message().is_empty());
            assert!(code.id().starts_with("NV-"));
        }
    }

    #[test]
    fn io_errors_keep_only_kind() {
        let err = io::Error::new(io::ErrorKind::PermissionDenied, "/secret/path/canary");
        let v = VaultError::io(ErrorCode::WriteFailed, &err);
        assert_eq!(v.code(), ErrorCode::PermissionDenied);
        let shown = format!("{v} {v:?}");
        assert!(!shown.contains("canary"));
        assert!(!shown.contains("/secret"));
    }
}
