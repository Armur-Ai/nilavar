//! Global, non-sensitive settings stored in plaintext JSON in the app data
//! directory. Nothing here may contain document content, titles, tags,
//! passphrases or search terms. Vault-specific preferences live inside the
//! encrypted vault instead.
//!
//! The only path stored is the last opened vault path, and only when
//! "Remember last vault" is enabled (default on; documented in PRIVACY.md).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vault_core::ErrorCode;

/// Notice to show on the next unlock after an automatic lock could not save.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingNotice {
    /// Changes were written as an encrypted unsaved-changes recovery copy.
    RecoveryCopy,
    /// Changes were discarded because neither save nor recovery copy worked.
    Discarded,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub reduce_motion: bool,
    pub effects: bool,
    pub editor_font_size: f32,
    pub soft_wrap: bool,
    pub line_numbers: bool,
    pub highlight_current_line: bool,
    pub remember_last_vault: bool,
    pub last_vault: Option<PathBuf>,
    pub lock_on_focus_loss: bool,
    pub ui_scale: f32,
    pub pending_notice: Option<PendingNotice>,
    /// Error code (e.g. "NV-305") associated with the pending notice.
    pub pending_notice_code: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            reduce_motion: false,
            effects: true,
            editor_font_size: 14.5,
            soft_wrap: true,
            line_numbers: true,
            highlight_current_line: true,
            remember_last_vault: true,
            last_vault: None,
            lock_on_focus_loss: false,
            ui_scale: 1.0,
            pending_notice: None,
            pending_notice_code: None,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        // Bounded read; malformed settings fall back to defaults.
        match vault_core::storage::read_bounded(path, 64 * 1024) {
            Ok(bytes) => serde_json::from_slice::<Settings>(&bytes)
                .map(Settings::sanitized)
                .unwrap_or_default(),
            Err(_) => Settings::default(),
        }
    }

    fn sanitized(mut self) -> Self {
        self.editor_font_size = self.editor_font_size.clamp(10.0, 28.0);
        self.ui_scale = self.ui_scale.clamp(0.8, 1.6);
        if !self.remember_last_vault {
            self.last_vault = None;
        }
        self
    }

    pub fn save(&self, path: &Path) -> Result<(), ErrorCode> {
        if let Some(dir) = path.parent() {
            vault_core::paths::create_private_dir_all(dir).map_err(|_| ErrorCode::WriteFailed)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| ErrorCode::Internal)?;
        vault_core::storage::write_atomic(path, &bytes, None, vault_core::storage::Faults::NONE)
            .map(|_| ())
            .map_err(|e| e.code())
    }
}
