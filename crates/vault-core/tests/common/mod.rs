//! Shared helpers for integration tests. All data is synthetic.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use vault_core::crypto::WorkFactor;
use vault_core::paths::DataDirs;
use vault_core::session::SessionOptions;

/// Distinctive fake canaries used by leakage tests.
pub const CANARY_PASS: &str = "NVCANARY-pass-Q7xZ9-passphrase";
pub const CANARY_TEXT: &str = "NVCANARY-text-K3mW1-document-body";
pub const CANARY_TITLE: &str = "NVCANARY-title-P5rT8";
pub const CANARY_TAG: &str = "nvcanary-tag-J2vB6";
pub const CANARY_FOLDER: &str = "NVCANARY-folder-H4nL0";

pub fn test_opts(data_root: &Path) -> SessionOptions {
    let mut o = SessionOptions::new(DataDirs::at(data_root));
    o.work = WorkFactor::TESTING;
    o
}

/// Finds a reference tool binary (age or rage).
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".cargo/bin").join(name));
    }
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        candidates.push(PathBuf::from(dir).join(name));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&path) {
            candidates.push(p.join(name));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Whether missing reference tools should fail the test run (set in CI).
pub fn interop_required() -> bool {
    std::env::var_os("NILAVAR_REQUIRE_INTEROP").is_some()
}

/// Runs a reference tool under a pseudo-terminal (via `script`) and feeds
/// `answers` (one passphrase per prompt). Returns success.
pub fn run_with_pty(tool: &Path, args: &[&str], answers: &[&str]) -> bool {
    let mut cmd = Command::new("script");
    if cfg!(target_os = "macos") {
        cmd.arg("-q").arg("/dev/null").arg(tool).args(args);
    } else {
        let mut line = shell_quote(&tool.to_string_lossy());
        for a in args {
            line.push(' ');
            line.push_str(&shell_quote(a));
        }
        cmd.arg("-q").arg("-e").arg("-c").arg(line).arg("/dev/null");
    }
    // Keep pinentry off PATH so rage prompts on the terminal.
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let mut stdin = child.stdin.take().expect("stdin");
    for a in answers {
        std::thread::sleep(Duration::from_millis(600));
        let _ = stdin.write_all(a.as_bytes());
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    }
    // Wait up to ~20s for completion while keeping stdin open.
    for _ in 0..200 {
        if let Ok(Some(status)) = child.try_wait() {
            return status.success();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    false
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
