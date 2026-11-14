//! Closed, typed diagnostics.
//!
//! [`Event`] is an allowlist: variants carry only enums, error codes and
//! small integers — never strings, paths, documents or byte buffers — so
//! nothing sensitive can be recorded even by mistake. Events live in a
//! bounded in-memory ring. A [`DiagnosticReport`] can be written locally,
//! only when the user explicitly asks; nothing is ever uploaded.
//!
//! Development builds obey the same policy; there is no debug flag that
//! records secrets.

use std::collections::VecDeque;

use serde::Serialize;

use crate::app::{APP_NAME, APP_VERSION, RELEASE_LABEL};
use crate::error::ErrorCode;
use crate::time::now_unix;

/// Maximum events kept in memory.
pub const MAX_EVENTS: usize = 256;

/// Why a vault was locked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockReason {
    /// The user locked it.
    Manual,
    /// Idle timeout.
    Idle,
    /// System sleep / screen lock.
    SystemSleep,
    /// The window was closed.
    WindowClosed,
    /// The window lost focus (optional setting).
    FocusLost,
}

/// Allowlisted diagnostic events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "event", content = "detail", rename_all = "snake_case")]
pub enum Event {
    /// App started.
    AppStarted,
    /// A vault was created.
    VaultCreated,
    /// Unlock succeeded.
    UnlockSucceeded,
    /// Unlock failed.
    UnlockFailed(ErrorCode),
    /// A save completed.
    SaveSucceeded,
    /// A save failed.
    SaveFailed(ErrorCode),
    /// An on-disk conflict was detected.
    ConflictDetected,
    /// A conflict copy was written.
    ConflictCopyWritten,
    /// The vault was locked.
    Locked(LockReason),
    /// Locking discarded unsaved changes after a failed save.
    LockDiscardedChanges,
    /// An unsaved-changes recovery copy was written during lock.
    RecoveryCopyWritten,
    /// Passphrase changed.
    PassphraseChanged,
    /// Passphrase change failed.
    PassphraseChangeFailed(ErrorCode),
    /// Backup written.
    BackupWritten,
    /// Import completed.
    Imported,
    /// Import failed.
    ImportFailed(ErrorCode),
    /// Export completed (`true` = plaintext).
    Exported {
        /// Whether the export was plaintext.
        plaintext: bool,
    },
    /// Export failed.
    ExportFailed(ErrorCode),
    /// Clipboard content placed by the app was cleared.
    ClipboardCleared,
    /// A panic was caught (message suppressed).
    PanicSuppressed,
    /// Any other failure, by code.
    Error(ErrorCode),
}

/// A timestamped event.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Record {
    /// Unix seconds.
    pub at: i64,
    /// The event.
    #[serde(flatten)]
    pub event: Event,
}

/// In-memory diagnostic ring buffer.
#[derive(Debug, Default)]
pub struct Diagnostics {
    records: VecDeque<Record>,
}

impl Diagnostics {
    /// Records an event. Accepts only [`Event`], so arbitrary data cannot be
    /// logged through this API.
    pub fn record(&mut self, event: Event) {
        if self.records.len() >= MAX_EVENTS {
            self.records.pop_front();
        }
        self.records.push_back(Record {
            at: now_unix(),
            event,
        });
    }

    /// Recorded events, oldest first.
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.records.iter()
    }

    /// Number of recorded events.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Capability flags describing what this build/platform actually provides.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Capabilities {
    /// The application implements no network features.
    pub no_app_networking: bool,
    /// Running inside an OS sandbox that denies network access (detected).
    pub os_network_sandbox: bool,
    /// Core dumps disabled for this process.
    pub core_dumps_disabled: bool,
    /// System sleep / screen-lock notifications wired up.
    pub sleep_lock_signals: bool,
    /// Clipboard privacy hints (e.g. exclude from history) applied.
    pub clipboard_privacy_hints: bool,
}

/// A local diagnostic report. Contains only versions, capability flags and
/// allowlisted events with error codes.
#[derive(Debug, Serialize)]
pub struct DiagnosticReport {
    /// Report schema version.
    pub schema: u32,
    /// Application name.
    pub app: &'static str,
    /// Application version.
    pub version: &'static str,
    /// Release label (e.g. "unaudited alpha").
    pub release: &'static str,
    /// OS family (e.g. "macos").
    pub os: &'static str,
    /// CPU architecture.
    pub arch: &'static str,
    /// Debug or release build.
    pub build: &'static str,
    /// Capability flags.
    pub capabilities: Capabilities,
    /// Recent events.
    pub events: Vec<Record>,
}

impl DiagnosticReport {
    /// Builds a report from the in-memory log.
    pub fn build(diag: &Diagnostics, capabilities: Capabilities) -> Self {
        Self {
            schema: 1,
            app: APP_NAME,
            version: APP_VERSION,
            release: RELEASE_LABEL,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            build: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            capabilities,
            events: diag.records().copied().collect(),
        }
    }

    /// Pretty JSON for review before saving.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

impl serde::Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_bounded() {
        let mut d = Diagnostics::default();
        for _ in 0..(MAX_EVENTS + 10) {
            d.record(Event::SaveSucceeded);
        }
        assert_eq!(d.len(), MAX_EVENTS);
    }

    #[test]
    fn report_contains_only_codes() {
        let mut d = Diagnostics::default();
        d.record(Event::UnlockFailed(ErrorCode::WrongPassphraseOrNotAVault));
        d.record(Event::Locked(LockReason::Idle));
        d.record(Event::Exported { plaintext: true });
        let caps = Capabilities {
            no_app_networking: true,
            os_network_sandbox: false,
            core_dumps_disabled: true,
            sleep_lock_signals: true,
            clipboard_privacy_hints: false,
        };
        let json = DiagnosticReport::build(&d, caps).to_json();
        assert!(json.contains("NV-101"));
        assert!(json.contains("\"idle\""));
        assert!(!json.contains("passphrase is incorrect"));
    }
}
