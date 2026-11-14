//! Welcome, unlock and create screens (and the masked Locking/Unlocking states).

use crate::icons as icon;
use egui::{Align2, Color32, RichText, Stroke};
use vault_core::app::{
    APP_NAME, APP_VERSION, DEFAULT_VAULT_FILENAME, RELEASE_LABEL, VAULT_EXTENSION,
};
use vault_core::lifecycle::LockState;
use vault_core::{ErrorCode, Passphrase, strength};
use zeroize::Zeroizing;

use crate::app::{CreateForm, NilavarApp, Screen, UnlockForm};
use crate::theme;

pub fn show(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, theme::BG);
    let animate = app.settings.effects && !app.settings.reduce_motion;
    if app.settings.effects {
        let painter = ui.painter().clone();
        if app.rain.paint(&painter, rect, animate) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    let card_w = 440.0_f32.min(rect.width() - 32.0);
    egui::Area::new(egui::Id::unique("locked-card"))
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .order(egui::Order::Middle)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(0x0B, 0x12, 0x10, 245))
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(28, 26))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 12],
                    blur: 40,
                    spread: 0,
                    color: Color32::from_black_alpha(200),
                })
                .show(ui, |ui| {
                    ui.set_width(card_w);
                    header(ui);
                    ui.add_space(14.0);
                    match app.state {
                        LockState::Unlocking => busy(
                            ui,
                            "Deriving key and decrypting…",
                            "scrypt is deliberately slow; this takes about a second.",
                        ),
                        LockState::Locking => busy(
                            ui,
                            "Locking…",
                            "Clearing the session and finishing background work.",
                        ),
                        _ => match &app.screen {
                            Screen::Welcome => welcome(app, ui),
                            Screen::Unlock(_) => unlock(app, ui),
                            Screen::Create(_) => create(app, ui),
                        },
                    }
                });
        });
}

fn header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        theme::glow_dot(ui.painter(), r.center(), theme::GREEN);
        ui.label(
            RichText::new(APP_NAME.to_uppercase())
                .font(theme::mono_bold(20.0))
                .color(theme::GREEN)
                .extra_letter_spacing(4.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("v{APP_VERSION}"))
                    .font(theme::mono(11.0))
                    .color(theme::MUTED),
            );
        });
    });
    ui.label(
        RichText::new("offline encrypted workspace")
            .font(theme::mono(12.0))
            .color(theme::MUTED),
    );
    ui.add_space(2.0);
    ui.label(
        RichText::new(format!(
            "{RELEASE_LABEL} · no network features · age passphrase encryption"
        ))
        .font(theme::mono(10.5))
        .color(theme::MUTED.gamma_multiply(0.8)),
    );
}

fn busy(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().color(theme::GREEN).size(18.0));
        ui.label(RichText::new(title).color(theme::TEXT).size(15.0));
    });
    ui.label(RichText::new(sub).color(theme::MUTED).size(12.5));
    ui.add_space(10.0);
}

fn error_line(ui: &mut egui::Ui, code: ErrorCode) {
    egui::Frame::new()
        .fill(Color32::from_rgb(0x2A, 0x10, 0x13))
        .stroke(Stroke::new(1.0, theme::RED.gamma_multiply(0.6)))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icon::WARNING_CIRCLE).color(theme::RED));
                ui.label(RichText::new(code.message()).color(theme::TEXT).size(13.0));
                ui.label(
                    RichText::new(code.id())
                        .font(theme::mono(11.0))
                        .color(theme::MUTED),
                );
            });
        });
}

fn welcome(app: &mut NilavarApp, ui: &mut egui::Ui) {
    ui.label(RichText::new("Open an existing vault or create a new one.").color(theme::TEXT));
    ui.add_space(12.0);
    let w = ui.available_width();
    if ui
        .add_sized(
            [w, 38.0],
            theme::primary_button(format!("{}  Open vault…", icon::FOLDER_OPEN)),
        )
        .clicked()
    {
        pick_and_unlock(app);
    }
    ui.add_space(6.0);
    if ui
        .add_sized(
            [w, 38.0],
            theme::secondary_button(format!("{}  Create new vault…", icon::PLUS_CIRCLE)),
        )
        .clicked()
    {
        app.screen = Screen::Create(CreateForm {
            path: None,
            pass: Zeroizing::new(String::new()),
            confirm: Zeroizing::new(String::new()),
            reveal: false,
            ack_no_reset: false,
            error: None,
        });
    }
    if let Some(p) = app.settings.last_vault.clone()
        && app.settings.remember_last_vault
    {
        ui.add_space(6.0);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if ui
            .add_sized(
                [w, 32.0],
                theme::secondary_button(format!("{}  Unlock {name}", icon::LOCK_KEY)),
            )
            .on_hover_text(p.display().to_string())
            .clicked()
        {
            app.screen = unlock_screen(p);
        }
    }
    ui.add_space(14.0);
    footer_note(
        ui,
        "Vaults are single encrypted files. Copy one to a cloud drive or USB stick, then open it on another computer with the same passphrase.",
    );
    effects_toggle(app, ui);
}

pub fn unlock_screen(path: std::path::PathBuf) -> Screen {
    Screen::Unlock(UnlockForm {
        path,
        pass: Zeroizing::new(String::new()),
        reveal: false,
        error: None,
        focus: true,
    })
}

fn pick_and_unlock(app: &mut NilavarApp) {
    if let Some(p) = rfd::FileDialog::new()
        .set_title("Open vault")
        .add_filter("Nilavar vault", &["age"])
        .add_filter("All files", &["*"])
        .pick_file()
    {
        app.screen = unlock_screen(p);
    }
}

fn footer_note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::MUTED).size(12.0));
}

fn effects_toggle(app: &mut NilavarApp, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut app.settings.effects,
                RichText::new("Visual effects")
                    .size(12.0)
                    .color(theme::MUTED),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut app.settings.reduce_motion,
                RichText::new("Reduce motion")
                    .size(12.0)
                    .color(theme::MUTED),
            )
            .changed();
        if changed {
            app.save_settings();
        }
    });
}

pub fn passphrase_field(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut Zeroizing<String>,
    reveal: bool,
    hint: &str,
    focus: bool,
) -> egui::Response {
    let r = ui.add_sized(
        [ui.available_width(), 34.0],
        egui::TextEdit::singleline(&mut **value)
            .id(egui::Id::unique(id))
            .password(!reveal)
            .font(theme::mono(14.0))
            .hint_text(hint)
            .margin(egui::Margin::symmetric(10, 8)),
    );
    if focus {
        r.request_focus();
    }
    r
}

fn unlock(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let Screen::Unlock(form) = &mut app.screen else {
        return;
    };
    let name = form
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon::LOCK_KEY).color(theme::GREEN).size(18.0));
        ui.label(
            RichText::new(&name)
                .font(theme::mono(14.0))
                .color(theme::TEXT),
        );
    });
    ui.label(
        RichText::new(
            form.path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        )
        .font(theme::mono(11.0))
        .color(theme::MUTED),
    );
    ui.add_space(10.0);
    let focus = std::mem::take(&mut form.focus);
    let resp = passphrase_field(
        ui,
        "unlock-pass",
        &mut form.pass,
        form.reveal,
        "Passphrase",
        focus,
    );
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut form.reveal,
            RichText::new("Show passphrase")
                .size(12.5)
                .color(theme::MUTED),
        );
    });
    if let Some(code) = form.error {
        ui.add_space(4.0);
        error_line(ui, code);
    }
    ui.add_space(10.0);
    let w = ui.available_width();
    let mut go = enter;
    if ui
        .add_sized(
            [w, 38.0],
            theme::primary_button(format!("{}  Unlock", icon::LOCK_KEY_OPEN)),
        )
        .clicked()
    {
        go = true;
    }
    ui.add_space(4.0);
    let mut back = false;
    let mut other = false;
    ui.horizontal(|ui| {
        if ui.link(RichText::new("Back").size(12.5)).clicked() {
            back = true;
        }
        ui.label(RichText::new("·").color(theme::MUTED));
        if ui
            .link(RichText::new("Open a different vault…").size(12.5))
            .clicked()
        {
            other = true;
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        back = true;
    }
    ui.add_space(8.0);
    footer_note(
        ui,
        "There is no password reset or recovery service. The passphrase is the only way to open this vault.",
    );
    if go {
        app.start_unlock();
    } else if back {
        app.screen = Screen::Welcome;
    } else if other {
        pick_and_unlock(app);
    }
}

fn create(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let Screen::Create(form) = &mut app.screen else {
        return;
    };
    ui.label(
        RichText::new("Create a new vault")
            .font(theme::ui_bold(16.0))
            .color(theme::TEXT),
    );
    ui.add_space(8.0);

    // Location.
    ui.horizontal(|ui| {
        let label = match &form.path {
            Some(p) => p.display().to_string(),
            None => "No location chosen".to_owned(),
        };
        ui.label(RichText::new(icon::FILE_LOCK).color(theme::GREEN));
        ui.add(
            egui::Label::new(RichText::new(label).font(theme::mono(12.0)).color(
                if form.path.is_some() {
                    theme::TEXT
                } else {
                    theme::MUTED
                },
            ))
            .truncate(),
        );
    });
    if ui
        .add(theme::secondary_button(format!(
            "{}  Choose location…",
            icon::FLOPPY_DISK
        )))
        .clicked()
        && let Some(mut p) = rfd::FileDialog::new()
            .set_title("Create vault")
            .set_file_name(DEFAULT_VAULT_FILENAME)
            .save_file()
    {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.ends_with(&format!(".{VAULT_EXTENSION}")) {
            p.set_file_name(format!("{name}.{VAULT_EXTENSION}"));
        }
        form.path = Some(p);
    }
    ui.label(
        RichText::new("Its file name, size and modification times stay visible to anyone who can see the file. The neutral default name reveals nothing about its contents.")
            .size(11.5)
            .color(theme::MUTED),
    );
    ui.add_space(10.0);

    passphrase_field(
        ui,
        "create-pass",
        &mut form.pass,
        form.reveal,
        "Passphrase",
        false,
    );
    ui.add_space(4.0);
    passphrase_field(
        ui,
        "create-confirm",
        &mut form.confirm,
        form.reveal,
        "Repeat passphrase",
        false,
    );
    ui.horizontal(|ui| {
        ui.checkbox(&mut form.reveal, RichText::new("Show").size(12.5).color(theme::MUTED));
        if ui
            .button(RichText::new(format!("{} Suggest a passphrase", icon::DICE_FIVE)).size(12.5))
            .on_hover_text("Seven random words from the EFF wordlist (~90 bits). Shown so you can write it down.")
            .clicked()
        {
            let p = strength::suggest_passphrase(7);
            form.pass = Zeroizing::new(p.reveal().to_owned());
            form.confirm = Zeroizing::new(p.reveal().to_owned());
            form.reveal = true;
        }
    });

    // Strength guidance (local).
    if !form.pass.is_empty() {
        let est = strength::estimate(&Passphrase::new(form.pass.to_string()));
        strength_meter(ui, &est);
        if !form.confirm.is_empty() && form.pass.as_str() != form.confirm.as_str() {
            ui.label(
                RichText::new(format!("{} The entries do not match.", icon::X_CIRCLE))
                    .color(theme::AMBER)
                    .size(12.5),
            );
        }
    }
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(theme::PANEL_HI)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.label(
                RichText::new(
                    "Anyone who gets a copy of the vault file can try to guess the passphrase offline, as fast as their hardware allows. \
                     A long passphrase of random words is the defence. There is no reset: forget it and the data is gone.",
                )
                .size(12.0)
                .color(theme::TEXT),
            );
        });
    ui.checkbox(
        &mut form.ack_no_reset,
        RichText::new(
            "I understand there is no password reset and I have written my passphrase down safely.",
        )
        .size(12.5)
        .color(theme::TEXT),
    );
    if let Some(code) = form.error {
        error_line(ui, code);
    }
    ui.add_space(8.0);
    let ready = form.path.is_some()
        && !form.pass.is_empty()
        && form.pass.as_str() == form.confirm.as_str()
        && form.ack_no_reset;
    let w = ui.available_width();
    let mut go = false;
    if ui
        .add_enabled(
            ready,
            egui::Button::new(
                RichText::new(format!("{}  Create encrypted vault", icon::SHIELD_CHECK))
                    .color(theme::GREEN)
                    .font(theme::ui_bold(14.0)),
            )
            .fill(Color32::from_rgb(0x0F, 0x2A, 0x1D))
            .stroke(Stroke::new(1.0, theme::GREEN))
            .min_size(egui::vec2(w, 38.0)),
        )
        .clicked()
    {
        go = true;
    }
    let mut back = false;
    if ui.link(RichText::new("Back").size(12.5)).clicked()
        || ui.input(|i| i.key_pressed(egui::Key::Escape))
    {
        back = true;
    }
    if go {
        app.start_create();
    } else if back {
        app.screen = Screen::Welcome;
    }
}

pub fn strength_meter(ui: &mut egui::Ui, est: &strength::Estimate) {
    use strength::Rating::*;
    let (frac, color) = match est.rating {
        VeryWeak => (0.12, theme::RED),
        Weak => (0.3, theme::RED),
        Fair => (0.55, theme::AMBER),
        Strong => (0.8, theme::GREEN),
        Excellent => (1.0, theme::GREEN),
    };
    ui.add_space(4.0);
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 3.0, theme::PANEL_HI);
    let mut fill = r;
    fill.set_width(r.width() * frac);
    ui.painter().rect_filled(fill, 3.0, color);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(est.rating.label().to_uppercase())
                .font(theme::mono_bold(11.0))
                .color(color),
        );
        ui.label(
            RichText::new(format!(
                "~{:.0} bits · rough offline guess time: {} (local estimate, not a guarantee)",
                est.bits,
                est.offline_time_label()
            ))
            .size(11.5)
            .color(theme::MUTED),
        );
    });
    for h in &est.hints {
        if *h == strength::Hint::WordBased {
            continue;
        }
        ui.label(
            RichText::new(format!("· {}", h.text()))
                .size(11.5)
                .color(theme::MUTED),
        );
    }
}
