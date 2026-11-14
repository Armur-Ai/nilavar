//! Shared helpers for integration tests. All data is synthetic.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// Runs a reference tool under a pseudo-terminal (via `tests/pty_drive.py`)
/// and answers each passphrase prompt in order. Returns success.
pub fn run_with_pty(tool: &Path, args: &[&str], answers: &[&str]) -> bool {
    let driver = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pty_drive.py");
    let mut cmd = Command::new("python3");
    cmd.arg(driver).args(answers).arg("--").arg(tool).args(args);
    // Keep pinentry off PATH so rage prompts on the terminal.
    cmd.env("PATH", "/usr/bin:/bin");
    // stderr is inherited so a failing tool's transcript shows in test logs.
    cmd.stdin(Stdio::null()).stdout(Stdio::null());
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

#[allow(dead_code)]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
