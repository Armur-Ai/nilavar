//! Platform adapters: process hardening, panic handling, clipboard and
//! sleep detection.
//!
//! The only `unsafe` in first-party code lives in [`disable_core_dumps`]
//! (two documented libc calls). Everything else is safe Rust.

use std::time::{Duration, Instant, SystemTime};

use vault_core::clipboard::ClipboardBackend;
use zeroize::Zeroizing;

/// Disables core dumps for this process (and, on Linux, marks it
/// non-dumpable, which also blocks same-user ptrace attach). Returns whether
/// it succeeded. OS-level capture outside the process's control (crash
/// reporters configured by an administrator, hibernation images, swap) is
/// not affected; see THREAT_MODEL.md.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn disable_core_dumps() -> bool {
    let lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `setrlimit` reads a fully-initialized `rlimit` struct that
    // lives on our stack for the duration of the call; it has no other
    // memory-safety preconditions.
    let ok = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &lim) } == 0;
    #[cfg(target_os = "linux")]
    let ok = {
        // SAFETY: `prctl(PR_SET_DUMPABLE, 0)` takes integer arguments only
        // and does not access memory through them.
        let r = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
        ok && r == 0
    };
    ok
}

#[cfg(not(unix))]
pub fn disable_core_dumps() -> bool {
    // Windows Error Reporting dumps are configured system-wide; documented.
    false
}

/// Replaces the default panic hook. Panic payloads (which may quote data,
/// e.g. string-slicing panics include the string) are never printed; only a
/// fixed message and the source location of the panic are shown.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        #[allow(clippy::print_stderr)]
        {
            eprintln!(
                "nilavar: internal error at {loc} (details suppressed to protect vault content)"
            );
        }
    }));
}

/// System clipboard via arboard with privacy hints where supported
/// (macOS: concealed type, excluded from clipboard history; Windows:
/// excluded from history and cloud clipboard; Linux: password-manager hint
/// honored by some clipboard managers).
pub struct SystemClipboard {
    inner: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            inner: arboard::Clipboard::new().ok(),
        }
    }

    pub fn privacy_hints_supported() -> bool {
        cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        ))
    }
}

impl ClipboardBackend for SystemClipboard {
    fn read_text(&mut self) -> Option<Zeroizing<String>> {
        self.inner.as_mut()?.get_text().ok().map(Zeroizing::new)
    }

    fn write_text(&mut self, text: &str) -> bool {
        let Some(cb) = self.inner.as_mut() else {
            return false;
        };
        let set = cb.set();
        #[cfg(target_os = "macos")]
        let set = {
            use arboard::SetExtApple;
            set.exclude_from_history()
        };
        #[cfg(target_os = "windows")]
        let set = {
            use arboard::SetExtWindows;
            set.exclude_from_history().exclude_from_cloud()
        };
        #[cfg(target_os = "linux")]
        let set = {
            use arboard::SetExtLinux;
            set.exclude_from_history()
        };
        set.text(text.to_owned()).is_ok()
    }

    fn clear(&mut self) -> bool {
        self.inner
            .as_mut()
            .map(|c| c.clear().is_ok())
            .unwrap_or(false)
    }
}

/// Detects system sleep by comparing wall-clock and monotonic progress.
/// On macOS and Linux the monotonic clock used by `Instant` stops during
/// sleep while wall time advances. Detection happens on wake, so locking is
/// "lock on resume", not "lock before sleep" (documented limitation).
pub struct SleepDetector {
    wall: SystemTime,
    mono: Instant,
}

impl SleepDetector {
    pub const THRESHOLD: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self {
            wall: SystemTime::now(),
            mono: Instant::now(),
        }
    }

    /// Returns true if the system appears to have slept since the last call.
    pub fn check(&mut self) -> bool {
        let wall_now = SystemTime::now();
        let mono_now = Instant::now();
        let wall_dt = wall_now.duration_since(self.wall).unwrap_or_default();
        let mono_dt = mono_now.saturating_duration_since(self.mono);
        self.wall = wall_now;
        self.mono = mono_now;
        wall_dt > mono_dt + Self::THRESHOLD
    }
}

/// Whether this process runs inside the macOS App Sandbox.
pub fn in_macos_sandbox() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}
