//! Explicit, documented resource limits (see `docs/FORMAT.md`).
//!
//! Every limit is checked *before* allocating structures proportional to the
//! untrusted input wherever practical.

/// Maximum UTF-8 size of a single document's text.
pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

/// Maximum decrypted payload size, including history and trash.
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;

/// Maximum encrypted vault (or encrypted import) file size accepted.
pub const MAX_ENCRYPTED_BYTES: usize = 64 * 1024 * 1024;

/// Maximum number of live documents.
pub const MAX_DOCUMENTS: usize = 5_000;

/// Maximum number of folders.
pub const MAX_FOLDERS: usize = 1_000;

/// Maximum folder nesting depth (root-level folders have depth 1).
pub const MAX_FOLDER_DEPTH: usize = 12;

/// Maximum number of documents in the trash.
pub const MAX_TRASH: usize = 5_000;

/// Maximum number of retained revisions per document.
///
/// When a new revision would exceed this count the *oldest* revision of that
/// document is dropped. This is a documented retention policy; the payload
/// size limit is never satisfied by pruning history silently.
pub const MAX_REVISIONS_PER_DOCUMENT: usize = 40;

/// Maximum total revisions across the vault (live + trash).
pub const MAX_TOTAL_REVISIONS: usize = 20_000;

/// Maximum UTF-8 size of a document title or folder name.
pub const MAX_NAME_BYTES: usize = 256;

/// Maximum tags per document.
pub const MAX_TAGS_PER_DOCUMENT: usize = 32;

/// Maximum UTF-8 size of a single tag.
pub const MAX_TAG_BYTES: usize = 64;

/// Maximum JSON nesting depth accepted in a decrypted payload.
pub const MAX_JSON_DEPTH: usize = 16;

/// Maximum size of an imported plaintext file.
pub const MAX_IMPORT_BYTES: usize = MAX_DOCUMENT_BYTES;

/// Default scrypt work factor (log2 N) used when saving. This matches the
/// reference age implementation's passphrase default.
pub const DEFAULT_SCRYPT_LOG_N: u8 = 18;

/// Maximum scrypt work factor accepted when opening a file. 2^20 needs about
/// 1 GiB of memory; anything above is rejected before attempting the KDF.
pub const MAX_SCRYPT_LOG_N: u8 = 20;

/// Minimum work factor the API will produce. Values below the default are only
/// intended for the test suite and fuzzing (they are still valid age files).
pub const MIN_SCRYPT_LOG_N: u8 = 10;

/// Number of encrypted last-known-good recovery copies kept per vault.
pub const RECOVERY_COPIES_KEPT: usize = 5;
