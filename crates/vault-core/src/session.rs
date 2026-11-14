//! An unlocked vault session: the decrypted model plus everything needed to
//! save it safely.
//!
//! Saving is split into [`Session::prepare_save`] (cheap, on the caller's
//! thread: history recording + payload encoding) and [`SaveJob::run`]
//! (expensive: scrypt + encryption + atomic write, intended for a worker
//! thread), followed by [`Session::finish_save`]. [`Session::save`] runs all
//! three synchronously for the CLI and tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use zeroize::Zeroizing;

use crate::crypto::{self, WorkFactor};
use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::format;
use crate::history::{self, SaveTrigger};
use crate::ids::{RevisionId, VaultId};
use crate::limits;
use crate::model::Vault;
use crate::paths::DataDirs;
use crate::secret::Passphrase;
use crate::storage::{self, Faults, Fingerprint, RecoveryKind, RecoveryStore, SessionLock};
use crate::time::{format_compact, now_unix};

/// Minimum spacing between last-known-good recovery copies.
pub const LKG_INTERVAL_SECS: i64 = 15 * 60;

/// Options shared by session operations.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// Application data directories (recovery copies, locks).
    pub data: DataDirs,
    /// scrypt work factor used when writing.
    pub work: WorkFactor,
    /// Storage fault injection (tests only).
    pub faults: Faults,
    /// Whether to take the local coordination lock.
    pub take_lock: bool,
}

impl SessionOptions {
    /// Production defaults for the given data directories.
    pub fn new(data: DataDirs) -> Self {
        Self {
            data,
            work: WorkFactor::DEFAULT,
            faults: Faults::NONE,
            take_lock: true,
        }
    }
}

/// Reads, decrypts and validates a vault file without creating a session.
/// Returns the vault and the fingerprint of the bytes that were read.
pub fn open_vault_file(path: &Path, passphrase: &Passphrase) -> VaultResult<(Vault, Fingerprint)> {
    let ciphertext = storage::read_bounded(path, limits::MAX_ENCRYPTED_BYTES)?;
    let fp = Fingerprint::of(&ciphertext);
    let plaintext = crypto::decrypt(passphrase, &ciphertext, limits::MAX_PAYLOAD_BYTES)?;
    let vault = format::decode(&plaintext)?;
    Ok((vault, fp))
}

/// An unlocked vault.
#[derive(Debug)]
pub struct Session {
    vault: Vault,
    passphrase: Arc<Passphrase>,
    path: PathBuf,
    fingerprint: Fingerprint,
    committed_revision: RevisionId,
    committed_parent: Option<RevisionId>,
    committed_counter: u64,
    committed_saved_at: i64,
    edit_generation: u64,
    saved_generation: u64,
    last_lkg_at: Option<i64>,
    opts: SessionOptions,
    lock: Option<SessionLock>,
    save_in_flight: bool,
}

/// Everything a worker needs to encrypt and write one save. Holds plaintext
/// in a zeroizing buffer; dropping the job wipes it.
pub struct SaveJob {
    plaintext: Zeroizing<Vec<u8>>,
    passphrase: Arc<Passphrase>,
    work: WorkFactor,
    path: PathBuf,
    expected: Fingerprint,
    lkg: Option<RecoveryStore>,
    faults: Faults,
    generation: u64,
    revision: RevisionId,
}

impl std::fmt::Debug for SaveJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SaveJob(<{} bytes redacted>)", self.plaintext.len())
    }
}

/// Successful save result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveSuccess {
    fingerprint: Fingerprint,
    generation: u64,
    revision: RevisionId,
    wrote_lkg: bool,
}

/// Result of running a [`SaveJob`].
pub type SaveResult = Result<SaveSuccess, (VaultError, u64)>;

impl SaveJob {
    /// Encrypts and atomically writes the vault. Safe to run on any thread.
    pub fn run(self) -> SaveResult {
        let generation = self.generation;
        self.run_inner().map_err(|e| (e, generation))
    }

    fn run_inner(self) -> VaultResult<SaveSuccess> {
        let ciphertext = crypto::encrypt(&self.passphrase, self.work, &self.plaintext)?;
        drop(self.plaintext);
        if ciphertext.len() > limits::MAX_ENCRYPTED_BYTES {
            return Err(VaultError::new(ErrorCode::SaveWouldExceedLimit));
        }
        // Keep an encrypted copy of the file we are about to replace.
        let mut wrote_lkg = false;
        if let Some(store) = &self.lkg
            && let Ok(previous) = storage::read_bounded(&self.path, limits::MAX_ENCRYPTED_BYTES)
            && Fingerprint::of(&previous) == self.expected
        {
            wrote_lkg = store.store(RecoveryKind::LastKnownGood, &previous).is_ok();
        }
        storage::write_atomic(&self.path, &ciphertext, Some(self.expected), self.faults)?;
        Ok(SaveSuccess {
            fingerprint: Fingerprint::of(&ciphertext),
            generation: self.generation,
            revision: self.revision,
            wrote_lkg,
        })
    }

    /// Writes the job's payload as an encrypted *unsaved-changes* recovery
    /// copy instead of the vault file (used when locking after a failed save).
    pub fn run_as_recovery_copy(
        self,
        recovery_root: &Path,
        vault_id: VaultId,
    ) -> VaultResult<PathBuf> {
        let ciphertext = crypto::encrypt(&self.passphrase, self.work, &self.plaintext)?;
        RecoveryStore::for_vault(recovery_root, vault_id).store(RecoveryKind::Unsaved, &ciphertext)
    }
}

impl Session {
    /// Creates a new vault file at `path` (which must not exist) and opens it.
    pub fn create(
        path: &Path,
        passphrase: Passphrase,
        vault: Vault,
        opts: SessionOptions,
    ) -> VaultResult<Self> {
        if passphrase.is_empty() {
            return Err(VaultError::new(ErrorCode::PassphraseEmpty));
        }
        let mut vault = vault;
        let now = now_unix();
        vault.saved_at = now;
        vault.save_counter = 1;
        history::record_save(&mut vault, now, SaveTrigger::Explicit)?;
        let plaintext = format::encode(&vault)?;
        let ciphertext = crypto::encrypt(&passphrase, opts.work, &plaintext)?;
        drop(plaintext);
        storage::write_new(path, &ciphertext, opts.faults)?;
        let fp = Fingerprint::of(&ciphertext);
        let lock = if opts.take_lock {
            Some(SessionLock::acquire(&opts.data.locks_dir(), path)?)
        } else {
            None
        };
        Ok(Self::from_parts(vault, passphrase, path, fp, opts, lock))
    }

    /// Opens an existing vault file.
    pub fn open(path: &Path, passphrase: Passphrase, opts: SessionOptions) -> VaultResult<Self> {
        let lock = if opts.take_lock {
            Some(SessionLock::acquire(&opts.data.locks_dir(), path)?)
        } else {
            None
        };
        let (vault, fp) = open_vault_file(path, &passphrase)?;
        storage::clean_stale_temps(path);
        Ok(Self::from_parts(vault, passphrase, path, fp, opts, lock))
    }

    fn from_parts(
        vault: Vault,
        passphrase: Passphrase,
        path: &Path,
        fingerprint: Fingerprint,
        opts: SessionOptions,
        lock: Option<SessionLock>,
    ) -> Self {
        Self {
            committed_revision: vault.revision_id,
            committed_parent: vault.parent_revision_id,
            committed_counter: vault.save_counter,
            committed_saved_at: vault.saved_at,
            vault,
            passphrase: Arc::new(passphrase),
            path: path.to_path_buf(),
            fingerprint,
            edit_generation: 0,
            saved_generation: 0,
            last_lkg_at: None,
            opts,
            lock,
            save_in_flight: false,
        }
    }

    /// The decrypted vault.
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// Mutable access. Call [`Self::mark_edited`] after changing content.
    pub fn vault_mut(&mut self) -> &mut Vault {
        &mut self.vault
    }

    /// Path of the vault file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Fingerprint of the ciphertext this session last read or wrote.
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Session options.
    pub fn options(&self) -> &SessionOptions {
        &self.opts
    }

    /// Records that in-memory content changed.
    pub fn mark_edited(&mut self) {
        self.edit_generation = self.edit_generation.wrapping_add(1);
    }

    /// Whether there are changes not yet written to disk.
    pub fn is_dirty(&self) -> bool {
        self.edit_generation != self.saved_generation
    }

    /// Whether a save job is outstanding.
    pub fn save_in_flight(&self) -> bool {
        self.save_in_flight
    }

    /// Recovery store for this vault.
    pub fn recovery_store(&self) -> RecoveryStore {
        RecoveryStore::for_vault(&self.opts.data.recovery_dir(), self.vault.vault_id)
    }

    /// Records history, assigns a new revision and encodes the payload.
    ///
    /// On error the vault's revision fields are restored and nothing is
    /// written; edits stay in memory.
    pub fn prepare_save(&mut self, trigger: SaveTrigger) -> VaultResult<SaveJob> {
        if self.save_in_flight {
            return Err(VaultError::new(ErrorCode::Busy));
        }
        let now = now_unix();
        history::record_save(&mut self.vault, now, trigger)?;
        self.vault.parent_revision_id = Some(self.committed_revision);
        self.vault.revision_id = RevisionId::random();
        self.vault.save_counter = self.committed_counter.saturating_add(1);
        self.vault.saved_at = now;
        let plaintext = match format::encode(&self.vault) {
            Ok(p) => p,
            Err(e) => {
                self.revert_revision();
                return Err(e);
            }
        };
        let lkg_due = self
            .last_lkg_at
            .is_none_or(|t| now.saturating_sub(t) >= LKG_INTERVAL_SECS);
        self.save_in_flight = true;
        Ok(SaveJob {
            plaintext,
            passphrase: Arc::clone(&self.passphrase),
            work: self.opts.work,
            path: self.path.clone(),
            expected: self.fingerprint,
            lkg: lkg_due.then(|| self.recovery_store()),
            faults: self.opts.faults,
            generation: self.edit_generation,
            revision: self.vault.revision_id,
        })
    }

    fn revert_revision(&mut self) {
        self.vault.revision_id = self.committed_revision;
        self.vault.parent_revision_id = self.committed_parent;
        self.vault.save_counter = self.committed_counter;
        self.vault.saved_at = self.committed_saved_at;
    }

    /// Applies the result of a [`SaveJob`].
    pub fn finish_save(&mut self, result: SaveResult) -> VaultResult<()> {
        self.save_in_flight = false;
        match result {
            Ok(s) => {
                self.fingerprint = s.fingerprint;
                self.saved_generation = s.generation;
                self.committed_parent = self.vault.parent_revision_id;
                self.committed_revision = s.revision;
                self.committed_counter = self.vault.save_counter;
                self.committed_saved_at = self.vault.saved_at;
                if s.wrote_lkg {
                    self.last_lkg_at = Some(now_unix());
                }
                Ok(())
            }
            Err((e, _)) => {
                self.revert_revision();
                Err(e)
            }
        }
    }

    /// Saves synchronously.
    pub fn save(&mut self, trigger: SaveTrigger) -> VaultResult<()> {
        let job = self.prepare_save(trigger)?;
        let result = job.run();
        self.finish_save(result)
    }

    /// Saves the current state as a *new* conflict-copy file next to the
    /// vault and switches the session to it. The original file is untouched.
    pub fn save_conflict_copy(&mut self) -> VaultResult<PathBuf> {
        if self.save_in_flight {
            return Err(VaultError::new(ErrorCode::Busy));
        }
        let dest = conflict_copy_path(&self.path);
        let now = now_unix();
        history::record_save(&mut self.vault, now, SaveTrigger::Explicit)?;
        self.vault.parent_revision_id = Some(self.committed_revision);
        self.vault.revision_id = RevisionId::random();
        self.vault.save_counter = self.committed_counter.saturating_add(1);
        self.vault.saved_at = now;
        let result = (|| {
            let plaintext = format::encode(&self.vault)?;
            let ciphertext = crypto::encrypt(&self.passphrase, self.opts.work, &plaintext)?;
            drop(plaintext);
            storage::write_new(&dest, &ciphertext, self.opts.faults)?;
            Ok(Fingerprint::of(&ciphertext))
        })();
        match result {
            Ok(fp) => {
                let lock = if self.opts.take_lock {
                    Some(SessionLock::acquire(&self.opts.data.locks_dir(), &dest)?)
                } else {
                    None
                };
                self.lock = lock;
                self.path = dest.clone();
                self.fingerprint = fp;
                self.saved_generation = self.edit_generation;
                self.committed_parent = self.vault.parent_revision_id;
                self.committed_revision = self.vault.revision_id;
                self.committed_counter = self.vault.save_counter;
                self.committed_saved_at = self.vault.saved_at;
                Ok(dest)
            }
            Err(e) => {
                self.revert_revision();
                Err(e)
            }
        }
    }

    /// Re-encrypts the vault with a new passphrase.
    ///
    /// The new file replaces the old one atomically; if anything fails the
    /// previous file still opens with the previous passphrase and the session
    /// keeps using it. Recovery copies and backups made earlier remain
    /// encrypted with the passphrase that was current when they were written.
    pub fn change_passphrase(&mut self, current: &Passphrase, new: Passphrase) -> VaultResult<()> {
        if !current.matches(&self.passphrase) {
            return Err(VaultError::new(ErrorCode::WrongPassphraseOrNotAVault));
        }
        if new.is_empty() {
            return Err(VaultError::new(ErrorCode::PassphraseEmpty));
        }
        if self.save_in_flight {
            return Err(VaultError::new(ErrorCode::Busy));
        }
        let new = Arc::new(new);
        let old = std::mem::replace(&mut self.passphrase, Arc::clone(&new));
        match self.save(SaveTrigger::Explicit) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.passphrase = old;
                Err(e)
            }
        }
    }

    /// Writes an encrypted full-vault backup to `dest` (replacing a regular
    /// file there, never following a symlink). Uses `passphrase` if given,
    /// otherwise the session passphrase.
    pub fn backup_to(&self, dest: &Path, passphrase: Option<&Passphrase>) -> VaultResult<()> {
        if dest == self.path {
            return Err(VaultError::new(ErrorCode::AlreadyExists));
        }
        let plaintext = format::encode(&self.vault)?;
        let pass = passphrase.unwrap_or(&self.passphrase);
        if pass.is_empty() {
            return Err(VaultError::new(ErrorCode::PassphraseEmpty));
        }
        let ciphertext = crypto::encrypt(pass, self.opts.work, &plaintext)?;
        drop(plaintext);
        storage::write_atomic(dest, &ciphertext, None, self.opts.faults)?;
        Ok(())
    }

    /// Checks whether the on-disk file still matches what this session last
    /// read or wrote.
    pub fn disk_unchanged(&self) -> VaultResult<bool> {
        match storage::read_bounded(&self.path, limits::MAX_ENCRYPTED_BYTES) {
            Ok(bytes) => Ok(Fingerprint::of(&bytes) == self.fingerprint),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Prepares a job that writes current state as an unsaved-changes
    /// recovery copy (used by the lock policy when the main save fails).
    pub fn prepare_recovery_copy(&mut self) -> VaultResult<SaveJob> {
        let plaintext = format::encode(&self.vault)?;
        Ok(SaveJob {
            plaintext,
            passphrase: Arc::clone(&self.passphrase),
            work: self.opts.work,
            path: self.path.clone(),
            expected: self.fingerprint,
            lkg: None,
            faults: self.opts.faults,
            generation: self.edit_generation,
            revision: self.vault.revision_id,
        })
    }

    /// Discards in-memory state and re-reads the vault file from disk with
    /// the session passphrase (used to resolve a conflict in favor of the
    /// on-disk version). Fails without changing anything if the file cannot
    /// be opened.
    pub fn reload_from_disk(&mut self) -> VaultResult<()> {
        if self.save_in_flight {
            return Err(VaultError::new(ErrorCode::Busy));
        }
        let (vault, fp) = open_vault_file(&self.path, &self.passphrase)?;
        self.committed_revision = vault.revision_id;
        self.committed_parent = vault.parent_revision_id;
        self.committed_counter = vault.save_counter;
        self.committed_saved_at = vault.saved_at;
        self.vault = vault;
        self.fingerprint = fp;
        self.saved_generation = self.edit_generation;
        Ok(())
    }

    /// Verifies a passphrase against the session without touching disk.
    pub fn passphrase_matches(&self, candidate: &Passphrase) -> bool {
        candidate.matches(&self.passphrase)
    }
}

/// Restores a backup (or any vault file) as a brand-new vault at `new_path`.
/// The restored vault gets a fresh vault id and revision lineage so it never
/// conflicts with the original. `new_path` must not exist.
pub fn restore_as_new(
    backup: &Path,
    backup_passphrase: &Passphrase,
    new_path: &Path,
    new_passphrase: Passphrase,
    opts: SessionOptions,
) -> VaultResult<Session> {
    let (mut vault, _) = open_vault_file(backup, backup_passphrase)?;
    vault.vault_id = VaultId::random();
    vault.revision_id = RevisionId::random();
    vault.parent_revision_id = None;
    vault.save_counter = 0;
    Session::create(new_path, new_passphrase, vault, opts)
}

/// `<dir>/<stem> (conflict YYYYMMDD-HHMMSS).nvault.age`
pub fn conflict_copy_path(path: &Path) -> PathBuf {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| crate::app::DEFAULT_VAULT_FILENAME.to_owned());
    let ext = format!(".{}", crate::app::VAULT_EXTENSION);
    let stem = name.strip_suffix(&ext).unwrap_or(&name);
    let ts = format_compact(now_unix());
    let mut candidate = dir.join(format!("{stem} (conflict {ts}){ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} (conflict {ts} {n}){ext}"));
        n += 1;
    }
    candidate
}
