//! Application data directories.
//!
//! Application data (non-sensitive settings, encrypted recovery copies and
//! local coordination locks) lives in a per-user directory:
//!
//! | Platform | Release builds                                  |
//! |----------|-------------------------------------------------|
//! | macOS    | `~/Library/Application Support/Nilavar`         |
//! | Linux    | `$XDG_DATA_HOME/nilavar` or `~/.local/share/nilavar` |
//! | Windows  | `%APPDATA%\Nilavar`                             |
//!
//! Debug builds use a separate `…-dev` directory so development runs never
//! touch normal user data. `NILAVAR_DATA_DIR` overrides the location (used by
//! the test-suite and for portable setups); it holds a path, never a secret.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::app::{APP_NAME, APP_SLUG};

/// Environment variable that overrides the data directory.
pub const DATA_DIR_ENV: &str = "NILAVAR_DATA_DIR";

/// Resolved application data directories.
#[derive(Clone, Debug)]
pub struct DataDirs {
    root: PathBuf,
}

impl DataDirs {
    /// Uses an explicit root directory.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves the platform default (or the override variable).
    pub fn resolve() -> io::Result<Self> {
        if let Some(p) = std::env::var_os(DATA_DIR_ENV).filter(|p| !p.is_empty()) {
            return Ok(Self::at(PathBuf::from(p)));
        }
        let base =
            platform_base().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home"))?;
        let name = if cfg!(debug_assertions) {
            format!("{}-dev", dir_name())
        } else {
            dir_name().to_owned()
        };
        Ok(Self::at(base.join(name)))
    }

    /// Root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Non-sensitive global settings file.
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Directory for encrypted recovery copies.
    pub fn recovery_dir(&self) -> PathBuf {
        self.root.join("recovery")
    }

    /// Directory for local coordination lock files.
    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    /// Creates the root with owner-only permissions where supported.
    pub fn ensure(&self) -> io::Result<()> {
        create_private_dir_all(&self.root)
    }
}

fn dir_name() -> &'static str {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        APP_NAME
    } else {
        APP_SLUG
    }
}

fn platform_base() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

/// `create_dir_all` with mode 0700 on Unix.
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}
