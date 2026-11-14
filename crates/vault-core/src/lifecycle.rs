//! Lock-state machine and lock policy.
//!
//! ```text
//!   Locked ──BeginUnlock──▶ Unlocking ──UnlockSucceeded──▶ Unlocked ◀─┐
//!     ▲                        │ UnlockFailed                │        │ SaveFinished
//!     │                        ▼                             │BeginSave
//!     │                      Locked                          ▼        │
//!     │                                                    Saving ────┘
//!     │                                                      │
//!     └──LockFinished{idle}── Locking ◀──BeginLock── Unlocked | Saving
//! ```
//!
//! The workspace is masked as soon as `Locking` begins. The machine refuses
//! to report `Locked` while secret-owning workers are still active.

use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::history::SaveTrigger;
use crate::paths::DataDirs;
use crate::session::Session;

/// Application lock states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockState {
    /// No vault plaintext is held.
    Locked,
    /// Decrypting on a worker.
    Unlocking,
    /// A session is open.
    Unlocked,
    /// A session is open and a save job is running.
    Saving,
    /// Workspace masked; tearing down the session and workers.
    Locking,
}

/// Inputs to the state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockEvent {
    /// Start decrypting.
    BeginUnlock,
    /// Decryption and validation succeeded.
    UnlockSucceeded,
    /// Decryption or validation failed.
    UnlockFailed,
    /// A save job was dispatched.
    BeginSave,
    /// The save job finished (success or failure).
    SaveFinished,
    /// Locking started (manual, idle, sleep or close).
    BeginLock,
    /// Teardown finished; `workers_idle` reports whether any worker still
    /// owns secret data.
    LockFinished {
        /// No secret-owning work remains.
        workers_idle: bool,
    },
}

impl LockState {
    /// Applies an event, returning the next state or an error for an
    /// undefined transition.
    pub fn apply(self, ev: LockEvent) -> VaultResult<LockState> {
        use LockEvent as E;
        use LockState as S;
        let next = match (self, ev) {
            (S::Locked, E::BeginUnlock) => S::Unlocking,
            (S::Unlocking, E::UnlockSucceeded) => S::Unlocked,
            (S::Unlocking, E::UnlockFailed) => S::Locked,
            (S::Unlocked, E::BeginSave) => S::Saving,
            (S::Saving, E::SaveFinished) => S::Unlocked,
            (S::Unlocked | S::Saving, E::BeginLock) => S::Locking,
            (S::Locking, E::SaveFinished) => S::Locking,
            (S::Locking, E::LockFinished { workers_idle: true }) => S::Locked,
            (S::Locking, E::LockFinished { workers_idle: false }) => S::Locking,
            _ => return Err(VaultError::new(ErrorCode::Internal)),
        };
        Ok(next)
    }

    /// Whether vault content may be shown.
    pub fn shows_workspace(self) -> bool {
        matches!(self, LockState::Unlocked | LockState::Saving)
    }
}

/// What happened when a vault was locked automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoLockOutcome {
    /// Nothing was unsaved.
    Clean,
    /// Unsaved changes were saved to the vault file.
    Saved,
    /// Saving failed; changes were written as an encrypted unsaved-changes
    /// recovery copy in the app data directory.
    RecoveryCopy(ErrorCode),
    /// Saving and the recovery copy both failed; unsaved changes were
    /// discarded. Only this non-sensitive flag is recorded.
    Discarded(ErrorCode),
}

impl AutoLockOutcome {
    /// Whether the user must be told about lost or diverted changes on next unlock.
    pub fn needs_notice(self) -> bool {
        matches!(self, AutoLockOutcome::RecoveryCopy(_) | AutoLockOutcome::Discarded(_))
    }
}

/// Automatic-lock policy: one bounded encrypted save attempt, then one
/// encrypted recovery-copy attempt, then release the session regardless.
/// Never writes plaintext. Consumes and drops the session.
pub fn auto_lock(mut session: Session, data: &DataDirs) -> AutoLockOutcome {
    if !session.is_dirty() {
        return AutoLockOutcome::Clean;
    }
    let err = match session.save(SaveTrigger::Auto) {
        Ok(()) => return AutoLockOutcome::Saved,
        Err(e) => e.code(),
    };
    let vault_id = session.vault().vault_id;
    let copy = session
        .prepare_recovery_copy()
        .and_then(|job| job.run_as_recovery_copy(&data.recovery_dir(), vault_id));
    drop(session);
    match copy {
        Ok(_) => AutoLockOutcome::RecoveryCopy(err),
        Err(_) => AutoLockOutcome::Discarded(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LockEvent as E;
    use LockState as S;

    #[test]
    fn happy_path() {
        let s = S::Locked.apply(E::BeginUnlock).unwrap();
        let s = s.apply(E::UnlockSucceeded).unwrap();
        let s = s.apply(E::BeginSave).unwrap();
        assert!(s.shows_workspace());
        let s = s.apply(E::BeginLock).unwrap();
        assert!(!s.shows_workspace());
        let s = s.apply(E::SaveFinished).unwrap();
        assert_eq!(s, S::Locking);
        let s = s.apply(E::LockFinished { workers_idle: false }).unwrap();
        assert_eq!(s, S::Locking, "never report locked while workers hold secrets");
        assert_eq!(s.apply(E::LockFinished { workers_idle: true }).unwrap(), S::Locked);
    }

    #[test]
    fn invalid_transitions_rejected() {
        assert!(S::Locked.apply(E::BeginSave).is_err());
        assert!(S::Locked.apply(E::UnlockSucceeded).is_err());
        assert!(S::Unlocking.apply(E::BeginLock).is_err());
        assert!(S::Locking.apply(E::BeginUnlock).is_err());
        assert!(S::Unlocked.apply(E::LockFinished { workers_idle: true }).is_err());
    }
}
