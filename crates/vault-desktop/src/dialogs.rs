//! Modal dialogs and the settings screen.

use crate::icons as icon;
use egui::{Color32, RichText, Stroke};
use vault_core::app::{APP_NAME, APP_VERSION, RELEASE_LABEL};
use vault_core::diagnostics::{DiagnosticReport, Event};
use vault_core::markdown::{LinkClass, classify_link};
use vault_core::model::VaultPrefs;
use vault_core::{ErrorCode, Passphrase, io_export, strength};
use zeroize::Zeroizing;

use crate::app::{Modal, NilavarApp, SettingsTab, ToastKind};
use crate::theme;
use crate::ui_locked::{passphrase_field, strength_meter};
use crate::worker::{Job, SessionOp};

fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(22, 18))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 36,
            spread: 0,
            color: Color32::from_black_alpha(200),
        })
}

fn title(ui: &mut egui::Ui, ic: &str, color: Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(ic).size(20.0).color(color));
        ui.label(
            RichText::new(text)
                .font(theme::ui_bold(16.0))
                .color(theme::TEXT),
        );
    });
    ui.add_space(6.0);
}

fn body(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::TEXT).size(13.5));
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::MUTED).size(12.0));
}

fn err(ui: &mut egui::Ui, code: Option<ErrorCode>) {
    if let Some(c) = code {
        ui.label(
            RichText::new(format!(
                "{} {} ({})",
                icon::WARNING_CIRCLE,
                c.message(),
                c.id()
            ))
            .color(theme::RED)
            .size(12.5),
        );
    }
}

/// Shows the current modal (if any).
pub fn show(app: &mut NilavarApp, ctx: &egui::Context) {
    let Some(modal) = app.modal.take() else {
        return;
    };
    let mut keep = true;
    let width: f32 = if matches!(modal, Modal::Settings) {
        720.0
    } else {
        460.0
    };
    let width = width.min(ctx.content_rect().width() - 40.0);
    let mut next: Option<Modal> = None;
    let mut modal = modal;
    let resp = egui::Modal::new(egui::Id::unique("nilavar-modal"))
        .frame(modal_frame())
        .backdrop_color(Color32::from_black_alpha(150))
        .show(ctx, |ui| {
            ui.set_width(width);
            keep = modal_body(app, ui, &mut modal, &mut next);
        });
    let dismissable = !matches!(
        modal,
        Modal::Busy(_) | Modal::SavingBeforeLock | Modal::Conflict | Modal::SaveFailedOnLock { .. }
    );
    if resp.should_close() && dismissable && !matches!(modal, Modal::ExportPlaintext { .. }) {
        keep = false;
    }
    if let Some(n) = next {
        app.modal = Some(n);
    } else if keep && app.modal.is_none() {
        app.modal = Some(modal);
    }
}

fn buttons_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(12.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}

/// Returns whether the modal stays open.
fn modal_body(
    app: &mut NilavarApp,
    ui: &mut egui::Ui,
    modal: &mut Modal,
    next: &mut Option<Modal>,
) -> bool {
    let ctx = ui.ctx().clone();
    match modal {
        Modal::Busy(label) => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(theme::GREEN).size(18.0));
                ui.label(RichText::new(*label).color(theme::TEXT));
            });
            true
        }
        Modal::UnsavedOnLock { quit } => {
            let quit = *quit;
            title(
                ui,
                icon::LOCK,
                theme::AMBER,
                if quit {
                    "Quit with unsaved changes?"
                } else {
                    "Lock with unsaved changes?"
                },
            );
            body(
                ui,
                "Some edits have not been written to the encrypted vault file yet.",
            );
            let mut open = true;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("Save and continue")).clicked() {
                    app.lock_choice_save(quit);
                    open = false;
                }
                if ui.add(theme::danger_button("Discard changes")).clicked() {
                    app.lock_choice_discard(quit);
                    open = false;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            open && app.modal.is_none()
        }
        Modal::SavingBeforeLock => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(theme::GREEN).size(18.0));
                ui.label(RichText::new("Saving encrypted vault…").color(theme::TEXT));
            });
            app.modal.is_none()
        }
        Modal::SaveFailedOnLock { code, quit } => {
            let (code, quit) = (*code, *quit);
            title(ui, icon::WARNING_OCTAGON, theme::RED, "Saving failed");
            body(ui, &format!("{} ({})", code.message(), code.id()));
            muted(
                ui,
                "The previous vault file is unchanged. Retry, discard the unsaved edits, or cancel and keep working (for example to save a conflict copy).",
            );
            let mut open = true;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("Retry")).clicked() {
                    app.lock_choice_save(quit);
                    open = false;
                }
                if ui
                    .add(theme::danger_button("Discard and continue"))
                    .clicked()
                {
                    app.lock_choice_discard(quit);
                    open = false;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            open && app.modal.is_none()
        }
        Modal::Conflict => {
            title(
                ui,
                icon::GIT_DIFF,
                theme::AMBER,
                "The vault changed on disk",
            );
            body(
                ui,
                "Another app, device or sync client modified the vault file after you opened it. \
                 Nothing was overwritten.",
            );
            muted(
                ui,
                "Save your version as a separate encrypted conflict copy (recommended), or discard your unsaved edits \
                 and reload the on-disk version. Nilavar never merges automatically.",
            );
            let mut open = true;
            buttons_row(ui, |ui| {
                if ui
                    .add(theme::primary_button("Save conflict copy"))
                    .clicked()
                {
                    app.run_session_op(SessionOp::ConflictCopy, "Writing encrypted conflict copy…");
                    open = false;
                }
                if ui
                    .add(theme::danger_button("Discard mine & reload"))
                    .clicked()
                {
                    app.run_session_op(SessionOp::Reload, "Reloading vault from disk…");
                    open = false;
                }
                if ui.add(theme::secondary_button("Not now")).clicked() {
                    open = false;
                }
            });
            open
        }
        Modal::ChangePassphrase {
            current,
            new,
            confirm,
            reveal,
            error,
        } => {
            title(ui, icon::KEY, theme::GREEN, "Change vault passphrase");
            muted(
                ui,
                "The vault is re-encrypted with the new passphrase and replaced atomically. If anything fails, the old file and passphrase keep working.",
            );
            ui.add_space(6.0);
            passphrase_field(
                ui,
                "cp-current",
                current,
                *reveal,
                "Current passphrase",
                false,
            );
            ui.add_space(4.0);
            passphrase_field(ui, "cp-new", new, *reveal, "New passphrase", false);
            ui.add_space(4.0);
            passphrase_field(
                ui,
                "cp-confirm",
                confirm,
                *reveal,
                "Repeat new passphrase",
                false,
            );
            ui.checkbox(reveal, "Show");
            if !new.is_empty() {
                strength_meter(ui, &strength::estimate(&Passphrase::new(new.to_string())));
            }
            err(ui, *error);
            muted(
                ui,
                "Backups, recovery copies and cloud version history made earlier still open with the OLD passphrase.",
            );
            let mut open = true;
            let mut go = false;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("Change passphrase")).clicked() {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go {
                let cur = Passphrase::new(current.to_string());
                if new.is_empty() {
                    *error = Some(ErrorCode::PassphraseEmpty);
                } else if new.as_str() != confirm.as_str() {
                    *error = Some(ErrorCode::PassphraseMismatch);
                } else if !app
                    .session
                    .as_ref()
                    .is_some_and(|s| s.passphrase_matches(&cur))
                {
                    *error = Some(ErrorCode::WrongPassphraseOrNotAVault);
                } else {
                    let np = Passphrase::new(std::mem::take(&mut **new));
                    app.run_session_op(
                        SessionOp::ChangePassphrase {
                            current: cur,
                            new: np,
                        },
                        "Re-encrypting vault…",
                    );
                    open = false;
                }
            }
            open
        }
        Modal::ExportEncrypted {
            doc,
            pass,
            confirm,
            reveal,
            error,
        } => {
            title(ui, icon::EXPORT, theme::GREEN, "Export encrypted document");
            muted(
                ui,
                "Writes this document's text as a standard age file. Anyone with the export passphrase can open it with age, rage or Nilavar's \"Import encrypted\".",
            );
            ui.add_space(6.0);
            passphrase_field(ui, "ex-pass", pass, *reveal, "Export passphrase", false);
            ui.add_space(4.0);
            passphrase_field(
                ui,
                "ex-confirm",
                confirm,
                *reveal,
                "Repeat export passphrase",
                false,
            );
            ui.checkbox(reveal, "Show");
            if !pass.is_empty() {
                strength_meter(ui, &strength::estimate(&Passphrase::new(pass.to_string())));
            }
            err(ui, *error);
            let mut open = true;
            let mut go = false;
            buttons_row(ui, |ui| {
                if ui
                    .add(theme::primary_button("Choose file & export"))
                    .clicked()
                {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go {
                if pass.is_empty() {
                    *error = Some(ErrorCode::PassphraseEmpty);
                } else if pass.as_str() != confirm.as_str() {
                    *error = Some(ErrorCode::PassphraseMismatch);
                } else if let Some((title_s, text)) = app
                    .session
                    .as_ref()
                    .and_then(|s| s.vault().document(*doc))
                    .map(|d| (d.title.expose().to_owned(), d.text.duplicate()))
                {
                    let name = io_export::suggested_filename(&title_s, "txt.age");
                    if let Some(dest) = rfd::FileDialog::new()
                        .set_title("Save encrypted export")
                        .set_file_name(name)
                        .save_file()
                    {
                        let job = Job::ExportEncrypted {
                            text,
                            dest,
                            pass: Passphrase::new(std::mem::take(&mut **pass)),
                            work: app.work,
                        };
                        if app.worker.submit(app.epoch, job).is_err() {
                            app.toast(ToastKind::Warn, ErrorCode::Busy.message());
                        } else {
                            app.toast(ToastKind::Info, "Encrypting export…");
                        }
                        open = false;
                    }
                }
            }
            open
        }
        Modal::ExportPlaintext { doc, dest, ack } => {
            title(ui, icon::WARNING, theme::AMBER, "Export as PLAINTEXT");
            body(ui, "This writes the document UNENCRYPTED to:");
            egui::Frame::new()
                .fill(theme::BG)
                .stroke(Stroke::new(1.0, theme::AMBER.gamma_multiply(0.6)))
                .corner_radius(4)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(dest.display().to_string())
                                .font(theme::mono(12.5))
                                .color(theme::AMBER),
                        )
                        .wrap(),
                    );
                });
            muted(
                ui,
                "Any person or program that can read that location can read it, and cloud sync, backups or the OS may copy it. Deleting it later does not guarantee it is unrecoverable.",
            );
            ui.checkbox(
                ack,
                RichText::new("I understand this file will not be encrypted").color(theme::TEXT),
            );
            let mut open = true;
            let mut go = false;
            buttons_row(ui, |ui| {
                if ui
                    .add_enabled(*ack, theme::danger_button("Write plaintext file"))
                    .clicked()
                {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go {
                let text = app
                    .session
                    .as_ref()
                    .and_then(|s| s.vault().document(*doc))
                    .map(|d| d.text.duplicate());
                if let Some(text) = text {
                    let r = io_export::confirm_plaintext_export(dest, *ack)
                        .and_then(|c| io_export::export_plaintext(&text, c));
                    match r {
                        Ok(p) => {
                            app.diag.record(Event::Exported { plaintext: true });
                            app.toast(
                                ToastKind::Warn,
                                format!("Plaintext written to {}", p.display()),
                            );
                        }
                        Err(e) => {
                            app.diag.record(Event::ExportFailed(e.code()));
                            app.error_toast(e);
                        }
                    }
                }
                open = false;
            }
            open
        }
        Modal::ImportEncrypted { path, pass, error } => {
            title(
                ui,
                icon::DOWNLOAD_SIMPLE,
                theme::GREEN,
                "Import encrypted text file",
            );
            ui.label(
                RichText::new(path.display().to_string())
                    .font(theme::mono(12.0))
                    .color(theme::MUTED),
            );
            let r = passphrase_field(
                ui,
                "imp-pass",
                pass,
                false,
                "Passphrase of that file",
                false,
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            err(ui, *error);
            let mut open = true;
            let mut go = enter;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("Decrypt & import")).clicked() {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go && !pass.is_empty() {
                let job = Job::ImportEncrypted {
                    path: path.clone(),
                    pass: Passphrase::new(std::mem::take(&mut **pass)),
                };
                if app.worker.submit(app.epoch, job).is_err() {
                    *error = Some(ErrorCode::Busy);
                } else {
                    open = false;
                }
            }
            open
        }
        Modal::RestoreBackup { path, pass, error } => {
            title(
                ui,
                icon::ARROW_COUNTER_CLOCKWISE,
                theme::GREEN,
                "Restore backup as a new vault",
            );
            ui.label(
                RichText::new(path.display().to_string())
                    .font(theme::mono(12.0))
                    .color(theme::MUTED),
            );
            muted(
                ui,
                "The backup is decrypted and written to a NEW vault file you choose (with the same passphrase). Your open vault is not changed.",
            );
            passphrase_field(
                ui,
                "rb-pass",
                pass,
                false,
                "Passphrase of the backup",
                false,
            );
            err(ui, *error);
            let mut open = true;
            let mut go = false;
            buttons_row(ui, |ui| {
                if ui
                    .add(theme::primary_button("Choose new location…"))
                    .clicked()
                {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go
                && !pass.is_empty()
                && let Some(new_path) = rfd::FileDialog::new()
                    .set_title("Save restored vault as")
                    .set_file_name(format!("restored.{}", vault_core::app::VAULT_EXTENSION))
                    .save_file()
            {
                let mut opts = app.session_opts();
                opts.take_lock = false;
                let job = Job::RestoreAsNew {
                    backup: path.clone(),
                    pass: Passphrase::new(std::mem::take(&mut **pass)),
                    new_path,
                    opts,
                };
                if app.worker.submit(app.epoch, job).is_err() {
                    *error = Some(ErrorCode::Busy);
                } else {
                    app.toast(ToastKind::Info, "Restoring backup…");
                    open = false;
                }
            }
            open
        }
        Modal::RenameDoc { doc, name } => {
            title(ui, icon::PENCIL_SIMPLE, theme::GREEN, "Document title");
            let r = ui.add_sized(
                [ui.available_width(), 32.0],
                egui::TextEdit::singleline(&mut **name)
                    .hint_text("Untitled")
                    .margin(egui::Margin::symmetric(8, 6)),
            );
            if !r.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                r.request_focus();
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut open = true;
            let mut go = enter;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("Save")).clicked() {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go {
                let n = name.trim().to_owned();
                let r = app
                    .session
                    .as_mut()
                    .map(|s| s.vault_mut().rename_document(*doc, &n));
                match r {
                    Some(Ok(())) => {
                        app.mark_changed();
                        app.ws.editor.request_focus = true;
                        open = false;
                    }
                    Some(Err(e)) => app.error_toast(e),
                    None => open = false,
                }
            }
            open
        }
        Modal::NewFolder { parent, name } => {
            title(ui, icon::FOLDER_SIMPLE_PLUS, theme::GREEN, "New folder");
            let r = ui.add_sized(
                [ui.available_width(), 32.0],
                egui::TextEdit::singleline(&mut **name).hint_text("Folder name"),
            );
            if !ui.memory(|m| m.focused().is_some()) {
                r.request_focus();
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut open = true;
            let mut go = enter;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("Create")).clicked() {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go {
                let n = name.trim().to_owned();
                let r = app
                    .session
                    .as_mut()
                    .map(|s| s.vault_mut().create_folder(&n, *parent));
                match r {
                    Some(Ok(id)) => {
                        app.mark_changed();
                        if let Some(p) = parent {
                            app.ws.expanded.insert(*p);
                        }
                        app.ws.expanded.insert(id);
                        open = false;
                    }
                    Some(Err(e)) => app.error_toast(e),
                    None => open = false,
                }
            }
            open
        }
        Modal::RenameFolder { folder, name } => {
            title(ui, icon::PENCIL_SIMPLE, theme::GREEN, "Rename folder");
            let r = ui.add_sized(
                [ui.available_width(), 32.0],
                egui::TextEdit::singleline(&mut **name),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut open = true;
            let mut go = enter;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("Rename")).clicked() {
                    go = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            if go {
                let n = name.trim().to_owned();
                let r = app
                    .session
                    .as_mut()
                    .map(|s| s.vault_mut().rename_folder(*folder, &n));
                match r {
                    Some(Ok(())) => {
                        app.mark_changed();
                        open = false;
                    }
                    Some(Err(e)) => app.error_toast(e),
                    None => open = false,
                }
            }
            open
        }
        Modal::ConfirmTrash(doc) => {
            let doc = *doc;
            title(ui, icon::TRASH, theme::RED, "Move to trash?");
            body(
                ui,
                "The document moves to the encrypted trash with its history. You can restore it until you purge it.",
            );
            let mut open = true;
            buttons_row(ui, |ui| {
                if ui.add(theme::danger_button("Move to trash")).clicked() {
                    let r = app
                        .session
                        .as_mut()
                        .map(|s| s.vault_mut().trash_document(doc));
                    match r {
                        Some(Ok(())) => {
                            app.close_tab(doc);
                            app.mark_changed();
                        }
                        Some(Err(e)) => app.error_toast(e),
                        None => {}
                    }
                    open = false;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            open
        }
        Modal::ConfirmPurge(doc) => {
            let doc = *doc;
            title(ui, icon::X_CIRCLE, theme::RED, "Delete forever?");
            body(
                ui,
                "The document and its history are removed from the vault on the next save.",
            );
            muted(
                ui,
                "Older vault copies, recovery copies, backups and cloud version history may still contain it.",
            );
            let mut open = true;
            buttons_row(ui, |ui| {
                if ui.add(theme::danger_button("Delete forever")).clicked() {
                    if let Some(Err(e)) = app
                        .session
                        .as_mut()
                        .map(|s| s.vault_mut().purge_from_trash(doc))
                    {
                        app.error_toast(e);
                    } else {
                        app.mark_changed();
                    }
                    open = false;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            open
        }
        Modal::ConfirmEmptyTrash => {
            title(ui, icon::TRASH, theme::RED, "Empty trash?");
            body(
                ui,
                "All trashed documents and their history are removed from the vault on the next save.",
            );
            muted(
                ui,
                "Older vault copies, recovery copies, backups and cloud version history may still contain them.",
            );
            let mut open = true;
            buttons_row(ui, |ui| {
                if ui.add(theme::danger_button("Empty trash")).clicked() {
                    if let Some(s) = app.session.as_mut() {
                        s.vault_mut().empty_trash();
                    }
                    app.mark_changed();
                    open = false;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    open = false;
                }
            });
            open
        }
        Modal::Link(dest) => {
            let class = classify_link(dest);
            title(ui, icon::LINK, theme::CYAN, "Link");
            muted(ui, "Destination:");
            egui::Frame::new()
                .fill(theme::BG)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(4)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(dest.as_str())
                                .font(theme::mono(12.5))
                                .color(theme::CYAN),
                        )
                        .wrap(),
                    );
                });
            match class {
                LinkClass::Web | LinkClass::Mail => muted(
                    ui,
                    "Opening hands this address to your default browser or mail app. Nilavar itself makes no network requests.",
                ),
                LinkClass::CopyOnly => muted(
                    ui,
                    "This kind of link (file, relative, script or custom scheme) is never opened by Nilavar. You can copy it.",
                ),
            }
            let mut open = true;
            let d = dest.clone();
            buttons_row(ui, |ui| {
                if class != LinkClass::CopyOnly
                    && ui.add(theme::primary_button("Open externally")).clicked()
                {
                    ctx.open_url(egui::OpenUrl::new_tab(d.clone()));
                    open = false;
                }
                if ui.add(theme::secondary_button("Copy link")).clicked() {
                    app.copy_secret(&d);
                    open = false;
                }
                if ui.add(theme::secondary_button("Close")).clicked() {
                    open = false;
                }
            });
            open
        }
        Modal::Notice { title: t, body: b } => {
            title(ui, icon::INFO, theme::CYAN, t);
            body(ui, b);
            let mut open = true;
            buttons_row(ui, |ui| {
                if ui.add(theme::primary_button("OK")).clicked() {
                    open = false;
                }
            });
            open
        }
        Modal::Settings => settings(app, ui, next),
    }
}

// ---------------------------------------------------------------------------

fn settings(app: &mut NilavarApp, ui: &mut egui::Ui, next: &mut Option<Modal>) -> bool {
    let mut open = true;
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon::GEAR_SIX).size(20.0).color(theme::GREEN));
        ui.label(
            RichText::new("Settings")
                .font(theme::ui_bold(17.0))
                .color(theme::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(icon::X).frame(false))
                .on_hover_text("Close (Esc)")
                .clicked()
            {
                open = false;
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        for (tab, label) in [
            (SettingsTab::Editor, "Editor"),
            (SettingsTab::Security, "Security"),
            (SettingsTab::Appearance, "Appearance"),
            (SettingsTab::Vault, "Vault"),
            (SettingsTab::Diagnostics, "Diagnostics"),
            (SettingsTab::About, "About & recovery"),
        ] {
            let on = app.settings_tab == tab;
            if ui
                .add(
                    egui::Button::new(RichText::new(label).color(if on {
                        theme::GREEN
                    } else {
                        theme::MUTED
                    }))
                    .frame_when_inactive(on)
                    .fill(theme::GREEN_DIM),
                )
                .clicked()
            {
                app.settings_tab = tab;
            }
        }
    });
    ui.add_space(8.0);
    let h = (ui.ctx().content_rect().height() * 0.62).clamp(260.0, 560.0);
    egui::ScrollArea::vertical()
        .max_height(h)
        .auto_shrink([false, false])
        .show(ui, |ui| match app.settings_tab {
            SettingsTab::Editor => editor_tab(app, ui),
            SettingsTab::Security => security_tab(app, ui),
            SettingsTab::Appearance => appearance_tab(app, ui),
            SettingsTab::Vault => vault_tab(app, ui, next, &mut open),
            SettingsTab::Diagnostics => diagnostics_tab(app, ui),
            SettingsTab::About => about_tab(app, ui),
        });
    open
}

fn editor_tab(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let s = &mut app.settings;
    let mut changed = false;
    changed |= ui
        .add(
            egui::Slider::new(&mut s.editor_font_size, 10.0..=28.0)
                .text("Editor font size")
                .step_by(0.5),
        )
        .changed();
    changed |= ui
        .checkbox(&mut s.soft_wrap, "Soft wrap long lines")
        .changed();
    changed |= ui.checkbox(&mut s.line_numbers, "Line numbers").changed();
    changed |= ui
        .checkbox(&mut s.highlight_current_line, "Highlight current line")
        .changed();
    let mut scale = s.ui_scale;
    if ui
        .add(
            egui::Slider::new(&mut scale, 0.8..=1.6)
                .text("Interface scale")
                .step_by(0.05),
        )
        .changed()
    {
        s.ui_scale = scale;
        ui.ctx().set_zoom_factor(scale);
        changed = true;
    }
    ui.add_space(8.0);
    muted(
        ui,
        "Shortcuts: ⌘N new · ⌘S save · ⌘F find · ⌥⌘F replace · ⇧⌘F search vault · ⌘P go to document · ⇧⌘P commands · ⌘W close tab · ⌘E preview · ⌘\\ sidebar · ⇧⌘\\ focus · ⇧⌘L lock · ⌘Z / ⇧⌘Z undo/redo · Tab / ⇧Tab indent (Ctrl instead of ⌘ on Windows/Linux).",
    );
    if changed {
        app.save_settings();
    }
}

fn security_tab(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let Some(sess) = app.session.as_mut() else {
        muted(ui, "Unlock a vault to change its security preferences.");
        return;
    };
    let mut prefs = sess.vault().prefs;
    let before = prefs;
    ui.label(
        RichText::new("Stored encrypted inside this vault")
            .size(11.5)
            .color(theme::MUTED),
    );
    let mut mins = prefs.idle_lock_secs / 60;
    ui.add(egui::Slider::new(&mut mins, 1..=240).text("Auto-lock after idle (minutes)"));
    prefs.idle_lock_secs = mins * 60;
    ui.add(
        egui::Slider::new(
            &mut prefs.clipboard_clear_secs,
            VaultPrefs::CLIPBOARD_RANGE.0..=VaultPrefs::CLIPBOARD_RANGE.1,
        )
        .text("Clear copied text after (seconds)"),
    );
    ui.checkbox(
        &mut prefs.autosave,
        "Encrypted autosave (debounced; ⌘S always saves)",
    );
    ui.checkbox(
        &mut prefs.lock_on_sleep,
        "Lock when the computer wakes from sleep",
    );
    prefs = prefs.clamped();
    if prefs != before {
        sess.vault_mut().prefs = prefs;
        app.mark_changed();
    }
    ui.add_space(8.0);
    ui.label(RichText::new("This device").size(11.5).color(theme::MUTED));
    let mut changed = false;
    changed |= ui
        .checkbox(
            &mut app.settings.lock_on_focus_loss,
            "Lock when the window loses focus",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut app.settings.remember_last_vault,
            "Remember the last vault's location (stored unencrypted in app settings)",
        )
        .changed();
    if !app.settings.remember_last_vault {
        app.settings.last_vault = None;
    }
    if changed {
        app.save_settings();
    }
    ui.add_space(8.0);
    muted(
        ui,
        "Clipboard: copying is best effort. Nilavar clears text it placed on the clipboard only if it is still unchanged, \
         and asks the OS to keep it out of clipboard history where supported. Other apps or clipboard managers may already \
         have read it. Screen locking: sleep is detected on wake (lock-on-resume); screen-lock notifications are not wired up \
         in this build — use idle lock.",
    );
}

fn appearance_tab(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let mut changed = false;
    changed |= ui
        .checkbox(
            &mut app.settings.effects,
            "Visual effects (digital rain on the locked screen only)",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut app.settings.reduce_motion,
            "Reduce motion (static effects, no animation)",
        )
        .changed();
    if changed {
        app.save_settings();
    }
    muted(
        ui,
        "Effects never appear behind editing text and use random decorative glyphs, never document content.",
    );
}

fn vault_tab(app: &mut NilavarApp, ui: &mut egui::Ui, next: &mut Option<Modal>, open: &mut bool) {
    let Some(sess) = app.session.as_ref() else {
        return;
    };
    let v = sess.vault();
    let store = sess.recovery_store();
    let rows = [
        ("File", sess.path().display().to_string()),
        ("Documents", v.documents.len().to_string()),
        ("Folders", v.folders.len().to_string()),
        ("Trash", v.trash.len().to_string()),
        ("Saved versions", v.total_history_entries().to_string()),
        (
            "Payload size",
            format!(
                "~{:.1} MiB of {} MiB limit",
                v.approx_payload_bytes() as f64 / 1048576.0,
                vault_core::limits::MAX_PAYLOAD_BYTES / 1048576
            ),
        ),
        ("Saves", v.save_counter.to_string()),
        ("Last saved", vault_core::time::format_utc(v.saved_at)),
    ];
    egui::Grid::new("vault-grid")
        .num_columns(2)
        .spacing(egui::vec2(12.0, 6.0))
        .show(ui, |ui| {
            for (k, val) in rows {
                ui.label(RichText::new(k).color(theme::MUTED));
                ui.add(
                    egui::Label::new(
                        RichText::new(val)
                            .font(theme::mono(12.0))
                            .color(theme::TEXT),
                    )
                    .wrap(),
                );
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    let ctx = ui.ctx().clone();
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(theme::secondary_button(format!(
                "{} Change passphrase…",
                icon::KEY
            )))
            .clicked()
        {
            app.run_command(crate::commands::Command::ChangePassphrase, &ctx);
            *next = app.modal.take();
            *open = false;
        }
        if ui
            .add(theme::secondary_button(format!(
                "{} Back up now…",
                icon::FLOPPY_DISK
            )))
            .clicked()
        {
            *open = false;
            app.backup_pick();
            *next = app.modal.take();
        }
        if ui
            .add(theme::secondary_button(format!(
                "{} Restore backup as new…",
                icon::ARROW_COUNTER_CLOCKWISE
            )))
            .clicked()
        {
            app.run_command(crate::commands::Command::RestoreBackup, &ctx);
            *next = app.modal.take();
            *open = false;
        }
    });
    ui.add_space(10.0);
    ui.label(
        RichText::new("Recovery copies")
            .font(theme::ui_bold(14.0))
            .color(theme::TEXT),
    );
    muted(
        ui,
        "Encrypted last-known-good copies of the vault file (made before saves, at most every 15 minutes, newest 5 kept) and \
         unsaved-changes copies written if an automatic lock could not save. They sit on THIS computer's disk next to app data — \
         not an independent backup — and stay encrypted with the passphrase current when written.",
    );
    ui.label(
        RichText::new(store.dir().display().to_string())
            .font(theme::mono(11.0))
            .color(theme::MUTED),
    );
    let copies = store.list();
    if copies.is_empty() {
        muted(ui, "No recovery copies yet.");
    }
    for c in copies.iter().take(12) {
        ui.horizontal(|ui| {
            let kind = match c.kind {
                vault_core::storage::RecoveryKind::LastKnownGood => "last-known-good",
                vault_core::storage::RecoveryKind::Unsaved => "UNSAVED CHANGES",
            };
            ui.label(
                RichText::new(format!("{kind}  {}", c.name))
                    .font(theme::mono(11.5))
                    .color(theme::TEXT),
            );
            if ui.small_button("Restore as new…").clicked() {
                *next = Some(Modal::RestoreBackup {
                    path: c.path.clone(),
                    pass: Zeroizing::new(String::new()),
                    error: None,
                });
                *open = false;
            }
        });
    }
    if !copies.is_empty()
        && ui
            .add(theme::danger_button(format!(
                "{} Delete all recovery copies",
                icon::TRASH
            )))
            .on_hover_text(
                "Normal file deletion; it does not guarantee erasure from SSD/flash storage.",
            )
            .clicked()
    {
        let n = store.purge_all();
        app.toast(ToastKind::Info, format!("Deleted {n} recovery copies."));
    }
}

fn diagnostics_tab(app: &mut NilavarApp, ui: &mut egui::Ui) {
    muted(
        ui,
        "An optional local report: app/platform versions, capability flags and error codes from this session. \
         It never contains passphrases, document text, titles, tags, file paths, screenshots or memory. Nothing is ever uploaded.",
    );
    if ui
        .add(theme::secondary_button("Generate report preview"))
        .clicked()
    {
        app.report_preview = Some(DiagnosticReport::build(&app.diag, app.caps).to_json());
    }
    if let Some(r) = app.report_preview.clone() {
        egui::ScrollArea::vertical()
            .id_salt("report")
            .max_height(240.0)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&r).font(theme::mono(11.0)).color(theme::TEXT))
                        .wrap(),
                );
            });
        if ui.add(theme::primary_button("Save report…")).clicked()
            && let Some(dest) = rfd::FileDialog::new()
                .set_title("Save diagnostic report")
                .set_file_name("nilavar-diagnostics.json")
                .save_file()
        {
            match vault_core::storage::write_atomic(
                &dest,
                r.as_bytes(),
                None,
                vault_core::storage::Faults::NONE,
            ) {
                Ok(_) => app.toast(ToastKind::Success, "Diagnostic report saved."),
                Err(e) => app.error_toast(e),
            }
        }
    }
}

fn about_tab(app: &mut NilavarApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(format!("{APP_NAME} {APP_VERSION} — {RELEASE_LABEL}"))
            .font(theme::ui_bold(14.0))
            .color(theme::GREEN),
    );
    muted(
        ui,
        "This build has not been independently audited. Do not rely on it as your only copy of critical secrets.",
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("Encryption")
            .font(theme::ui_bold(13.0))
            .color(theme::TEXT),
    );
    body(
        ui,
        "age v1 passphrase mode (scrypt, work factor 2^18; files up to 2^20 accepted) via the Rust `age` crate 0.12.1, which its maintainers label BETA. Fresh file key, salt and nonce on every save.",
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("Capabilities on this computer")
            .font(theme::ui_bold(13.0))
            .color(theme::TEXT),
    );
    let c = app.caps;
    let row = |ui: &mut egui::Ui, ok: bool, yes: &str, no: &str| {
        let (ic, col, txt) = if ok {
            (icon::CHECK_CIRCLE, theme::GREEN, yes)
        } else {
            (icon::MINUS_CIRCLE, theme::AMBER, no)
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(ic).color(col));
            ui.label(RichText::new(txt).size(12.5).color(theme::TEXT));
        });
    };
    row(
        ui,
        c.no_app_networking,
        "No networking implemented in the app (no HTTP client, no telemetry, no updates).",
        "",
    );
    row(
        ui,
        c.os_network_sandbox,
        "Running inside the macOS App Sandbox without the network entitlement (OS-enforced).",
        "Not running in an OS sandbox: \"no networking\" is a property of the code, not enforced by the OS here.",
    );
    row(
        ui,
        c.core_dumps_disabled,
        "Core dumps disabled for this process.",
        "Core dumps could not be disabled on this platform.",
    );
    row(
        ui,
        c.sleep_lock_signals,
        "Sleep detected on wake (lock-on-resume).",
        "System sleep/lock signals unavailable.",
    );
    row(
        ui,
        c.clipboard_privacy_hints,
        "Clipboard privacy hints applied (exclude from history where supported).",
        "No clipboard privacy hints on this platform.",
    );
    ui.add_space(8.0);
    ui.label(
        RichText::new("Recovery checklist")
            .font(theme::ui_bold(13.0))
            .color(theme::TEXT),
    );
    for line in [
        "1. Keep at least two copies of the vault file in different places (e.g. cloud drive + offline USB).",
        "2. Store the passphrase somewhere safe offline. There is no reset and no backdoor.",
        "3. To open on another computer: install Nilavar, open the vault file, enter the passphrase. Nothing else is needed — no account, no device key.",
        "4. Test it: open a backup on a second machine (or with `nilavar-cli verify`) before you need it.",
        "5. Without Nilavar: `age -d vault.nvault.age > payload.json` decrypts the vault; docs/FORMAT.md describes the JSON.",
        "6. After a passphrase change, old backups still open with the old passphrase — delete them if it was compromised.",
    ] {
        body(ui, line);
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new("What this app cannot protect against")
            .font(theme::ui_bold(13.0))
            .color(theme::TEXT),
    );
    muted(
        ui,
        "Malware or another person with access to your unlocked user session, administrator/root attackers, screen capture or recording, clipboard history managers, OS swap/hibernation files (use full-disk encryption), and guaranteed erasure on SSDs. See THREAT_MODEL.md.",
    );
}
