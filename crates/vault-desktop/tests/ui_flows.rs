//! Headless end-to-end UI tests driving the real application with
//! egui_kittest (AccessKit queries + simulated input), using only synthetic
//! vaults in temporary directories.
//!
//! Set `NILAVAR_SCREENSHOTS=<dir>` to also write rendered PNG screenshots
//! (wgpu offscreen rendering) of the main screens.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use vault_core::Passphrase;
use vault_core::crypto::WorkFactor;
use vault_core::history::SaveTrigger;
use vault_core::lifecycle::LockState;
use vault_core::model::Vault;
use vault_core::paths::DataDirs;
use vault_core::session::{self, Session, SessionOptions};
use vault_desktop::app::{NilavarApp, RightPane, Screen};
use vault_desktop::ui_locked::unlock_screen;

const PASS: &str = "synthetic demo passphrase";
const CANARY: &str = "NVCANARY-ui-body-4471";

fn opts(data: &Path) -> SessionOptions {
    let mut o = SessionOptions::new(DataDirs::at(data));
    o.work = WorkFactor::TESTING;
    o.take_lock = false;
    o
}

/// A synthetic demo vault with folders, tags, Markdown and history.
fn demo_vault(dir: &Path) -> PathBuf {
    let path = dir.join("vault.nvault.age");
    let mut v = Vault::new_with_welcome();
    let infra = v.create_folder("Infrastructure", None).unwrap();
    let personal = v.create_folder("Personal", None).unwrap();
    let keys = v.create_folder("API keys", Some(infra)).unwrap();
    let d = v
        .create_document("Staging API credentials", Some(keys))
        .unwrap();
    v.document_mut(d).unwrap().text.set(
        "# Staging API credentials\n\n> Synthetic example data — not real credentials.\n\n\
         | Service | Key id |\n|---|---|\n| payments-sandbox | `pk_test_EXAMPLE_0000` |\n| maps-dev | `AIzaEXAMPLE-not-real` |\n\n\
         ## Rotation checklist\n\n- [x] Generate new key in the dashboard\n- [ ] Update the deploy secret\n- [ ] Revoke the old key\n\n\
         ```bash\nexport API_KEY=\"example-not-a-real-key\"\n```\n\nSee [runbook](https://example.invalid/runbook).\n",
    );
    v.set_tags(d, &["work", "api"]).unwrap();
    let r = v
        .create_document("Recovery codes (example)", Some(personal))
        .unwrap();
    v.document_mut(r)
        .unwrap()
        .text
        .set("Example recovery codes (synthetic):\n\n1. 1111-2222\n2. 3333-4444\n3. 5555-6666\n");
    v.set_tags(r, &["2fa"]).unwrap();
    let n = v.create_document("Server notes", Some(infra)).unwrap();
    v.document_mut(n).unwrap().text.set(&format!(
        "# Server notes\n\nhost: demo.example.invalid\nuser: deploy\n\n{CANARY}\n"
    ));
    let mut s = Session::create(&path, Passphrase::from(PASS), v, opts(&dir.join("data"))).unwrap();
    // Build some history.
    for i in 0..3 {
        let id = s
            .vault()
            .documents
            .iter()
            .find(|x| x.title.expose() == "Server notes")
            .unwrap()
            .id;
        let doc = s.vault_mut().document_mut(id).unwrap();
        let t = format!("{}\nrevision note {i}\n", doc.text.expose());
        doc.text.set(&t);
        s.mark_edited();
        s.save(SaveTrigger::Explicit).unwrap();
    }
    path
}

fn harness(dir: &Path, vault: &Path) -> Harness<'static, NilavarApp> {
    let data = DataDirs::at(dir.join("data"));
    let vault = vault.to_path_buf();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_max_steps(1_000_000)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app =
                NilavarApp::new(cc, data, vault_desktop::detect_capabilities(false, false));
            app.work = WorkFactor::TESTING;
            app.settings.reduce_motion = true;
            app.screen = unlock_screen(vault);
            // Never touch the developer's real clipboard from tests.
            app.clipboard = Box::new(FakeClipboard::default());
            app
        });
    h.run_steps(3);
    h
}

fn wait_until(h: &mut Harness<'_, NilavarApp>, what: &str, cond: impl Fn(&NilavarApp) -> bool) {
    let start = Instant::now();
    while !cond(h.state()) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(15));
        h.step();
    }
    h.run_steps(2);
}

fn shot(h: &mut Harness<'_, NilavarApp>, name: &str) {
    let img = h.render().expect("offscreen render");
    assert!(img.width() > 0 && img.height() > 0);
    if let Some(dir) = std::env::var_os("NILAVAR_SCREENSHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        img.save(dir.join(format!("{name}.png"))).unwrap();
    }
}

fn type_into_focused(h: &mut Harness<'_, NilavarApp>, text: &str) {
    h.event(egui::Event::Text(text.to_owned()));
    h.run_steps(2);
}

fn unlock(h: &mut Harness<'_, NilavarApp>, pass: &str) {
    // The passphrase field is focused on the unlock screen.
    type_into_focused(h, pass);
    h.get_by_label_contains("Unlock").click();
    h.run_steps(2);
}

#[test]
fn unlock_edit_save_lock_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let vault = demo_vault(dir.path());
    let mut h = harness(dir.path(), &vault);
    shot(&mut h, "01-lock-screen");

    // Wrong passphrase: fixed error code, still locked, no partial UI.
    unlock(&mut h, "not the passphrase");
    wait_until(&mut h, "unlock failure", |a| {
        a.state == LockState::Locked && matches!(&a.screen, Screen::Unlock(f) if f.error.is_some())
    });
    assert!(h.query_by_label_contains("NV-101").is_some());
    assert!(h.state().session.is_none());
    shot(&mut h, "02-wrong-passphrase");

    // Correct passphrase.
    h.get_by_label_contains("Unlock").focus();
    if let Screen::Unlock(f) = &mut h.state_mut().screen {
        f.focus = true;
    }
    h.run_steps(2);
    unlock(&mut h, PASS);
    wait_until(&mut h, "unlocked", |a| {
        a.state == LockState::Unlocked && a.session.is_some()
    });
    assert!(h.query_by_label_contains("Welcome").is_some());

    // Open the credentials doc with preview.
    let id = h
        .state()
        .session
        .as_ref()
        .unwrap()
        .vault()
        .documents
        .iter()
        .find(|d| d.title.expose() == "Staging API credentials")
        .unwrap()
        .id;
    h.state_mut().open_doc(id);
    h.state_mut().ws.right = RightPane::Preview;
    let folders: Vec<_> = h
        .state()
        .session
        .as_ref()
        .unwrap()
        .vault()
        .folders
        .iter()
        .map(|f| f.id)
        .collect();
    h.state_mut().ws.expanded.extend(folders);
    h.run_steps(4);
    shot(&mut h, "03-editor-preview");

    // Command palette.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::P);
    h.run_steps(3);
    assert!(h.state().ws.palette.is_some());
    type_into_focused(&mut h, "lock");
    h.run_steps(2);
    shot(&mut h, "04-command-palette");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().ws.palette.is_none());

    // History pane on a document with revisions.
    let notes = h
        .state()
        .session
        .as_ref()
        .unwrap()
        .vault()
        .documents
        .iter()
        .find(|d| d.title.expose() == "Server notes")
        .unwrap()
        .id;
    h.state_mut().open_doc(notes);
    h.state_mut().ws.right = RightPane::History;
    let first = h
        .state()
        .session
        .as_ref()
        .unwrap()
        .vault()
        .document(notes)
        .unwrap()
        .history[0]
        .id;
    h.state_mut().ws.history_sel = Some(first);
    h.run_steps(4);
    shot(&mut h, "05-history-diff");

    // Edit through the editor widget and save with ⌘S.
    h.state_mut().ws.editor.request_focus = true;
    h.run_steps(2);
    type_into_focused(&mut h, "typed through the UI ✓");
    assert!(h.state().session.as_ref().unwrap().is_dirty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    wait_until(&mut h, "saved", |a| {
        !a.session.as_ref().unwrap().is_dirty() && a.state == LockState::Unlocked
    });
    let (v, _) = session::open_vault_file(&vault, &Passphrase::from(PASS)).unwrap();
    assert!(
        v.document(notes)
            .unwrap()
            .text
            .expose()
            .contains("typed through the UI ✓")
    );

    // Lock with ⇧⌘L: workspace masked, session dropped, egui memory wiped.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::L);
    wait_until(&mut h, "locked", |a| a.state == LockState::Locked);
    assert!(h.state().session.is_none());
    assert!(h.state().ws.tabs.is_empty());
    assert!(h.query_by_label_contains(CANARY).is_none());
    assert!(
        h.query_by_label_contains("Staging API credentials")
            .is_none()
    );
    let leftover = h.state().session.is_none();
    assert!(leftover);
    shot(&mut h, "06-locked-again");
}

#[test]
fn unsaved_manual_lock_offers_save_discard_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let vault = demo_vault(dir.path());
    let mut h = harness(dir.path(), &vault);
    unlock(&mut h, PASS);
    wait_until(&mut h, "unlocked", |a| a.state == LockState::Unlocked);
    h.state_mut().ws.editor.request_focus = true;
    h.run_steps(2);
    type_into_focused(&mut h, "unsaved words");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::L);
    h.run_steps(3);
    // Still unlocked; dialog offered.
    assert_eq!(h.state().state, LockState::Unlocked);
    assert!(
        h.query_by_label_contains("Lock with unsaved changes?")
            .is_some()
    );
    shot(&mut h, "07-unsaved-on-lock");
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(h.state().state, LockState::Unlocked);

    // Discard: locks without writing.
    let before = std::fs::read(&vault).unwrap();
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::L);
    h.run_steps(3);
    h.get_by_label("Discard changes").click();
    wait_until(&mut h, "locked", |a| a.state == LockState::Locked);
    assert_eq!(
        std::fs::read(&vault).unwrap(),
        before,
        "discard must not write"
    );
}

#[test]
fn conflict_is_detected_and_conflict_copy_written() {
    let dir = tempfile::tempdir().unwrap();
    let vault = demo_vault(dir.path());
    let mut h = harness(dir.path(), &vault);
    unlock(&mut h, PASS);
    wait_until(&mut h, "unlocked", |a| a.state == LockState::Unlocked);

    // Another writer modifies the file on disk.
    let mut other = Session::open(
        &vault,
        Passphrase::from(PASS),
        opts(&dir.path().join("data2")),
    )
    .unwrap();
    other
        .vault_mut()
        .create_document("from another device", None)
        .unwrap();
    other.mark_edited();
    other.save(SaveTrigger::Explicit).unwrap();
    let theirs = std::fs::read(&vault).unwrap();

    h.state_mut().ws.editor.request_focus = true;
    h.run_steps(2);
    type_into_focused(&mut h, "my edit");
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    wait_until(&mut h, "conflict dialog", |a| {
        matches!(a.modal, Some(vault_desktop::app::Modal::Conflict))
    });
    assert_eq!(
        std::fs::read(&vault).unwrap(),
        theirs,
        "never silently overwrite"
    );
    shot(&mut h, "08-conflict");
    h.get_by_label("Save conflict copy").click();
    wait_until(&mut h, "conflict copy", |a| {
        !a.session_away && a.session.as_ref().is_some_and(|s| s.path() != vault)
    });
    let copy = h.state().session.as_ref().unwrap().path().to_path_buf();
    assert!(
        copy.file_name()
            .unwrap()
            .to_string_lossy()
            .contains("(conflict ")
    );
    let (v, _) = session::open_vault_file(&copy, &Passphrase::from(PASS)).unwrap();
    assert!(
        v.documents
            .iter()
            .any(|d| d.text.expose().contains("my edit"))
    );
    assert_eq!(std::fs::read(&vault).unwrap(), theirs);
}

#[test]
fn idle_lock_saves_dirty_session_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let vault = demo_vault(dir.path());
    let mut h = harness(dir.path(), &vault);
    unlock(&mut h, PASS);
    wait_until(&mut h, "unlocked", |a| a.state == LockState::Unlocked);
    h.state_mut().ws.editor.request_focus = true;
    h.run_steps(2);
    type_into_focused(&mut h, "idle edit");
    // Pretend the user has been idle past the timeout.
    let idle = h
        .state()
        .session
        .as_ref()
        .unwrap()
        .vault()
        .prefs
        .idle_lock_secs;
    h.state_mut().last_activity = Instant::now() - Duration::from_secs(u64::from(idle) + 1);
    h.state_mut().tick(&egui::Context::default());
    wait_until(&mut h, "locked", |a| a.state == LockState::Locked);
    let (v, _) = session::open_vault_file(&vault, &Passphrase::from(PASS)).unwrap();
    assert!(
        v.documents
            .iter()
            .any(|d| d.text.expose().contains("idle edit"))
    );
}

#[test]
fn create_settings_empty_and_narrow_screens_render() {
    let dir = tempfile::tempdir().unwrap();
    let data = DataDirs::at(dir.path().join("data"));
    let target = dir.path().join("new.nvault.age");
    let t2 = target.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_max_steps(1_000_000)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app =
                NilavarApp::new(cc, data, vault_desktop::detect_capabilities(false, false));
            app.work = WorkFactor::TESTING;
            app.settings.reduce_motion = true;
            app.clipboard = Box::new(FakeClipboard::default());
            app
        });
    h.run_steps(3);
    shot(&mut h, "09-welcome");
    h.get_by_label_contains("Create new vault").click();
    h.run_steps(2);
    if let Screen::Create(f) = &mut h.state_mut().screen {
        f.path = Some(t2);
    }
    h.run_steps(2);
    // Mismatched confirmation keeps the create button disabled.
    if let Screen::Create(f) = &mut h.state_mut().screen {
        f.pass = zeroize::Zeroizing::new("correct horse battery staple orbit".into());
        f.confirm = zeroize::Zeroizing::new("correct horse battery staple".into());
    }
    h.run_steps(3);
    assert!(h.query_by_label_contains("do not match").is_some());
    if let Screen::Create(f) = &mut h.state_mut().screen {
        f.confirm = zeroize::Zeroizing::new("correct horse battery staple orbit".into());
        f.ack_no_reset = true;
    }
    h.run_steps(3);
    shot(&mut h, "10-create-vault");
    h.get_by_label_contains("Create encrypted vault").click();
    wait_until(&mut h, "created", |a| a.state == LockState::Unlocked);
    assert!(target.exists());
    let (v, _) = session::open_vault_file(
        &target,
        &Passphrase::from("correct horse battery staple orbit"),
    )
    .unwrap();
    assert_eq!(v.documents.len(), 1);

    // Empty state: close the welcome tab.
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(3);
    assert!(h.state().ws.active.is_none());
    shot(&mut h, "11-empty-state");

    // Settings (security tab).
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(2);
    h.get_by_label("Security").click();
    h.run_steps(8);
    shot(&mut h, "12-settings-security");
    h.get_by_label("About & recovery").click();
    h.run_steps(3);
    shot(&mut h, "13-settings-about");
    h.key_press(Key::Escape);
    h.run_steps(2);

    // Narrow window collapses side panels instead of crushing the editor.
    h.state_mut().run_command(
        vault_desktop::commands::Command::NewDocument,
        &egui::Context::default(),
    );
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.state_mut().modal = None;
    h.set_size(egui::vec2(600.0, 700.0));
    h.run_steps(4);
    shot(&mut h, "14-narrow-window");
}

/// In-memory clipboard so tests never touch the developer's real clipboard.
#[derive(Clone, Default)]
struct FakeClipboard(std::sync::Arc<std::sync::Mutex<Option<String>>>);

impl vault_core::clipboard::ClipboardBackend for FakeClipboard {
    fn read_text(&mut self) -> Option<zeroize::Zeroizing<String>> {
        self.0.lock().unwrap().clone().map(zeroize::Zeroizing::new)
    }
    fn write_text(&mut self, text: &str) -> bool {
        *self.0.lock().unwrap() = Some(text.to_owned());
        true
    }
    fn clear(&mut self) -> bool {
        *self.0.lock().unwrap() = None;
        true
    }
}

#[test]
fn editor_copy_goes_through_guard_and_is_cleared_on_lock_only_if_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let vault = demo_vault(dir.path());
    let mut h = harness(dir.path(), &vault);
    let fake = FakeClipboard::default();
    h.state_mut().clipboard = Box::new(fake.clone());
    unlock(&mut h, PASS);
    wait_until(&mut h, "unlocked", |a| a.state == LockState::Unlocked);
    h.state_mut().ws.editor.request_focus = true;
    h.run_steps(2);
    // Select all and copy with the editor focused.
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.event(egui::Event::Copy);
    h.run_steps(2);
    let copied = fake
        .0
        .lock()
        .unwrap()
        .clone()
        .expect("copy reached the clipboard backend");
    assert!(copied.contains(CANARY), "copied the document text");
    assert!(
        h.state().clip_guard.has_pending(),
        "copy is tracked for auto-clear"
    );
    // egui's own clipboard command was intercepted (never written directly).
    assert!(
        !h.output()
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(_)))
    );
    // Locking clears it because it is unchanged.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::L);
    wait_until(&mut h, "locked", |a| a.state == LockState::Locked);
    assert!(
        fake.0.lock().unwrap().is_none(),
        "unchanged app clipboard value cleared on lock"
    );

    // Unrelated newer clipboard content survives a lock.
    if let Screen::Unlock(f) = &mut h.state_mut().screen {
        f.focus = true;
    }
    h.run_steps(2);
    unlock(&mut h, PASS);
    wait_until(&mut h, "unlocked again", |a| a.state == LockState::Unlocked);
    h.state_mut().copy_secret("synthetic value");
    *fake.0.lock().unwrap() = Some("something the user copied elsewhere".into());
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::L);
    wait_until(&mut h, "locked", |a| a.state == LockState::Locked);
    assert_eq!(
        fake.0.lock().unwrap().as_deref(),
        Some("something the user copied elsewhere")
    );
}

#[test]
fn preview_links_require_confirmation_and_never_open_directly() {
    let dir = tempfile::tempdir().unwrap();
    let vault = demo_vault(dir.path());
    let mut h = harness(dir.path(), &vault);
    unlock(&mut h, PASS);
    wait_until(&mut h, "unlocked", |a| a.state == LockState::Unlocked);
    let id = h
        .state()
        .session
        .as_ref()
        .unwrap()
        .vault()
        .documents
        .iter()
        .find(|d| d.title.expose() == "Staging API credentials")
        .unwrap()
        .id;
    h.state_mut().open_doc(id);
    h.state_mut().ws.right = RightPane::Preview;
    h.run_steps(4);
    let link = h.get_by_label_contains("See runbook");
    let r = link.rect();
    // Click on the word "runbook" near the end of the label.
    h.event(egui::Event::PointerMoved(egui::pos2(
        r.right() - 20.0,
        r.center().y,
    )));
    h.event(egui::Event::PointerButton {
        pos: egui::pos2(r.right() - 20.0, r.center().y),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: egui::pos2(r.right() - 20.0, r.center().y),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    assert!(
        matches!(&h.state().modal, Some(vault_desktop::app::Modal::Link(d)) if d == "https://example.invalid/runbook"),
        "clicking a link shows the confirmation dialog"
    );
    assert!(
        h.query_by_label_contains("https://example.invalid/runbook")
            .is_some(),
        "destination is visible"
    );
    assert!(
        !h.output()
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))),
        "no URL opened without confirmation"
    );
    shot(&mut h, "15-link-confirmation");
}
