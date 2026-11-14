//! The unlocked workspace.

use crate::icons as icon;
use egui::{Color32, RichText, Sense, Stroke};
use vault_core::history::{self, DiffKind};
use vault_core::ids::{DocId, FolderId};
use vault_core::lifecycle::LockState;
use vault_core::model::{DocMode, HistoryKind};
use vault_core::search::{self, Query};
use vault_core::textops;
use vault_core::time::format_utc;
use zeroize::Zeroizing;

use crate::app::{Filter, Modal, NilavarApp, RightPane, SaveStatus, ToastKind};
use crate::commands::{Command, platform_shortcut};
use crate::editor::{self, EditorOptions};
use crate::preview::{self, PreviewAction};
use crate::theme;

const NARROW: f32 = 900.0;

pub fn show(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let width = ui.max_rect().width();
    let narrow = width < NARROW;
    let very_narrow = width < 640.0;

    top_bar(app, ui);
    status_bar(app, ui);

    if app.session_away {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Spinner::new().color(theme::GREEN).size(22.0));
                });
            });
        return;
    }

    let show_sidebar = app.ws.sidebar_open && !app.ws.focus_mode && !very_narrow;
    if show_sidebar {
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(260.0)
            .size_range(200.0..=420.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 10))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| sidebar(app, ui));
    }

    let show_right =
        app.ws.right != RightPane::None && !app.ws.focus_mode && !narrow && app.ws.active.is_some();
    if show_right {
        egui::Panel::right("right-pane")
            .resizable(true)
            .default_size((width * 0.34).clamp(280.0, 520.0))
            .size_range(240.0..=760.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| right_pane(app, ui));
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG))
        .show(ui, |ui| center(app, ui, narrow));

    palette(app, ui.ctx());
    toasts(app, ui.ctx());
}

// ---------------------------------------------------------------------------

fn top_bar(app: &mut NilavarApp, ui: &mut egui::Ui) {
    egui::Panel::top("top-bar")
        .exact_size(40.0)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let side = if app.ws.sidebar_open {
                    icon::SIDEBAR_SIMPLE
                } else {
                    icon::SIDEBAR
                };
                if icon_button(ui, side, "Toggle sidebar (⌘\\)").clicked() {
                    app.ws.sidebar_open = !app.ws.sidebar_open;
                }
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                theme::glow_dot(ui.painter(), r.center(), theme::GREEN);
                ui.label(
                    RichText::new("NILAVAR")
                        .font(theme::mono_bold(13.0))
                        .color(theme::GREEN)
                        .extra_letter_spacing(3.0),
                );
                if let Some(s) = app.session.as_ref() {
                    let name = s
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.label(RichText::new("/").color(theme::BORDER));
                    ui.label(
                        RichText::new(name)
                            .font(theme::mono(12.0))
                            .color(theme::MUTED),
                    )
                    .on_hover_text(s.path().display().to_string());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let lock = ui
                        .add(
                            egui::Button::new(
                                RichText::new(format!("{}  Lock", icon::LOCK))
                                    .color(theme::GREEN)
                                    .size(13.0),
                            )
                            .fill(theme::PANEL_HI)
                            .stroke(Stroke::new(1.0, theme::BORDER)),
                        )
                        .on_hover_text(platform_shortcut("Lock now (⇧⌘L)"));
                    if lock.clicked() {
                        app.request_lock(vault_core::diagnostics::LockReason::Manual);
                    }
                    if icon_button(ui, icon::GEAR_SIX, "Settings (⌘,)").clicked() {
                        app.modal = Some(Modal::Settings);
                    }
                    if icon_button(
                        ui,
                        icon::COMMAND,
                        &platform_shortcut("Command palette (⇧⌘P)"),
                    )
                    .clicked()
                    {
                        app.run_command(Command::CommandPalette, ui.ctx());
                    }
                    if app.ws.active.is_some() {
                        ui.add_space(6.0);
                        let panes = [
                            (RightPane::Info, icon::INFO, "Document info & tags"),
                            (
                                RightPane::History,
                                icon::CLOCK_COUNTER_CLOCKWISE,
                                "Version history",
                            ),
                            (RightPane::Preview, icon::EYE, "Markdown preview (⌘E)"),
                        ];
                        for (pane, ic, tip) in panes {
                            let on = app.ws.right == pane;
                            let b = egui::Button::new(RichText::new(ic).size(16.0).color(if on {
                                theme::GREEN
                            } else {
                                theme::MUTED
                            }))
                            .frame_when_inactive(on)
                            .fill(theme::GREEN_DIM);
                            if ui.add(b).on_hover_text(platform_shortcut(tip)).clicked() {
                                app.ws.focus_mode = false;
                                app.ws.right = if on { RightPane::None } else { pane };
                            }
                        }
                        let fm = app.ws.focus_mode;
                        if icon_button(
                            ui,
                            if fm {
                                icon::ARROWS_IN
                            } else {
                                icon::ARROWS_OUT
                            },
                            &platform_shortcut("Focus mode (⇧⌘\\)"),
                        )
                        .clicked()
                        {
                            app.ws.focus_mode = !fm;
                        }
                    }
                });
            });
        });
}

fn icon_button(ui: &mut egui::Ui, ic: &str, tip: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(ic).size(16.0).color(theme::MUTED)).frame(false))
        .on_hover_text(tip)
}

// ---------------------------------------------------------------------------

fn status_bar(app: &mut NilavarApp, ui: &mut egui::Ui) {
    egui::Panel::bottom("status-bar")
        .exact_size(28.0)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(12, 3))
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(ui, |ui| {
            let bar_w = ui.available_width();
            ui.horizontal_centered(|ui| {
                let small = |t: String, c: Color32| RichText::new(t).font(theme::mono(11.0)).color(c);
                // Save state — always a real state.
                let (dot, text, tip) = match app.save_status {
                    SaveStatus::Clean => (theme::GREEN, "SAVED".to_owned(), "All changes are encrypted and written to the vault file."),
                    SaveStatus::Dirty => (theme::AMBER, "UNSAVED".to_owned(), "Edits are in memory only. Autosave or ⌘S writes them encrypted."),
                    SaveStatus::Saving => (theme::CYAN, "SAVING…".to_owned(), "Encrypting and writing atomically."),
                    SaveStatus::Failed(c) => (theme::RED, format!("SAVE FAILED {}", c.id()), c.message()),
                };
                let (r, resp) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                theme::glow_dot(ui.painter(), r.center(), dot);
                resp.on_hover_text(tip);
                let lbl = ui.label(small(text, dot)).on_hover_text(tip);
                if let SaveStatus::Failed(_) = app.save_status
                    && lbl.clicked()
                {
                    app.run_command(Command::Save, ui.ctx());
                }
                if matches!(app.save_status, SaveStatus::Failed(_))
                    && ui.link(small("retry".into(), theme::CYAN)).clicked()
                {
                    app.run_command(Command::Save, ui.ctx());
                }
                ui.label(small("│".into(), theme::BORDER));
                if app.ws.active.is_some() {
                    let (l, c) = app.ws.editor.cursor;
                    ui.label(small(format!("Ln {l}, Col {c}"), theme::MUTED));
                    if app.ws.editor.selection_chars > 0 {
                        ui.label(small(format!("({} selected)", app.ws.editor.selection_chars), theme::MUTED));
                    }
                    let info = app
                        .ws
                        .active
                        .and_then(|id| app.session.as_ref().and_then(|s| s.vault().document(id)))
                        .map(|d| (d.text.char_count(), d.text.expose().split_whitespace().count(), d.mode, d.text.len()));
                    if let Some((chars, words, dmode, bytes)) = info.filter(|_| bar_w >= 760.0) {
                        ui.label(small(format!("{chars} chars · {words} words"), theme::MUTED));
                        let mode = match dmode {
                            DocMode::Markdown => "Markdown",
                            DocMode::Plain => "Plain text",
                        };
                        if ui
                            .add(egui::Label::new(small(mode.into(), theme::MUTED)).sense(Sense::click()))
                            .on_hover_text("Switch Markdown / plain text")
                            .clicked()
                        {
                            app.run_command(Command::ToggleDocMode, ui.ctx());
                        }
                        if bytes > vault_core::limits::MAX_DOCUMENT_BYTES * 9 / 10 {
                            ui.label(small(
                                format!("{} near 2 MiB limit", icon::WARNING),
                                theme::AMBER,
                            ));
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(small(format!("{} no network features", icon::WIFI_SLASH), theme::MUTED))
                        .on_hover_text(if app.caps.os_network_sandbox {
                            "This app implements no networking, and the macOS no-network sandbox makes the kernel deny all sockets for this process."
                        } else {
                            "This app implements no networking. No OS sandbox is enforcing that in this build — see Security & capabilities."
                        });
                    if bar_w >= 560.0 {
                    ui.label(small("│".into(), theme::BORDER));
                    ui.label(small(format!("{} age · scrypt", icon::LOCK_SIMPLE), theme::MUTED))
                        .on_hover_text("Vault encrypted at rest with age passphrase mode (scrypt).");
                    }
                    if let Some(s) = app.session.as_ref() {
                        let idle = std::time::Duration::from_secs(u64::from(s.vault().prefs.idle_lock_secs));
                        let left = idle.saturating_sub(app.last_activity.elapsed());
                        if left.as_secs() <= 60 {
                            ui.label(small("│".into(), theme::BORDER));
                            ui.label(small(format!("auto-lock in {}s", left.as_secs()), theme::AMBER));
                        }
                    }
                    if app.state == LockState::Saving {
                        ui.add(egui::Spinner::new().size(11.0).color(theme::CYAN));
                    }
                });
            });
        });
}

// ---------------------------------------------------------------------------

fn sidebar(app: &mut NilavarApp, ui: &mut egui::Ui) {
    // Search.
    let search_id = egui::Id::unique("vault-search");
    ui.horizontal(|ui| {
        let r = ui.add_sized(
            [ui.available_width(), 30.0],
            egui::TextEdit::singleline(&mut *app.ws.search)
                .id(search_id)
                .hint_text(format!(
                    "{} Search vault   tag:name",
                    icon::MAGNIFYING_GLASS
                ))
                .margin(egui::Margin::symmetric(8, 6)),
        );
        if std::mem::take(&mut app.ws.search_focus) {
            r.request_focus();
        }
        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.ws.search.clear();
            r.surrender_focus();
        }
    });
    ui.add_space(6.0);

    if !app.ws.search.trim().is_empty() {
        search_results(app, ui);
        return;
    }

    // Views.
    ui.horizontal(|ui| {
        let all = matches!(app.ws.filter, Filter::All);
        if nav_item(ui, icon::FILES, "All documents", all).clicked() {
            app.ws.filter = Filter::All;
        }
    });
    let trash_n = app
        .session
        .as_ref()
        .map(|s| s.vault().trash.len())
        .unwrap_or(0);
    if nav_item(
        ui,
        icon::TRASH,
        &format!("Trash ({trash_n})"),
        matches!(app.ws.filter, Filter::Trash),
    )
    .clicked()
    {
        app.ws.filter = Filter::Trash;
    }
    ui.add_space(4.0);
    ui.separator();

    if matches!(app.ws.filter, Filter::Trash) {
        trash_view(app, ui);
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 110.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::section_label(ui, "DOCUMENTS");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(icon::FOLDER_SIMPLE_PLUS).color(theme::MUTED),
                            )
                            .frame(false),
                        )
                        .on_hover_text("New folder")
                        .clicked()
                    {
                        app.run_command(Command::NewFolder, ui.ctx());
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new(icon::FILE_PLUS).color(theme::MUTED))
                                .frame(false),
                        )
                        .on_hover_text(platform_shortcut("New document (⌘N)"))
                        .clicked()
                    {
                        app.run_command(Command::NewDocument, ui.ctx());
                    }
                });
            });
            match app.ws.filter.clone() {
                Filter::Tag(t) => {
                    let ids = doc_ids_sorted(app, |d| d.has_tag(&t));
                    for id in ids {
                        doc_row(app, ui, id, 0);
                    }
                }
                _ => folder_tree(app, ui, None, 0),
            }
            let empty = app
                .session
                .as_ref()
                .is_some_and(|s| s.vault().documents.is_empty());
            if empty {
                ui.add_space(10.0);
                ui.label(RichText::new("No documents yet.").color(theme::MUTED));
                if ui
                    .add(theme::secondary_button(format!(
                        "{} New document",
                        icon::FILE_PLUS
                    )))
                    .clicked()
                {
                    app.run_command(Command::NewDocument, ui.ctx());
                }
                if ui
                    .add(theme::secondary_button(format!(
                        "{} Import text files…",
                        icon::DOWNLOAD_SIMPLE
                    )))
                    .clicked()
                {
                    app.run_command(Command::ImportFiles, ui.ctx());
                }
            }
        });

    // Tags.
    let tags: Vec<String> = app
        .session
        .as_ref()
        .map(|s| {
            s.vault()
                .all_tags()
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    ui.separator();
    theme::section_label(ui, "TAGS");
    if tags.is_empty() {
        ui.label(
            RichText::new("Add tags from the info pane.")
                .size(11.5)
                .color(theme::MUTED),
        );
    }
    egui::ScrollArea::vertical()
        .id_salt("tags")
        .max_height(80.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for t in tags {
                    let on = matches!(&app.ws.filter, Filter::Tag(x) if x.as_str() == t);
                    let chip =
                        egui::Button::new(RichText::new(format!("#{t}")).size(12.0).color(if on {
                            theme::GREEN
                        } else {
                            theme::MUTED
                        }))
                        .fill(if on {
                            theme::GREEN_DIM
                        } else {
                            theme::PANEL_HI
                        })
                        .stroke(Stroke::new(
                            1.0,
                            if on { theme::GREEN } else { theme::BORDER },
                        ))
                        .corner_radius(10);
                    if ui.add(chip).clicked() {
                        app.ws.filter = if on {
                            Filter::All
                        } else {
                            Filter::Tag(Zeroizing::new(t))
                        };
                    }
                }
            });
        });
}

fn nav_item(ui: &mut egui::Ui, ic: &str, label: &str, on: bool) -> egui::Response {
    theme::row_button(ui, format!("{ic}   {label}"), on, 0.0)
}

fn doc_ids_sorted(
    app: &NilavarApp,
    pred: impl Fn(&vault_core::model::Document) -> bool,
) -> Vec<DocId> {
    let Some(s) = app.session.as_ref() else {
        return Vec::new();
    };
    let mut v: Vec<&vault_core::model::Document> =
        s.vault().documents.iter().filter(|d| pred(d)).collect();
    v.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.updated_at.cmp(&a.updated_at))
    });
    v.into_iter().map(|d| d.id).collect()
}

fn folder_tree(app: &mut NilavarApp, ui: &mut egui::Ui, parent: Option<FolderId>, depth: usize) {
    let folders: Vec<(FolderId, String, usize)> = match app.session.as_ref() {
        Some(s) => s
            .vault()
            .child_folders(parent)
            .into_iter()
            .map(|f| {
                let n = s
                    .vault()
                    .documents
                    .iter()
                    .filter(|d| d.folder == Some(f.id))
                    .count();
                (f.id, f.name.expose().to_owned(), n)
            })
            .collect(),
        None => return,
    };
    for (fid, name, count) in folders {
        let expanded = app.ws.expanded.contains(&fid);
        let selected = matches!(app.ws.filter, Filter::Folder(f) if f == fid);
        let caret = if expanded {
            icon::CARET_DOWN
        } else {
            icon::CARET_RIGHT
        };
        let ic = if expanded {
            icon::FOLDER_OPEN
        } else {
            icon::FOLDER
        };
        let resp = theme::row_button(
            ui,
            format!("{caret}  {ic}   {name}   {count}"),
            selected,
            depth as f32 * 14.0,
        );
        if resp.clicked() {
            if selected && expanded {
                app.ws.expanded.remove(&fid);
            } else {
                app.ws.filter = Filter::Folder(fid);
                app.ws.expanded.insert(fid);
            }
        }
        resp.context_menu(|ui| {
            if ui
                .button(format!("{} New document here", icon::FILE_PLUS))
                .clicked()
            {
                app.ws.filter = Filter::Folder(fid);
                app.run_command(Command::NewDocument, ui.ctx());
                ui.close();
            }
            if ui
                .button(format!("{} New subfolder", icon::FOLDER_SIMPLE_PLUS))
                .clicked()
            {
                app.modal = Some(Modal::NewFolder {
                    parent: Some(fid),
                    name: Zeroizing::new(String::new()),
                });
                ui.close();
            }
            if ui
                .button(format!("{} Rename folder", icon::PENCIL_SIMPLE))
                .clicked()
            {
                app.modal = Some(Modal::RenameFolder {
                    folder: fid,
                    name: Zeroizing::new(name.clone()),
                });
                ui.close();
            }
            if ui
                .button(RichText::new(format!("{} Delete folder", icon::TRASH)).color(theme::RED))
                .clicked()
            {
                let r = app
                    .session
                    .as_mut()
                    .map(|s| s.vault_mut().delete_folder(fid));
                match r {
                    Some(Ok(())) => {
                        app.mark_changed();
                        app.ws.filter = Filter::All;
                    }
                    Some(Err(e)) => app.error_toast(e),
                    None => {}
                }
                ui.close();
            }
        });
        if expanded {
            folder_tree(app, ui, Some(fid), depth + 1);
            for id in doc_ids_sorted(app, |d| d.folder == Some(fid)) {
                doc_row(app, ui, id, depth + 1);
            }
        }
    }
    if parent.is_none() {
        for id in doc_ids_sorted(app, |d| d.folder.is_none()) {
            doc_row(app, ui, id, 0);
        }
    }
}

fn doc_row(app: &mut NilavarApp, ui: &mut egui::Ui, id: DocId, depth: usize) {
    let Some((title, pinned, mode)) = app
        .session
        .as_ref()
        .and_then(|s| s.vault().document(id))
        .map(|d| (display_title(d.title.expose()), d.pinned, d.mode))
    else {
        return;
    };
    let active = app.ws.active == Some(id);
    let dirty = app.doc_is_dirty(id);
    let ic = match (pinned, mode) {
        (true, _) => icon::PUSH_PIN,
        (_, DocMode::Markdown) => icon::FILE_TEXT,
        (_, DocMode::Plain) => icon::FILE,
    };
    let mut text = format!("{ic}   {title}");
    if dirty {
        text.push_str("  •");
    }
    let resp = theme::row_button(ui, text, active, depth as f32 * 14.0 + 18.0);
    if resp.clicked() {
        app.open_doc(id);
    }
    resp.context_menu(|ui| doc_menu(app, ui, id));
}

fn display_title(t: &str) -> String {
    if t.trim().is_empty() {
        "Untitled".to_owned()
    } else {
        t.to_owned()
    }
}

fn doc_menu(app: &mut NilavarApp, ui: &mut egui::Ui, id: DocId) {
    let ctx = ui.ctx().clone();
    // Actions apply to this document: make it the active tab first.
    let run = |app: &mut NilavarApp, cmd: Command| {
        app.open_doc(id);
        app.run_command(cmd, &ctx);
    };
    if ui.button(format!("{} Open", icon::FILE_TEXT)).clicked() {
        app.open_doc(id);
        ui.close();
    }
    if ui
        .button(format!("{} Rename", icon::PENCIL_SIMPLE))
        .clicked()
    {
        run(app, Command::RenameDocument);
        ui.close();
    }
    if ui
        .button(format!("{} Pin / unpin", icon::PUSH_PIN))
        .clicked()
    {
        run(app, Command::TogglePin);
        ui.close();
    }
    ui.menu_button(format!("{} Move to", icon::FOLDER), |ui| {
        let folders: Vec<(Option<FolderId>, String)> = app
            .session
            .as_ref()
            .map(|s| {
                let mut v = vec![(None, "(vault root)".to_owned())];
                for f in &s.vault().folders {
                    let path: Vec<&str> = s
                        .vault()
                        .folder_path(f.id)
                        .iter()
                        .map(|n| n.expose())
                        .collect();
                    v.push((Some(f.id), path.join(" / ")));
                }
                v
            })
            .unwrap_or_default();
        for (fid, label) in folders {
            if ui.button(label).clicked() {
                let r = app
                    .session
                    .as_mut()
                    .map(|s| s.vault_mut().move_document(id, fid));
                if let Some(Err(e)) = r {
                    app.error_toast(e);
                } else {
                    app.mark_changed();
                }
                ui.close();
            }
        }
    });
    if ui
        .button(format!("{} Copy text (auto-clears)", icon::COPY))
        .clicked()
    {
        run(app, Command::CopyDocument);
        ui.close();
    }
    ui.separator();
    if ui
        .button(format!("{} Export encrypted (.age)…", icon::EXPORT))
        .clicked()
    {
        run(app, Command::ExportEncrypted);
        ui.close();
    }
    if ui
        .button(
            RichText::new(format!("{} Export as plaintext…", icon::WARNING)).color(theme::AMBER),
        )
        .clicked()
    {
        run(app, Command::ExportPlaintext);
        ui.close();
    }
    ui.separator();
    if ui
        .button(RichText::new(format!("{} Move to trash", icon::TRASH)).color(theme::RED))
        .clicked()
    {
        app.modal = Some(Modal::ConfirmTrash(id));
        ui.close();
    }
}

fn search_results(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let key = editor::keyed_hash(&app.ws.editor, app.ws.search.as_str());
    if app.ws.search_key != Some(key) {
        let q = Query::parse(&app.ws.search);
        app.ws.search_hits = app
            .session
            .as_ref()
            .map(|s| search::search_vault(s.vault(), &q, 200))
            .unwrap_or_default();
        app.ws.search_key = Some(key);
    }
    let n = app.ws.search_hits.len();
    theme::section_label(ui, &format!("{n} RESULT{}", if n == 1 { "" } else { "S" }));
    if n == 0 {
        ui.label(RichText::new("No matches in titles, tags or text.").color(theme::MUTED));
        return;
    }
    let mut open: Option<(DocId, Option<usize>)> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for h in &app.ws.search_hits {
                let title = app
                    .session
                    .as_ref()
                    .and_then(|s| s.vault().document(h.doc))
                    .map(|d| display_title(d.title.expose()))
                    .unwrap_or_default();
                let resp = egui::Frame::new()
                    .fill(theme::PANEL_HI)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(5)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon::FILE_TEXT).color(theme::GREEN));
                            ui.add(
                                egui::Label::new(RichText::new(title).color(theme::TEXT))
                                    .truncate(),
                            );
                            if h.body_matches > 0 {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(h.body_matches.to_string())
                                                .size(11.0)
                                                .color(theme::MUTED),
                                        );
                                    },
                                );
                            }
                        });
                        if !h.snippet.is_empty() {
                            let mut job = egui::text::LayoutJob::default();
                            let s = h.snippet.as_str();
                            let m = h.snippet_match.clone();
                            let f = theme::mono(11.5);
                            job.append(
                                &s[..m.start],
                                0.0,
                                egui::TextFormat::simple(f.clone(), theme::MUTED),
                            );
                            let mut hf = egui::TextFormat::simple(f.clone(), theme::AMBER);
                            hf.background = Color32::from_rgb(0x2E, 0x26, 0x10);
                            job.append(&s[m.clone()], 0.0, hf);
                            job.append(&s[m.end..], 0.0, egui::TextFormat::simple(f, theme::MUTED));
                            job.wrap.max_width = ui.available_width();
                            ui.label(job);
                        }
                    })
                    .response
                    .interact(Sense::click());
                if resp.clicked() {
                    open = Some((h.doc, h.first_offset));
                }
                ui.add_space(4.0);
            }
        });
    if let Some((doc, off)) = open {
        app.open_doc(doc);
        let q = Query::parse(&app.ws.search);
        if let Some(off) = off
            && let Some(text) = app
                .session
                .as_ref()
                .and_then(|s| s.vault().document(doc))
                .map(|d| d.text.expose().to_owned())
        {
            let a = textops::byte_to_char(&text, off);
            let len = q.text.chars().count();
            app.ws.editor.pending_selection = Some((a, a + len));
            app.ws.editor.find.open = true;
            app.ws.editor.find.query = Zeroizing::new(q.text.clone());
            app.ws.editor.find.invalidate();
            drop(Zeroizing::new(text));
        }
    }
}

fn trash_view(app: &mut NilavarApp, ui: &mut egui::Ui) {
    theme::section_label(ui, "TRASH");
    ui.label(
        RichText::new("Deleted documents stay encrypted in the vault until purged. Old copies and backups may still contain them.")
            .size(11.5)
            .color(theme::MUTED),
    );
    let items: Vec<(DocId, String, i64)> = app
        .session
        .as_ref()
        .map(|s| {
            let mut v: Vec<_> = s
                .vault()
                .trash
                .iter()
                .map(|t| {
                    (
                        t.document.id,
                        display_title(t.document.title.expose()),
                        t.deleted_at,
                    )
                })
                .collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.2));
            v
        })
        .unwrap_or_default();
    if items.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("Trash is empty.").color(theme::MUTED));
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 40.0)
        .show(ui, |ui| {
            for (id, title, at) in items {
                egui::Frame::new()
                    .fill(theme::PANEL_HI)
                    .corner_radius(5)
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&title).color(theme::TEXT));
                        ui.label(
                            RichText::new(format!("deleted {}", format_utc(at)))
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .small_button(format!("{} Restore", icon::ARROW_COUNTER_CLOCKWISE))
                                .clicked()
                            {
                                let r = app
                                    .session
                                    .as_mut()
                                    .map(|s| s.vault_mut().restore_from_trash(id));
                                match r {
                                    Some(Ok(())) => {
                                        app.mark_changed();
                                        app.ws.filter = Filter::All;
                                        app.open_doc(id);
                                    }
                                    Some(Err(e)) => app.error_toast(e),
                                    None => {}
                                }
                            }
                            if ui
                                .small_button(
                                    RichText::new(format!("{} Delete forever", icon::X))
                                        .color(theme::RED),
                                )
                                .clicked()
                            {
                                app.modal = Some(Modal::ConfirmPurge(id));
                            }
                        });
                    });
                ui.add_space(4.0);
            }
        });
    if ui
        .add(theme::danger_button(format!("{} Empty trash", icon::TRASH)))
        .clicked()
    {
        app.modal = Some(Modal::ConfirmEmptyTrash);
    }
}

// ---------------------------------------------------------------------------

fn center(app: &mut NilavarApp, ui: &mut egui::Ui, narrow: bool) {
    if !app.ws.focus_mode {
        tabs_bar(app, ui);
    }
    let Some(id) = app.ws.active else {
        empty_state(app, ui);
        return;
    };
    if app
        .session
        .as_ref()
        .and_then(|s| s.vault().document(id))
        .is_none()
    {
        app.ws.tabs.retain(|d| *d != id);
        app.ws.active = app.ws.tabs.first().copied();
        return;
    }

    // Narrow layout: right pane content replaces the editor when toggled.
    if narrow && app.ws.right != RightPane::None && !app.ws.focus_mode {
        egui::Frame::new().inner_margin(12).show(ui, |ui| {
            if ui
                .button(format!("{} Back to editor", icon::ARROW_LEFT))
                .clicked()
            {
                app.ws.right = RightPane::None;
            }
            right_pane(app, ui);
        });
        return;
    }

    title_row(app, ui, id);
    if app.ws.editor.find.open {
        find_bar(app, ui, id);
    }

    let opts = EditorOptions {
        font_size: app.settings.editor_font_size,
        soft_wrap: app.settings.soft_wrap,
        line_numbers: app.settings.line_numbers,
        highlight_line: app.settings.highlight_current_line,
        mode: app
            .session
            .as_ref()
            .and_then(|s| s.vault().document(id))
            .map(|d| d.mode)
            .unwrap_or_default(),
    };
    let max_w = if app.ws.focus_mode {
        860.0
    } else {
        f32::INFINITY
    };
    let mut changed = false;
    let avail = ui.available_width();
    let pad = ((avail - max_w) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_max_width(avail - pad * 2.0);
            let Some(s) = app.session.as_mut() else {
                return;
            };
            let Some(d) = s.vault_mut().document_mut(id) else {
                return;
            };
            let out = editor::show(ui, id, &mut d.text, &mut app.ws.editor, &opts);
            changed = out.changed;
        });
    });
    if changed {
        app.mark_edited(id);
        app.ws.preview = None;
    }
}

fn empty_state(app: &mut NilavarApp, ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.label(
                RichText::new(icon::TERMINAL_WINDOW)
                    .size(44.0)
                    .color(theme::GREEN.gamma_multiply(0.6)),
            );
            ui.label(
                RichText::new("No document open")
                    .font(theme::ui_bold(18.0))
                    .color(theme::TEXT),
            );
            ui.label(
                RichText::new("Pick one in the sidebar, or start something new.")
                    .color(theme::MUTED),
            );
            ui.add_space(10.0);
            for (label, cmd) in [
                ("New document", Command::NewDocument),
                ("Go to document…", Command::QuickSwitch),
                ("Import text files…", Command::ImportFiles),
            ] {
                let sc = platform_shortcut(cmd.shortcut());
                if ui
                    .add(
                        theme::secondary_button(format!("{label}   {sc}"))
                            .min_size(egui::vec2(240.0, 32.0)),
                    )
                    .clicked()
                {
                    app.run_command(cmd, ui.ctx());
                }
            }
        });
    });
}

fn tabs_bar(app: &mut NilavarApp, ui: &mut egui::Ui) {
    if app.ws.tabs.is_empty() {
        return;
    }
    let mut to_open = None;
    let mut to_close = None;
    egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 0 })
        .stroke(Stroke::new(1.0, theme::BORDER))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::horizontal().id_salt("tabs").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for id in app.ws.tabs.clone() {
                        let Some(title) = app
                            .session
                            .as_ref()
                            .and_then(|s| s.vault().document(id))
                            .map(|d| display_title(d.title.expose()))
                        else {
                            continue;
                        };
                        let active = app.ws.active == Some(id);
                        let dirty = app.doc_is_dirty(id);
                        let short: String = if title.chars().count() > 24 {
                            format!("{}…", title.chars().take(23).collect::<String>())
                        } else {
                            title.clone()
                        };
                        let frame = egui::Frame::new()
                            .fill(if active { theme::BG } else { theme::PANEL })
                            .stroke(Stroke::new(1.0, if active { theme::BORDER } else { Color32::TRANSPARENT }))
                            .corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 })
                            .inner_margin(egui::Margin::symmetric(10, 6));
                        let r = frame
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if active {
                                        let (r, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), Sense::hover());
                                        ui.painter().circle_filled(r.center(), 2.5, theme::GREEN);
                                    }
                                    let l = ui.add(
                                        egui::Label::new(RichText::new(short).size(13.0).color(if active { theme::TEXT } else { theme::MUTED }))
                                            .selectable(false)
                                            .sense(Sense::click()),
                                    );
                                    let close_txt = if dirty { "●" } else { icon::X };
                                    let c = ui
                                        .add(egui::Button::new(RichText::new(close_txt).size(11.0).color(if dirty { theme::AMBER } else { theme::MUTED })).frame(false))
                                        .on_hover_text(if dirty { "Unsaved changes — close tab (content stays in the vault)" } else { "Close tab" });
                                    if c.clicked() {
                                        to_close = Some(id);
                                    }
                                    l
                                })
                                .inner
                            })
                            .inner;
                        let r = r.on_hover_text(title);
                        if r.clicked() {
                            to_open = Some(id);
                        }
                        if r.middle_clicked() {
                            to_close = Some(id);
                        }
                        if active && r.rect.is_positive() {
                            let y = r.rect.bottom() + 6.0;
                            ui.painter().hline(
                                (r.rect.left() - 10.0)..=(r.rect.right() + 30.0),
                                y,
                                Stroke::new(2.0, theme::GREEN.gamma_multiply(0.85)),
                            );
                        }
                    }
                });
            });
        });
    if let Some(id) = to_close {
        app.close_tab(id);
    } else if let Some(id) = to_open {
        app.open_doc(id);
    }
}

fn title_row(app: &mut NilavarApp, ui: &mut egui::Ui, id: DocId) {
    let Some((title, folder_path, updated)) = app.session.as_ref().and_then(|s| {
        let d = s.vault().document(id)?;
        let fp = d
            .folder
            .map(|f| {
                s.vault()
                    .folder_path(f)
                    .iter()
                    .map(|n| n.expose().to_owned())
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        Some((display_title(d.title.expose()), fp, d.updated_at))
    }) else {
        return;
    };
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 18,
            right: 14,
            top: 10,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if !folder_path.is_empty() {
                    ui.label(
                        RichText::new(format!("{} {folder_path} /", icon::FOLDER))
                            .size(12.5)
                            .color(theme::MUTED),
                    );
                }
                let r = ui
                    .add(
                        egui::Label::new(
                            RichText::new(&title)
                                .font(theme::ui_bold(17.0))
                                .color(theme::TEXT),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_text("Double-click to rename");
                if r.double_clicked() {
                    app.run_command(Command::RenameDocument, ui.ctx());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("edited {}", format_utc(updated)))
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                });
            });
        });
}

fn find_bar(app: &mut NilavarApp, ui: &mut egui::Ui, id: DocId) {
    let mut close = false;
    let mut next = false;
    let mut prev = false;
    let mut replace_one = false;
    let mut replace_all = false;
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let f = &mut app.ws.editor.find;
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::MAGNIFYING_GLASS).color(theme::MUTED));
                let r = ui.add(
                    egui::TextEdit::singleline(&mut *f.query)
                        .id(egui::Id::unique("find-query"))
                        .hint_text("Find (literal)")
                        .desired_width(220.0),
                );
                if std::mem::take(&mut f.focus_query) {
                    r.request_focus();
                }
                if r.changed() {
                    f.invalidate();
                    f.current = 0;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if ui.input(|i| i.modifiers.shift) {
                        prev = true;
                    } else {
                        next = true;
                    }
                    r.request_focus();
                }
                let count = f.matches.len();
                let label = if f.query.is_empty() {
                    String::new()
                } else if count == 0 {
                    "no matches".into()
                } else {
                    format!(
                        "{} of {}{}",
                        f.current + 1,
                        count,
                        if count >= 5000 { "+" } else { "" }
                    )
                };
                ui.label(RichText::new(label).font(theme::mono(11.5)).color(
                    if count == 0 && !f.query.is_empty() {
                        theme::AMBER
                    } else {
                        theme::MUTED
                    },
                ));
                if ui
                    .small_button(icon::ARROW_UP)
                    .on_hover_text("Previous (⇧Enter)")
                    .clicked()
                {
                    prev = true;
                }
                if ui
                    .small_button(icon::ARROW_DOWN)
                    .on_hover_text("Next (Enter)")
                    .clicked()
                {
                    next = true;
                }
                if ui
                    .toggle_value(&mut f.case_sensitive, "Aa")
                    .on_hover_text("Match case")
                    .changed()
                {
                    f.invalidate();
                }
                ui.toggle_value(&mut f.show_replace, "Replace");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(icon::X).frame(false))
                        .on_hover_text("Close (Esc)")
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            if f.show_replace {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::SWAP).color(theme::MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut *f.replacement)
                            .hint_text("Replace with")
                            .desired_width(220.0),
                    );
                    if ui.button("Replace").clicked() {
                        replace_one = true;
                    }
                    if ui.button("Replace all").clicked() {
                        replace_all = true;
                    }
                });
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    if close {
        app.ws.editor.find.open = false;
        app.ws.editor.request_focus = true;
        return;
    }
    let Some(text) = app
        .session
        .as_ref()
        .and_then(|s| s.vault().document(id))
        .map(|d| d.text.duplicate())
    else {
        return;
    };
    if next || prev {
        editor::find_step(&mut app.ws.editor, text.expose(), next);
    }
    if replace_one || replace_all {
        let mut did = false;
        let mut n = 0;
        if let Some(d) = app
            .session
            .as_mut()
            .and_then(|s| s.vault_mut().document_mut(id))
        {
            if replace_all {
                n = editor::replace_all(&mut app.ws.editor, &mut d.text);
                did = n > 0;
            } else {
                did = editor::replace_current(&mut app.ws.editor, &mut d.text);
            }
        }
        if did {
            app.mark_edited(id);
            if replace_all {
                app.toast(
                    ToastKind::Info,
                    format!("Replaced {n} occurrence(s). Undo with ⌘Z."),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------

fn right_pane(app: &mut NilavarApp, ui: &mut egui::Ui) {
    let Some(id) = app.ws.active else { return };
    ui.horizontal(|ui| {
        for (pane, label) in [
            (RightPane::Preview, "Preview"),
            (RightPane::History, "History"),
            (RightPane::Info, "Info"),
        ] {
            let on = app.ws.right == pane;
            if ui
                .add(
                    egui::Button::new(RichText::new(label).size(13.0).color(if on {
                        theme::GREEN
                    } else {
                        theme::MUTED
                    }))
                    .frame_when_inactive(on)
                    .fill(theme::GREEN_DIM),
                )
                .clicked()
            {
                app.ws.right = pane;
            }
        }
    });
    ui.separator();
    match app.ws.right {
        RightPane::Preview => preview_pane(app, ui, id),
        RightPane::History => history_pane(app, ui, id),
        RightPane::Info => info_pane(app, ui, id),
        RightPane::None => {}
    }
}

fn preview_pane(app: &mut NilavarApp, ui: &mut egui::Ui, id: DocId) {
    let Some(d) = app.session.as_ref().and_then(|s| s.vault().document(id)) else {
        return;
    };
    if d.mode == DocMode::Plain {
        ui.label(RichText::new("Plain-text document — no Markdown preview.").color(theme::MUTED));
        return;
    }
    let key = editor::keyed_hash(&app.ws.editor, d.text.expose());
    if app.ws.preview.as_ref().map(|p| p.0) != Some(key) {
        app.ws.preview = Some((key, vault_core::markdown::parse(d.text.expose())));
    }
    ui.label(
        RichText::new(
            "Inert preview · no HTML, scripts or remote images · links open only on confirmation",
        )
        .size(10.5)
        .color(theme::MUTED),
    );
    let size = app.settings.editor_font_size;
    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some((_, p)) = app.ws.preview.as_ref() {
                action = preview::show(ui, p, size);
            }
        });
    match action {
        Some(PreviewAction::Link(dest)) => app.modal = Some(Modal::Link(dest)),
        Some(PreviewAction::ToggleTask(n)) => {
            let mut changed = false;
            if let Some(d) = app
                .session
                .as_mut()
                .and_then(|s| s.vault_mut().document_mut(id))
                && let Some(off) = preview::nth_task_offset(d.text.expose(), n)
            {
                let cur = &d.text.expose()[off..off + 3];
                let new = if cur == "[ ]" { "[x]" } else { "[ ]" };
                d.text.remove_range(off, off + 3);
                d.text.insert_str(off, new);
                changed = true;
            }
            if changed {
                app.mark_edited(id);
                app.ws.preview = None;
            }
        }
        None => {}
    }
}

fn history_pane(app: &mut NilavarApp, ui: &mut egui::Ui, id: DocId) {
    let Some(d) = app.session.as_ref().and_then(|s| s.vault().document(id)) else {
        return;
    };
    let entries: Vec<(vault_core::ids::HistoryId, i64, HistoryKind, usize)> = d
        .history
        .iter()
        .rev()
        .map(|h| (h.id, h.saved_at, h.kind, h.text.len()))
        .collect();
    ui.label(
        RichText::new(format!(
            "{} saved version(s), kept encrypted in the vault (up to {} per document; autosaves within 10 min are merged).",
            entries.len(),
            vault_core::limits::MAX_REVISIONS_PER_DOCUMENT
        ))
        .size(11.5)
        .color(theme::MUTED),
    );
    if entries.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new("No saved versions yet. Save (⌘S) to create one.").color(theme::MUTED),
        );
        return;
    }
    let sel = app
        .ws
        .history_sel
        .filter(|h| entries.iter().any(|e| e.0 == *h));
    egui::ScrollArea::vertical()
        .id_salt("history-list")
        .max_height((ui.available_height() * 0.38).max(120.0))
        .show(ui, |ui| {
            for (i, (hid, at, kind, len)) in entries.iter().enumerate() {
                let on = sel == Some(*hid);
                let kind_s = match kind {
                    HistoryKind::Auto => "autosave",
                    HistoryKind::Checkpoint => "saved",
                    HistoryKind::Restore => "restored",
                };
                let latest = if i == 0 { " · latest" } else { "" };
                let text =
                    RichText::new(format!("{}  {kind_s}{latest} · {len} B", format_utc(*at)))
                        .font(theme::mono(11.5));
                if theme::row_button(ui, text, on, 0.0).clicked() {
                    app.ws.history_sel = Some(*hid);
                }
            }
        });
    ui.separator();
    let Some(hid) = app.ws.history_sel else {
        ui.label(RichText::new("Select a version to preview or compare it.").color(theme::MUTED));
        return;
    };
    let Some(d) = app.session.as_ref().and_then(|s| s.vault().document(id)) else {
        return;
    };
    let Some(h) = d.history_entry(hid) else {
        return;
    };
    let mut restore = false;
    let mut copy = false;
    ui.horizontal(|ui| {
        ui.toggle_value(&mut app.ws.history_diff, "Compare with current");
        if ui
            .button(format!(
                "{} Restore as new version",
                icon::ARROW_COUNTER_CLOCKWISE
            ))
            .clicked()
        {
            restore = true;
        }
        if ui
            .button(icon::COPY)
            .on_hover_text("Copy this version (auto-clears)")
            .clicked()
        {
            copy = true;
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("history-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if app.ws.history_diff {
                let lines = history::diff_lines(&h.text, &d.text);
                let (a, r) = history::diff_stats(&lines);
                ui.label(
                    RichText::new(format!("+{a}  −{r} lines (this version → current)"))
                        .font(theme::mono(11.0))
                        .color(theme::MUTED),
                );
                for l in lines.iter().take(4000) {
                    let (prefix, color, bg) = match l.kind {
                        DiffKind::Same => (" ", theme::MUTED, Color32::TRANSPARENT),
                        DiffKind::Added => ("+", theme::GREEN, Color32::from_rgb(0x0C, 0x24, 0x18)),
                        DiffKind::Removed => ("−", theme::RED, Color32::from_rgb(0x26, 0x0E, 0x11)),
                    };
                    egui::Frame::new().fill(bg).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{prefix} {}", l.text.as_str()))
                                    .font(theme::mono(12.0))
                                    .color(color),
                            )
                            .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                    });
                }
            } else {
                ui.add(
                    egui::Label::new(
                        RichText::new(h.text.expose())
                            .font(theme::mono(12.5))
                            .color(theme::TEXT),
                    )
                    .wrap_mode(egui::TextWrapMode::Wrap),
                );
            }
        });
    if copy {
        let t = h.text.duplicate();
        app.copy_secret(t.expose());
    }
    if restore {
        let r = app
            .session
            .as_mut()
            .map(|s| s.vault_mut().restore_history_entry(id, hid));
        match r {
            Some(Ok(())) => {
                app.mark_edited(id);
                app.ws.preview = None;
                app.toast(ToastKind::Success, "Restored. The current text is now that version; the previous text stays in history.");
            }
            Some(Err(e)) => app.error_toast(e),
            None => {}
        }
    }
}

fn info_pane(app: &mut NilavarApp, ui: &mut egui::Ui, id: DocId) {
    let Some(d) = app.session.as_ref().and_then(|s| s.vault().document(id)) else {
        return;
    };
    let tags: Vec<String> = d.tags.iter().map(|t| t.expose().to_owned()).collect();
    let rows = [
        ("Created", format_utc(d.created_at)),
        ("Edited", format_utc(d.updated_at)),
        (
            "Size",
            format!("{} bytes ({} chars)", d.text.len(), d.text.char_count()),
        ),
        (
            "Lines",
            (d.text.expose().matches('\n').count() + 1).to_string(),
        ),
        ("Versions", d.history.len().to_string()),
        (
            "Mode",
            if d.mode == DocMode::Markdown {
                "Markdown".into()
            } else {
                "Plain text".into()
            },
        ),
        ("Id", d.id.short()),
    ];
    egui::Grid::new("info-grid")
        .num_columns(2)
        .spacing(egui::vec2(12.0, 6.0))
        .show(ui, |ui| {
            for (k, v) in rows {
                ui.label(RichText::new(k).size(12.0).color(theme::MUTED));
                ui.label(RichText::new(v).font(theme::mono(12.0)).color(theme::TEXT));
                ui.end_row();
            }
        });
    ui.add_space(10.0);
    theme::section_label(ui, "TAGS");
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for t in &tags {
            let b = egui::Button::new(
                RichText::new(format!("#{t}  {}", icon::X))
                    .size(12.0)
                    .color(theme::TEXT),
            )
            .fill(theme::PANEL_HI)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(10);
            if ui.add(b).on_hover_text("Remove tag").clicked() {
                remove = Some(t.clone());
            }
        }
    });
    let mut add = false;
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut *app.ws.tags_input)
                .hint_text("add tag (no spaces)")
                .desired_width(160.0),
        );
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            add = true;
        }
        if ui.button(format!("{} Add", icon::PLUS)).clicked() {
            add = true;
        }
    });
    if add || remove.is_some() {
        let mut new: Vec<String> = tags.clone();
        if let Some(r) = remove {
            new.retain(|t| *t != r);
        }
        if add {
            for t in app.ws.tags_input.split([',', ' ']) {
                let t = t.trim().trim_start_matches('#');
                if !t.is_empty() {
                    new.push(t.to_owned());
                }
            }
        }
        let refs: Vec<&str> = new.iter().map(String::as_str).collect();
        let r = app
            .session
            .as_mut()
            .map(|s| s.vault_mut().set_tags(id, &refs));
        match r {
            Some(Ok(())) => {
                app.ws.tags_input = Zeroizing::new(String::new());
                app.mark_changed();
            }
            Some(Err(e)) => app.error_toast(e),
            None => {}
        }
    }
    ui.add_space(12.0);
    theme::section_label(ui, "ACTIONS");
    let ctx = ui.ctx().clone();
    ui.horizontal_wrapped(|ui| {
        for (label, cmd) in [
            (
                format!("{} Rename", icon::PENCIL_SIMPLE),
                Command::RenameDocument,
            ),
            (format!("{} Pin", icon::PUSH_PIN), Command::TogglePin),
            (format!("{} Copy", icon::COPY), Command::CopyDocument),
            (
                format!("{} Export .age", icon::EXPORT),
                Command::ExportEncrypted,
            ),
        ] {
            if ui.button(label).clicked() {
                app.run_command(cmd, &ctx);
            }
        }
        if ui
            .button(
                RichText::new(format!("{} Plaintext export", icon::WARNING)).color(theme::AMBER),
            )
            .clicked()
        {
            app.run_command(Command::ExportPlaintext, &ctx);
        }
        if ui
            .button(RichText::new(format!("{} Trash", icon::TRASH)).color(theme::RED))
            .clicked()
        {
            app.run_command(Command::DeleteDocument, &ctx);
        }
    });
    if let Some(n) = app
        .session
        .as_ref()
        .and_then(|s| s.vault().document(id))
        .map(|d| d.history.len())
        && n > 1
    {
        ui.add_space(8.0);
        if ui
            .button(
                RichText::new(format!("{} Purge older versions ({})", icon::BROOM, n - 1))
                    .color(theme::AMBER),
            )
            .on_hover_text(
                "Keeps only the latest saved version. Frees space toward the 32 MiB vault limit.",
            )
            .clicked()
        {
            let r = app
                .session
                .as_mut()
                .map(|s| s.vault_mut().purge_history(id));
            if let Some(Ok(k)) = r {
                app.mark_changed();
                app.toast(ToastKind::Info, format!("Removed {k} older version(s) on next save. Older vault copies may still contain them."));
            }
        }
    }
}

// ---------------------------------------------------------------------------

fn palette(app: &mut NilavarApp, ctx: &egui::Context) {
    let Some(mut p) = app.ws.palette.take() else {
        return;
    };
    let mut close = false;
    let mut chosen_cmd: Option<Command> = None;
    let mut chosen_doc: Option<DocId> = None;

    // Build candidates.
    let docs: Vec<(DocId, String, String)> = app
        .session
        .as_ref()
        .map(|s| {
            let mut v: Vec<(DocId, String, String, i64)> = s
                .vault()
                .documents
                .iter()
                .map(|d| {
                    let folder = d
                        .folder
                        .map(|f| {
                            s.vault()
                                .folder_path(f)
                                .iter()
                                .map(|n| n.expose().to_owned())
                                .collect::<Vec<_>>()
                                .join(" / ")
                        })
                        .unwrap_or_default();
                    (d.id, display_title(d.title.expose()), folder, d.updated_at)
                })
                .collect();
            // Recent tabs first, then recency.
            v.sort_by_key(|x| {
                let r = app
                    .ws
                    .recent
                    .iter()
                    .position(|d| *d == x.0)
                    .unwrap_or(usize::MAX);
                (r, std::cmp::Reverse(x.3))
            });
            v.into_iter().map(|(a, b, c, _)| (a, b, c)).collect()
        })
        .unwrap_or_default();

    let q = p.query.to_string();
    let mut items: Vec<(i64, usize)> = Vec::new();
    if p.docs_mode {
        for (i, (_, title, folder)) in docs.iter().enumerate() {
            let cand = if folder.is_empty() {
                title.clone()
            } else {
                format!("{title} {folder}")
            };
            if let Some(s) = search::fuzzy_score(&q, &cand) {
                items.push((s, i));
            }
        }
    } else {
        for (i, c) in Command::ALL.iter().enumerate() {
            if c.needs_document() && app.ws.active.is_none() {
                continue;
            }
            if let Some(s) = search::fuzzy_score(&q, c.label()) {
                items.push((s, i));
            }
        }
    }
    if !q.is_empty() {
        items.sort_by_key(|x| std::cmp::Reverse(x.0));
    }
    items.truncate(60);
    if p.selected >= items.len() {
        p.selected = items.len().saturating_sub(1);
    }
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) && !items.is_empty() {
            p.selected = (p.selected + 1) % items.len();
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) && !items.is_empty() {
            p.selected = (p.selected + items.len() - 1) % items.len();
        }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            close = true;
        }
    });
    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

    let screen = ctx.content_rect();
    let w = (screen.width() - 40.0).min(560.0);
    egui::Area::new(egui::Id::unique("palette-backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let r = ui.allocate_rect(screen, Sense::click());
            ui.painter()
                .rect_filled(screen, 0.0, Color32::from_black_alpha(120));
            if r.clicked() {
                close = true;
            }
        });
    egui::Area::new(egui::Id::unique("palette"))
        .order(egui::Order::Tooltip)
        .fixed_pos(egui::pos2(screen.center().x - w / 2.0, screen.top() + 80.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::GREEN.gamma_multiply(0.5)))
                .corner_radius(10)
                .inner_margin(10)
                .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(200) })
                .show(ui, |ui| {
                    ui.set_width(w);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if p.docs_mode { icon::FILE_TEXT } else { icon::COMMAND }).color(theme::GREEN).size(16.0));
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut *p.query)
                                .id(egui::Id::unique("palette-input"))
                                .hint_text(if p.docs_mode { "Go to document…" } else { "Type a command…" })
                                .frame(egui::Frame::NONE)
                                .font(theme::mono(14.0))
                                .desired_width(f32::INFINITY),
                        );
                        if p.just_opened {
                            r.request_focus();
                            p.just_opened = false;
                        }
                        if r.changed() {
                            p.selected = 0;
                        }
                    });
                    ui.separator();
                    if items.is_empty() {
                        ui.label(RichText::new("No matches").color(theme::MUTED));
                    }
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (row, (_, i)) in items.iter().enumerate() {
                            let on = row == p.selected;
                            let (label, right) = if p.docs_mode {
                                let (_, t, f) = &docs[*i];
                                (format!("{}  {t}", icon::FILE_TEXT), f.clone())
                            } else {
                                let c = Command::ALL[*i];
                                (c.label().to_owned(), platform_shortcut(c.shortcut()))
                            };
                            let resp = egui::Frame::new()
                                .fill(if on { theme::GREEN_DIM } else { Color32::TRANSPARENT })
                                .corner_radius(5)
                                .inner_margin(egui::Margin::symmetric(8, 5))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(label).color(if on { theme::GREEN } else { theme::TEXT }));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new(right).font(theme::mono(11.0)).color(theme::MUTED));
                                        });
                                    });
                                })
                                .response
                                .interact(Sense::click());
                            if on {
                                resp.scroll_to_me(None);
                            }
                            if resp.clicked() || (on && enter) {
                                if p.docs_mode {
                                    chosen_doc = Some(docs[*i].0);
                                } else {
                                    chosen_cmd = Some(Command::ALL[*i]);
                                }
                            }
                        }
                    });
                    ui.label(
                        RichText::new("↑↓ navigate · Enter run · Esc close · actions only, never shell commands")
                            .size(10.5)
                            .color(theme::MUTED),
                    );
                });
        });
    if let Some(d) = chosen_doc {
        app.open_doc(d);
        return;
    }
    if let Some(c) = chosen_cmd {
        app.run_command(c, ctx);
        return;
    }
    if !close {
        app.ws.palette = Some(p);
    }
}

fn toasts(app: &mut NilavarApp, ctx: &egui::Context) {
    let now = std::time::Instant::now();
    app.toasts.retain(|t| t.until > now);
    if app.toasts.is_empty() {
        return;
    }
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("toasts"))
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -36.0))
        .show(ctx, |ui| {
            ui.set_max_width((screen.width() - 40.0).min(420.0));
            for t in &app.toasts {
                let (c, ic) = match t.kind {
                    ToastKind::Info => (theme::CYAN, icon::INFO),
                    ToastKind::Success => (theme::GREEN, icon::CHECK_CIRCLE),
                    ToastKind::Warn => (theme::AMBER, icon::WARNING),
                    ToastKind::Error => (theme::RED, icon::WARNING_OCTAGON),
                };
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .stroke(Stroke::new(1.0, c.gamma_multiply(0.6)))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(ic).color(c));
                            ui.label(RichText::new(&t.text).color(theme::TEXT).size(13.0));
                        });
                    });
                ui.add_space(6.0);
            }
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}
