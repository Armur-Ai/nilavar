//! Debounced, coalesced autosave scheduling.
//!
//! A save is due once edits have been quiet for [`AutosavePolicy::quiet`],
//! or after [`AutosavePolicy::max_delay`] of continuous editing. Only one save
//! is ever in flight; edits during a save simply make the next save due.
//! After a failure, retries back off. No snapshot queue exists, so at most
//! one encrypted-save payload is outstanding at any time.

use std::time::{Duration, Instant};

/// Timing policy.
#[derive(Clone, Copy, Debug)]
pub struct AutosavePolicy {
    /// Quiet period after the last edit.
    pub quiet: Duration,
    /// Maximum time a change may stay unsaved while editing continues.
    pub max_delay: Duration,
    /// Wait after a failed save before retrying automatically.
    pub retry_after: Duration,
}

impl Default for AutosavePolicy {
    fn default() -> Self {
        Self {
            quiet: Duration::from_secs(3),
            max_delay: Duration::from_secs(45),
            retry_after: Duration::from_secs(20),
        }
    }
}

/// Autosave scheduler state.
#[derive(Clone, Debug, Default)]
pub struct Autosave {
    policy: AutosavePolicy,
    first_unsaved_edit: Option<Instant>,
    last_edit: Option<Instant>,
    last_failure: Option<Instant>,
}

impl Autosave {
    /// Creates a scheduler.
    pub fn new(policy: AutosavePolicy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
    }

    /// Records an edit.
    pub fn on_edit(&mut self, now: Instant) {
        self.first_unsaved_edit.get_or_insert(now);
        self.last_edit = Some(now);
    }

    /// Records that a save was dispatched covering all edits so far.
    pub fn on_dispatched(&mut self) {
        self.first_unsaved_edit = None;
        self.last_edit = None;
    }

    /// Records a failed save; the edits are unsaved again.
    pub fn on_failed(&mut self, now: Instant, edited_at: Instant) {
        self.last_failure = Some(now);
        self.first_unsaved_edit.get_or_insert(edited_at);
        self.last_edit.get_or_insert(edited_at);
    }

    /// Records a successful save.
    pub fn on_succeeded(&mut self) {
        self.last_failure = None;
    }

    /// Whether a save should be dispatched now.
    pub fn due(&self, now: Instant, in_flight: bool, enabled: bool) -> bool {
        if !enabled || in_flight {
            return false;
        }
        let (Some(first), Some(last)) = (self.first_unsaved_edit, self.last_edit) else {
            return false;
        };
        if let Some(f) = self.last_failure
            && now.saturating_duration_since(f) < self.policy.retry_after
        {
            return false;
        }
        now.saturating_duration_since(last) >= self.policy.quiet
            || now.saturating_duration_since(first) >= self.policy.max_delay
    }

    /// Time until the next check is useful (for scheduling a repaint).
    pub fn next_check_in(&self, now: Instant) -> Option<Duration> {
        let last = self.last_edit?;
        let quiet_left = self
            .policy
            .quiet
            .saturating_sub(now.saturating_duration_since(last));
        Some(quiet_left.max(Duration::from_millis(200)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounces_and_caps_delay() {
        let p = AutosavePolicy::default();
        let mut a = Autosave::new(p);
        let t0 = Instant::now();
        assert!(!a.due(t0, false, true));
        a.on_edit(t0);
        assert!(!a.due(t0 + Duration::from_secs(1), false, true));
        assert!(a.due(t0 + p.quiet, false, true));
        assert!(
            !a.due(t0 + p.quiet, true, true),
            "never two saves in flight"
        );
        assert!(!a.due(t0 + p.quiet, false, false), "disabled");
        // Continuous typing: due at max_delay even without a quiet gap.
        let mut b = Autosave::new(p);
        let mut t = t0;
        while t < t0 + p.max_delay {
            b.on_edit(t);
            t += Duration::from_secs(1);
        }
        assert!(b.due(t0 + p.max_delay, false, true));
        b.on_dispatched();
        assert!(!b.due(t0 + p.max_delay, false, true));
    }

    #[test]
    fn failures_back_off() {
        let p = AutosavePolicy::default();
        let mut a = Autosave::new(p);
        let t0 = Instant::now();
        a.on_edit(t0);
        a.on_dispatched();
        let fail_at = t0 + Duration::from_secs(5);
        a.on_failed(fail_at, t0);
        assert!(!a.due(fail_at + Duration::from_secs(1), false, true));
        assert!(a.due(fail_at + p.retry_after, false, true));
    }
}
