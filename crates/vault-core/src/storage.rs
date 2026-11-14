//! Failure-safe storage of ciphertext.
//!
//! * [`write_atomic`] writes a same-directory temporary file created
//!   exclusively with owner-only permissions, writes and `fsync`s it, renames
//!   it over the destination and syncs the directory. The previous file is
//!   never truncated; a failure at any step leaves it untouched and removes
//!   the temporary file.
//! * [`read_bounded`] reads a regular, non-symlink file up to a size limit.
//! * [`Fingerprint`] identifies exactly which encrypted bytes a session
//!   opened, so external modifications are detected before overwriting.
//! * [`RecoveryStore`] keeps a bounded set of encrypted last-known-good and
//!   unsaved-changes copies in the application data directory.
//! * [`SessionLock`] coordinates cooperating app instances on one machine. It
//!   is not a distributed lock and does not coordinate with cloud sync clients.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::ids::{VaultId, random_bytes};
use crate::limits;
use crate::paths::create_private_dir_all;
use crate::time::{format_compact, now_unix};

/// SHA-256 of a vault file's ciphertext.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// Fingerprints ciphertext bytes.
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Short hex form for display (ciphertext hashes are not sensitive).
    pub fn short_hex(&self) -> String {
        self.0[..6].iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fingerprint({})", self.short_hex())
    }
}

/// A step at which a write can be made to fail (test instrumentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultPoint {
    /// Creating the temporary file.
    CreateTemp,
    /// Midway through writing the temporary file (simulates disk-full).
    Write,
    /// Flushing the temporary file to stable storage.
    Flush,
    /// Renaming the temporary file over the destination.
    Replace,
}

/// Optional fault injection for storage tests. Production code always uses
/// [`Faults::NONE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    /// Fail at this point.
    pub fail_at: Option<FaultPoint>,
}

impl Faults {
    /// No injected faults.
    pub const NONE: Faults = Faults { fail_at: None };

    fn check(&self, at: FaultPoint) -> io::Result<()> {
        if self.fail_at == Some(at) {
            let kind = match at {
                FaultPoint::Write => io::ErrorKind::StorageFull,
                _ => io::ErrorKind::Other,
            };
            Err(io::Error::new(kind, "injected fault"))
        } else {
            Ok(())
        }
    }
}

/// Outcome details of an atomic write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteReport {
    /// Whether syncing the parent directory succeeded (or was not needed).
    pub dir_synced: bool,
}

/// Refuses symlinks and non-regular files at `path` (if it exists).
pub fn check_target(path: &Path) -> VaultResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(VaultError::new(ErrorCode::UnexpectedSymlink)),
        Ok(m) if !m.is_file() => Err(VaultError::new(ErrorCode::NotARegularFile)),
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(VaultError::io(ErrorCode::ReadFailed, &e)),
    }
}

/// Reads a regular file of at most `max` bytes.
pub fn read_bounded(path: &Path, max: usize) -> VaultResult<Vec<u8>> {
    if !check_target(path)? {
        return Err(VaultError::new(ErrorCode::NotFound));
    }
    let file = File::open(path).map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?;
    let len = file
        .metadata()
        .map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?
        .len();
    if len > max as u64 {
        return Err(VaultError::new(ErrorCode::EncryptedInputTooLarge));
    }
    let mut buf = Vec::with_capacity(len as usize);
    file.take(max as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?;
    if buf.len() > max {
        return Err(VaultError::new(ErrorCode::EncryptedInputTooLarge));
    }
    Ok(buf)
}

fn temp_path_for(dest: &Path) -> VaultResult<PathBuf> {
    let parent = parent_dir(dest);
    let name = dest
        .file_name()
        .ok_or(VaultError::new(ErrorCode::NotARegularFile))?
        .to_string_lossy();
    let r: [u8; 6] = random_bytes();
    let suffix: String = r.iter().map(|b| format!("{b:02x}")).collect();
    Ok(parent.join(format!(".{name}.{suffix}.tmp")))
}

fn parent_dir(p: &Path) -> PathBuf {
    match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Creates a new file exclusively (never following or replacing anything
/// existing) with owner-only permissions on Unix.
fn create_exclusive(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Atomically replaces (or creates) `dest` with `bytes`.
///
/// `expected` enables conflict detection: if `Some`, the current file must
/// exist with exactly that fingerprint, checked before writing and again
/// immediately before the rename.
pub fn write_atomic(
    dest: &Path,
    bytes: &[u8],
    expected: Option<Fingerprint>,
    faults: Faults,
) -> VaultResult<WriteReport> {
    let exists = check_target(dest)?;
    if let Some(fp) = expected {
        verify_unchanged(dest, exists, fp)?;
    }
    let tmp = temp_path_for(dest)?;
    let result = write_temp_and_replace(dest, &tmp, bytes, expected, faults);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Writes a new file at `dest`, failing if anything already exists there.
pub fn write_new(dest: &Path, bytes: &[u8], faults: Faults) -> VaultResult<WriteReport> {
    if check_target(dest)? {
        return Err(VaultError::new(ErrorCode::AlreadyExists));
    }
    let tmp = temp_path_for(dest)?;
    let result = (|| {
        write_temp(&tmp, bytes, faults)?;
        // Link-then-unlink would be ideal; a final existence check keeps the
        // window small and we never replace an existing regular file blindly.
        if check_target(dest)? {
            return Err(VaultError::new(ErrorCode::AlreadyExists));
        }
        faults
            .check(FaultPoint::Replace)
            .map_err(|e| VaultError::io(ErrorCode::ReplaceFailed, &e))?;
        fs::rename(&tmp, dest).map_err(|e| VaultError::io(ErrorCode::ReplaceFailed, &e))?;
        Ok(WriteReport {
            dir_synced: sync_dir(&parent_dir(dest)),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn verify_unchanged(dest: &Path, exists: bool, fp: Fingerprint) -> VaultResult<()> {
    if !exists {
        return Err(VaultError::new(ErrorCode::ConflictDetected));
    }
    let current = read_bounded(dest, limits::MAX_ENCRYPTED_BYTES).map_err(|e| match e.code() {
        ErrorCode::EncryptedInputTooLarge | ErrorCode::NotFound => {
            VaultError::new(ErrorCode::ConflictDetected)
        }
        _ => e,
    })?;
    if Fingerprint::of(&current) != fp {
        return Err(VaultError::new(ErrorCode::ConflictDetected));
    }
    Ok(())
}

fn write_temp(tmp: &Path, bytes: &[u8], faults: Faults) -> VaultResult<()> {
    faults
        .check(FaultPoint::CreateTemp)
        .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
    let mut f = create_exclusive(tmp).map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
    if faults.fail_at == Some(FaultPoint::Write) {
        // Simulate a partial write followed by an error (e.g. disk full).
        let half = bytes.len() / 2;
        f.write_all(&bytes[..half])
            .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
        let e = faults.check(FaultPoint::Write).unwrap_err();
        return Err(VaultError::io(ErrorCode::WriteFailed, &e));
    }
    f.write_all(bytes)
        .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
    faults
        .check(FaultPoint::Flush)
        .map_err(|e| VaultError::io(ErrorCode::FlushFailed, &e))?;
    // On Apple platforms std's sync_all issues F_FULLFSYNC.
    f.sync_all()
        .map_err(|e| VaultError::io(ErrorCode::FlushFailed, &e))?;
    Ok(())
}

fn write_temp_and_replace(
    dest: &Path,
    tmp: &Path,
    bytes: &[u8],
    expected: Option<Fingerprint>,
    faults: Faults,
) -> VaultResult<WriteReport> {
    write_temp(tmp, bytes, faults)?;
    // Re-check immediately before replacing to narrow the race window with
    // other writers. A residual window remains (documented).
    let exists = check_target(dest)?;
    if let Some(fp) = expected {
        verify_unchanged(dest, exists, fp)?;
    }
    faults
        .check(FaultPoint::Replace)
        .map_err(|e| VaultError::io(ErrorCode::ReplaceFailed, &e))?;
    fs::rename(tmp, dest).map_err(|e| VaultError::io(ErrorCode::ReplaceFailed, &e))?;
    Ok(WriteReport {
        dir_synced: sync_dir(&parent_dir(dest)),
    })
}

/// Syncs directory metadata so the rename is durable (Unix). Returns
/// whether it succeeded; failure does not undo the completed rename.
fn sync_dir(dir: &Path) -> bool {
    #[cfg(unix)]
    {
        File::open(dir).and_then(|d| d.sync_all()).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        true
    }
}

/// Removes stale temporary files left by an interrupted write next to `dest`.
/// Only files matching this app's exact temp-name pattern are touched.
pub fn clean_stale_temps(dest: &Path) -> usize {
    let Some(name) = dest.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return 0;
    };
    let prefix = format!(".{name}.");
    let Ok(entries) = fs::read_dir(parent_dir(dest)) else {
        return 0;
    };
    let mut n = 0;
    for e in entries.flatten() {
        let fname = e.file_name().to_string_lossy().into_owned();
        if fname.starts_with(&prefix)
            && fname.ends_with(".tmp")
            && fname.len() == prefix.len() + 12 + 4
            && e.file_type().map(|t| t.is_file()).unwrap_or(false)
            && fs::remove_file(e.path()).is_ok()
        {
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// Recovery copies

/// Kind of a recovery copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryKind {
    /// The vault file as it was before being replaced by a save.
    LastKnownGood,
    /// Unsaved session state written when the main save failed during lock.
    Unsaved,
}

impl RecoveryKind {
    fn prefix(self) -> &'static str {
        match self {
            RecoveryKind::LastKnownGood => "lkg-",
            RecoveryKind::Unsaved => "unsaved-",
        }
    }
}

/// A recovery copy on disk.
#[derive(Clone, Debug)]
pub struct RecoveryCopy {
    /// Location.
    pub path: PathBuf,
    /// Kind.
    pub kind: RecoveryKind,
    /// File name (contains only a timestamp, never content).
    pub name: String,
}

/// Encrypted recovery copies for one vault, stored in the app data directory
/// (`recovery/<vault key>/`). These sit on the same disk as the app data and
/// are **not** an independent backup.
#[derive(Clone, Debug)]
pub struct RecoveryStore {
    dir: PathBuf,
}

impl RecoveryStore {
    /// The store for `vault_id` under `recovery_root`. The directory name is a
    /// keyed hash of the vault id, not the id itself.
    pub fn for_vault(recovery_root: &Path, vault_id: VaultId) -> Self {
        let mut h = Sha256::new();
        h.update(b"nilavar-recovery-dir-v1");
        h.update(vault_id.to_hex().as_bytes());
        let d = h.finalize();
        let key: String = d[..8].iter().map(|b| format!("{b:02x}")).collect();
        Self {
            dir: recovery_root.join(key),
        }
    }

    /// Directory holding this vault's copies.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes an encrypted copy, then prunes to the retention limit.
    pub fn store(&self, kind: RecoveryKind, ciphertext: &[u8]) -> VaultResult<PathBuf> {
        create_private_dir_all(&self.dir)
            .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
        let r: [u8; 3] = random_bytes();
        let name = format!(
            "{}{}-{:02x}{:02x}{:02x}.nvault.age",
            kind.prefix(),
            format_compact(now_unix()),
            r[0],
            r[1],
            r[2]
        );
        let path = self.dir.join(name);
        write_new(&path, ciphertext, Faults::NONE)?;
        self.prune(kind, limits::RECOVERY_COPIES_KEPT);
        Ok(path)
    }

    /// Lists copies, newest first.
    pub fn list(&self) -> Vec<RecoveryCopy> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut v: Vec<RecoveryCopy> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let kind = if name.starts_with("lkg-") {
                    RecoveryKind::LastKnownGood
                } else if name.starts_with("unsaved-") {
                    RecoveryKind::Unsaved
                } else {
                    return None;
                };
                name.ends_with(".nvault.age").then(|| RecoveryCopy {
                    path: e.path(),
                    kind,
                    name,
                })
            })
            .collect();
        v.sort_by(|a, b| {
            let ka = a.name.trim_start_matches(a.kind.prefix());
            let kb = b.name.trim_start_matches(b.kind.prefix());
            kb.cmp(ka)
        });
        v
    }

    fn prune(&self, kind: RecoveryKind, keep: usize) {
        for old in self
            .list()
            .into_iter()
            .filter(|c| c.kind == kind)
            .skip(keep)
        {
            let _ = fs::remove_file(old.path);
        }
    }

    /// Deletes every copy (e.g. after a passphrase change). Deletion is a
    /// normal unlink; it does not guarantee erasure from the storage medium.
    pub fn purge_all(&self) -> usize {
        let mut n = 0;
        for c in self.list() {
            if fs::remove_file(&c.path).is_ok() {
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// Local coordination lock

/// Exclusive advisory lock held while a vault is unlocked in this app.
///
/// The lock file lives in the app data directory, named by a hash of the
/// vault's canonical path, so nothing is added next to a synced vault.
#[derive(Debug)]
pub struct SessionLock {
    _file: File,
    path: PathBuf,
}

impl SessionLock {
    /// Acquires the lock for `vault_path`, failing with
    /// [`ErrorCode::VaultInUse`] if another cooperating instance holds it.
    pub fn acquire(locks_dir: &Path, vault_path: &Path) -> VaultResult<Self> {
        create_private_dir_all(locks_dir)
            .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
        let canonical = fs::canonicalize(vault_path).unwrap_or_else(|_| vault_path.to_path_buf());
        let mut h = Sha256::new();
        h.update(b"nilavar-lock-v1");
        h.update(canonical.to_string_lossy().as_bytes());
        let d = h.finalize();
        let key: String = d[..12].iter().map(|b| format!("{b:02x}")).collect();
        let path = locks_dir.join(format!("{key}.lock"));
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts
            .open(&path)
            .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file, path }),
            Err(fs::TryLockError::WouldBlock) => Err(VaultError::new(ErrorCode::VaultInUse)),
            Err(fs::TryLockError::Error(e)) => Err(VaultError::io(ErrorCode::VaultInUse, &e)),
        }
    }

    /// The lock file path (in the app data directory).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("v.nvault.age");
        write_atomic(&p, b"one", None, Faults::NONE).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        let fp = Fingerprint::of(b"one");
        write_atomic(&p, b"two", Some(fp), Faults::NONE).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        // Stale expectation → conflict, file untouched.
        let err = write_atomic(&p, b"three", Some(fp), Faults::NONE).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ConflictDetected);
        assert_eq!(fs::read(&p).unwrap(), b"two");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn every_fault_preserves_previous_file_and_cleans_temp() {
        for at in [
            FaultPoint::CreateTemp,
            FaultPoint::Write,
            FaultPoint::Flush,
            FaultPoint::Replace,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("v.nvault.age");
            write_atomic(&p, b"good", None, Faults::NONE).unwrap();
            let err = write_atomic(
                &p,
                b"new contents",
                Some(Fingerprint::of(b"good")),
                Faults { fail_at: Some(at) },
            )
            .unwrap_err();
            assert_ne!(err.code(), ErrorCode::Internal);
            assert_eq!(fs::read(&p).unwrap(), b"good", "fault at {at:?}");
            let leftovers: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().collect();
            assert_eq!(leftovers.len(), 1, "temp file left after fault at {at:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_targets() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::write(&real, b"x").unwrap();
        let link = dir.path().join("link.nvault.age");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            write_atomic(&link, b"y", None, Faults::NONE)
                .unwrap_err()
                .code(),
            ErrorCode::UnexpectedSymlink
        );
        assert_eq!(
            read_bounded(&link, 10).unwrap_err().code(),
            ErrorCode::UnexpectedSymlink
        );
        assert_eq!(fs::read(&real).unwrap(), b"x");
    }

    #[test]
    fn write_new_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("n");
        write_new(&p, b"a", Faults::NONE).unwrap();
        assert_eq!(
            write_new(&p, b"b", Faults::NONE).unwrap_err().code(),
            ErrorCode::AlreadyExists
        );
        assert_eq!(fs::read(&p).unwrap(), b"a");
    }

    #[test]
    fn read_bounded_limits() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        fs::write(&p, vec![0u8; 100]).unwrap();
        assert_eq!(
            read_bounded(&p, 99).unwrap_err().code(),
            ErrorCode::EncryptedInputTooLarge
        );
        assert_eq!(read_bounded(&p, 100).unwrap().len(), 100);
    }

    #[test]
    fn recovery_store_retention() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::for_vault(dir.path(), VaultId::random());
        for i in 0..(limits::RECOVERY_COPIES_KEPT + 3) {
            store
                .store(RecoveryKind::LastKnownGood, &[i as u8])
                .unwrap();
        }
        store.store(RecoveryKind::Unsaved, b"u").unwrap();
        let all = store.list();
        assert_eq!(
            all.iter()
                .filter(|c| c.kind == RecoveryKind::LastKnownGood)
                .count(),
            limits::RECOVERY_COPIES_KEPT
        );
        assert_eq!(
            all.iter()
                .filter(|c| c.kind == RecoveryKind::Unsaved)
                .count(),
            1
        );
        assert_eq!(store.purge_all(), limits::RECOVERY_COPIES_KEPT + 1);
    }

    #[test]
    fn session_lock_excludes_second_holder() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("v.nvault.age");
        fs::write(&vault, b"x").unwrap();
        let locks = dir.path().join("locks");
        let a = SessionLock::acquire(&locks, &vault).unwrap();
        assert_eq!(
            SessionLock::acquire(&locks, &vault).unwrap_err().code(),
            ErrorCode::VaultInUse
        );
        drop(a);
        SessionLock::acquire(&locks, &vault).unwrap();
    }

    #[test]
    fn stale_temp_cleanup_is_narrow() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("v.nvault.age");
        fs::write(dir.path().join(".v.nvault.age.0123456789ab.tmp"), b"t").unwrap();
        fs::write(dir.path().join("unrelated.tmp"), b"t").unwrap();
        assert_eq!(clean_stale_temps(&p), 1);
        assert!(dir.path().join("unrelated.tmp").exists());
    }
}
