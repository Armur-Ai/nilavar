//! Clipboard clearing policy.
//!
//! Copying from the vault is deliberate and best effort. After the
//! configured delay (and on lock) the app clears the clipboard **only if it
//! still holds exactly the value the app placed there**; newer, unrelated
//! clipboard content is never erased. The app remembers only a keyed hash of
//! what it copied (with a per-process random key), not the value itself.
//!
//! Other programs and clipboard-history managers may already have read the
//! value; clearing cannot undo that.

use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::ids::random_bytes;

/// Platform clipboard access, abstracted for testing.
pub trait ClipboardBackend {
    /// Reads the current text content, if any.
    fn read_text(&mut self) -> Option<Zeroizing<String>>;
    /// Replaces the clipboard with `text`. Returns success.
    fn write_text(&mut self, text: &str) -> bool;
    /// Clears the clipboard. Returns success.
    fn clear(&mut self) -> bool;
}

/// Result of a clearing check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearOutcome {
    /// Nothing pending, or not yet due.
    Nothing,
    /// The app's value was still present and was cleared.
    Cleared,
    /// The clipboard changed since the app copied; left alone.
    LeftAlone,
}

struct Pending {
    digest: [u8; 32],
    deadline: Instant,
}

/// Tracks the one value the app most recently copied.
pub struct ClipboardGuard {
    key: [u8; 32],
    pending: Option<Pending>,
}

impl Default for ClipboardGuard {
    fn default() -> Self {
        Self {
            key: random_bytes(),
            pending: None,
        }
    }
}

impl std::fmt::Debug for ClipboardGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClipboardGuard(pending: {})", self.pending.is_some())
    }
}

impl ClipboardGuard {
    fn digest(&self, text: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.key);
        h.update(text.as_bytes());
        h.finalize().into()
    }

    /// Copies `text` and schedules clearing after `clear_after`.
    pub fn copy(&mut self, backend: &mut dyn ClipboardBackend, text: &str, now: Instant, clear_after: Duration) -> bool {
        if !backend.write_text(text) {
            return false;
        }
        self.pending = Some(Pending {
            digest: self.digest(text),
            deadline: now + clear_after,
        });
        true
    }

    /// Whether a clear is pending.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Time until the pending clear is due.
    pub fn due_in(&self, now: Instant) -> Option<Duration> {
        self.pending.as_ref().map(|p| p.deadline.saturating_duration_since(now))
    }

    /// Clears the app's value if the deadline passed.
    pub fn tick(&mut self, backend: &mut dyn ClipboardBackend, now: Instant) -> ClearOutcome {
        match &self.pending {
            Some(p) if now >= p.deadline => self.clear_now(backend),
            _ => ClearOutcome::Nothing,
        }
    }

    /// Clears the app's value immediately (e.g. on lock) if still present.
    pub fn clear_now(&mut self, backend: &mut dyn ClipboardBackend) -> ClearOutcome {
        let Some(p) = self.pending.take() else {
            return ClearOutcome::Nothing;
        };
        match backend.read_text() {
            Some(current) if self.digest(&current) == p.digest => {
                if backend.clear() {
                    ClearOutcome::Cleared
                } else {
                    ClearOutcome::LeftAlone
                }
            }
            _ => ClearOutcome::LeftAlone,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        value: Option<String>,
    }
    impl ClipboardBackend for Fake {
        fn read_text(&mut self) -> Option<Zeroizing<String>> {
            self.value.clone().map(Zeroizing::new)
        }
        fn write_text(&mut self, text: &str) -> bool {
            self.value = Some(text.to_owned());
            true
        }
        fn clear(&mut self) -> bool {
            self.value = None;
            true
        }
    }

    #[test]
    fn clears_only_unchanged_value_after_timeout() {
        let mut cb = Fake::default();
        let mut g = ClipboardGuard::default();
        let t0 = Instant::now();
        g.copy(&mut cb, "SYNTH-secret", t0, Duration::from_secs(30));
        assert_eq!(g.tick(&mut cb, t0 + Duration::from_secs(29)), ClearOutcome::Nothing);
        assert_eq!(g.tick(&mut cb, t0 + Duration::from_secs(30)), ClearOutcome::Cleared);
        assert_eq!(cb.value, None);
    }

    #[test]
    fn never_erases_newer_unrelated_content() {
        let mut cb = Fake::default();
        let mut g = ClipboardGuard::default();
        let t0 = Instant::now();
        g.copy(&mut cb, "SYNTH-secret", t0, Duration::from_secs(5));
        cb.value = Some("user copied something else".into());
        assert_eq!(g.tick(&mut cb, t0 + Duration::from_secs(10)), ClearOutcome::LeftAlone);
        assert_eq!(cb.value.as_deref(), Some("user copied something else"));
        assert!(!g.has_pending());
    }

    #[test]
    fn lock_clears_immediately() {
        let mut cb = Fake::default();
        let mut g = ClipboardGuard::default();
        g.copy(&mut cb, "SYNTH", Instant::now(), Duration::from_secs(600));
        assert_eq!(g.clear_now(&mut cb), ClearOutcome::Cleared);
        assert_eq!(g.clear_now(&mut cb), ClearOutcome::Nothing);
    }
}
