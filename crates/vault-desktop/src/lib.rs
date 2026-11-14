//! Nilavar desktop application library (the binary is a thin wrapper so
//! the real application can be driven headlessly in UI tests).
//!
//! First-party desktop code is free of `unsafe` except for the documented
//! libc calls in `platform::disable_core_dumps`.

#![deny(unsafe_code)]

pub mod app;
pub mod commands;
pub mod dialogs;
pub mod editor;
pub mod highlight;
pub mod icons;
pub mod platform;
pub mod preview;
pub mod rain;
pub mod settings;
pub mod theme;
pub mod ui_locked;
pub mod ui_workspace;
pub mod worker;

use std::time::Instant;

use app::{Modal, NilavarApp};
use vault_core::app::APP_NAME;
use vault_core::diagnostics::{Capabilities, LockReason};
use vault_core::lifecycle::LockState;
use vault_core::paths::DataDirs;

impl eframe::App for NilavarApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
        if self.quit_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Wipe egui per-widget memory (text-edit undo snapshots) when asked.
        if std::mem::take(&mut self.wipe_egui_memory) {
            ctx.memory_mut(|m| {
                m.data.clear();
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
        }
        for id in std::mem::take(&mut self.pending_forget) {
            ctx.memory_mut(|m| m.data.remove::<egui::text_edit::TextEditState>(id));
        }

        // User activity resets the idle timer.
        let active = ctx.input(|i| !i.events.is_empty() || i.pointer.delta() != egui::Vec2::ZERO);
        if active {
            self.last_activity = Instant::now();
        }

        // Window close.
        if ctx.input(|i| i.viewport().close_requested()) {
            match self.state {
                LockState::Locked => {}
                LockState::Unlocked | LockState::Saving => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.request_lock(LockReason::WindowClosed);
                }
                LockState::Unlocking | LockState::Locking => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
            }
        }

        // Optional lock on focus loss.
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        let lost_focus = self.was_focused && !focused;
        self.was_focused = focused;
        if self.settings.lock_on_focus_loss
            && lost_focus
            && self.is_unlocked()
            && !matches!(self.modal, Some(Modal::Busy(_)))
        {
            self.request_lock(LockReason::FocusLost);
        }

        self.handle_shortcuts(&ctx);

        if self.state.shows_workspace() && (self.session.is_some() || self.session_away) {
            ui_workspace::show(self, ui);
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::BG))
                .show(ui, |ui| ui_locked::show(self, ui));
        }
        dialogs::show(self, &ctx);

        // Route copy/cut through the clipboard guard (privacy hints + timed
        // clear), and never publish selections to the X11/Wayland PRIMARY
        // selection.
        let copies: Vec<String> = ctx.output_mut(|o| {
            let mut v = Vec::new();
            o.commands.retain_mut(|c| match c {
                egui::OutputCommand::CopyText(s) => {
                    v.push(std::mem::take(s));
                    false
                }
                egui::OutputCommand::TextSelectionSettled(s) => {
                    zeroize::Zeroize::zeroize(s);
                    false
                }
                _ => true,
            });
            v
        });
        for mut s in copies {
            if self.is_unlocked() {
                let secs = self
                    .session
                    .as_ref()
                    .map(|x| x.vault().prefs.clipboard_clear_secs)
                    .unwrap_or(30);
                self.clip_guard.copy(
                    &mut *self.clipboard,
                    &s,
                    Instant::now(),
                    std::time::Duration::from_secs(u64::from(secs)),
                );
            } else {
                // Locked screens contain no vault content; plain copy.
                self.clipboard.write_text(&s);
            }
            zeroize::Zeroize::zeroize(&mut s);
        }
    }
}

/// Capabilities detected for this process (after hardening).
pub fn detect_capabilities(core_dumps_disabled: bool, network_sandbox: bool) -> Capabilities {
    Capabilities {
        no_app_networking: true,
        os_network_sandbox: network_sandbox || platform::in_macos_sandbox(),
        core_dumps_disabled,
        sleep_lock_signals: true,
        clipboard_privacy_hints: platform::SystemClipboard::privacy_hints_supported(),
    }
}

/// Runs the application.
pub fn run() -> eframe::Result {
    platform::install_panic_hook();
    let core_dumps_disabled = platform::disable_core_dumps();
    // Deny all network access before any other thread starts.
    let network_sandbox = platform::deny_network();

    let data = match DataDirs::resolve() {
        Ok(d) => d,
        Err(_) => DataDirs::at(std::env::temp_dir().join("nilavar-fallback")),
    };
    let caps = detect_capabilities(core_dumps_disabled, network_sandbox);

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([520.0, 420.0])
        .with_app_id(vault_core::app::APP_SLUG);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Never persist egui/window state: it could contain plaintext.
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(NilavarApp::new(cc, data, caps)))),
    )
}
