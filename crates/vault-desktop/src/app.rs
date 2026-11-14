//! Application state, lock lifecycle, background jobs and commands.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Key, KeyboardShortcut, Modifiers};
use vault_core::autosave::{Autosave, AutosavePolicy};
use vault_core::clipboard::{ClearOutcome, ClipboardGuard};
use vault_core::crypto::WorkFactor;
use vault_core::diagnostics::{Capabilities, Diagnostics, Event, LockReason};
use vault_core::history::SaveTrigger;
use vault_core::ids::{DocId, FolderId, HistoryId};
use vault_core::lifecycle::{AutoLockOutcome, LockEvent, LockState};
use vault_core::model::DocMode;
use vault_core::paths::DataDirs;
use vault_core::search::SearchHit;
use vault_core::session::{Session, SessionOptions};
use vault_core::{ErrorCode, Passphrase, VaultError, io_export};
use zeroize::Zeroizing;

use crate::commands::Command;
use crate::editor::EditorState;
use crate::platform::{SleepDetector, SystemClipboard};
use crate::rain::Rain;
use crate::settings::{PendingNotice, Settings};
use crate::worker::{Done, Job, SessionOp, SessionOpKind, Worker};

// ---------------------------------------------------------------------------
// Locked-screen forms

pub struct UnlockForm {
    pub path: PathBuf,
    pub pass: Zeroizing<String>,
    pub reveal: bool,
    pub error: Option<ErrorCode>,
    pub focus: bool,
}

pub struct CreateForm {
    pub path: Option<PathBuf>,
    pub pass: Zeroizing<String>,
    pub confirm: Zeroizing<String>,
    pub reveal: bool,
    pub ack_no_reset: bool,
    pub error: Option<ErrorCode>,
}

pub enum Screen {
    Welcome,
    Unlock(UnlockForm),
    Create(CreateForm),
}

// ---------------------------------------------------------------------------
// Workspace (all plaintext UI state; dropped on lock)

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RightPane {
    None,
    Preview,
    History,
    Info,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Filter {
    All,
    Folder(FolderId),
    Tag(Zeroizing<String>),
    Trash,
}

pub struct PaletteState {
    pub docs_mode: bool,
    pub query: Zeroizing<String>,
    pub selected: usize,
    pub just_opened: bool,
}

pub struct Workspace {
    pub tabs: Vec<DocId>,
    pub active: Option<DocId>,
    pub recent: Vec<DocId>,
    pub sidebar_open: bool,
    pub right: RightPane,
    pub focus_mode: bool,
    pub filter: Filter,
    pub search: Zeroizing<String>,
    pub search_hits: Vec<SearchHit>,
    pub search_key: Option<u64>,
    pub search_focus: bool,
    pub palette: Option<PaletteState>,
    pub editor: EditorState,
    pub history_sel: Option<HistoryId>,
    pub history_diff: bool,
    pub preview: Option<(u64, vault_core::markdown::Preview)>,
    pub doc_edit_gen: HashMap<DocId, u64>,
    pub expanded: HashSet<FolderId>,
    pub tags_input: Zeroizing<String>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            recent: Vec::new(),
            sidebar_open: true,
            right: RightPane::None,
            focus_mode: false,
            filter: Filter::All,
            search: Zeroizing::new(String::new()),
            search_hits: Vec::new(),
            search_key: None,
            search_focus: false,
            palette: None,
            editor: EditorState::default(),
            history_sel: None,
            history_diff: true,
            preview: None,
            doc_edit_gen: HashMap::new(),
            expanded: HashSet::new(),
            tags_input: Zeroizing::new(String::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// Modals

pub enum Modal {
    UnsavedOnLock {
        quit: bool,
    },
    SavingBeforeLock,
    SaveFailedOnLock {
        code: ErrorCode,
        quit: bool,
    },
    Conflict,
    ChangePassphrase {
        current: Zeroizing<String>,
        new: Zeroizing<String>,
        confirm: Zeroizing<String>,
        reveal: bool,
        error: Option<ErrorCode>,
    },
    ExportEncrypted {
        doc: DocId,
        pass: Zeroizing<String>,
        confirm: Zeroizing<String>,
        reveal: bool,
        error: Option<ErrorCode>,
    },
    ExportPlaintext {
        doc: DocId,
        dest: PathBuf,
        ack: bool,
    },
    ImportEncrypted {
        path: PathBuf,
        pass: Zeroizing<String>,
        error: Option<ErrorCode>,
    },
    RestoreBackup {
        path: PathBuf,
        pass: Zeroizing<String>,
        error: Option<ErrorCode>,
    },
    RenameDoc {
        doc: DocId,
        name: Zeroizing<String>,
    },
    NewFolder {
        parent: Option<FolderId>,
        name: Zeroizing<String>,
    },
    RenameFolder {
        folder: FolderId,
        name: Zeroizing<String>,
    },
    ConfirmTrash(DocId),
    ConfirmPurge(DocId),
    ConfirmEmptyTrash,
    Link(String),
    Notice {
        title: &'static str,
        body: String,
    },
    Settings,
    Busy(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    Clean,
    Dirty,
    Saving,
    Failed(ErrorCode),
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LockMode {
    /// Drop the session (it is clean or the user chose to discard).
    Release,
    /// Automatic: encrypted save, else encrypted recovery copy, else discard.
    Auto,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Editor,
    Security,
    Appearance,
    Vault,
    Diagnostics,
    About,
}

// ---------------------------------------------------------------------------

pub struct NilavarApp {
    pub data: DataDirs,
    pub settings: Settings,
    pub diag: Diagnostics,
    pub caps: Capabilities,
    pub worker: Worker,
    pub epoch: u64,
    pub state: LockState,
    pub session: Option<Box<Session>>,
    pub session_away: bool,
    pub screen: Screen,
    pub ws: Workspace,
    pub autosave: Autosave,
    pub save_status: SaveStatus,
    pub save_gen_in_flight: Option<u64>,
    pub clipboard: SystemClipboard,
    pub clip_guard: ClipboardGuard,
    pub last_activity: Instant,
    pub sleep: SleepDetector,
    pub toasts: Vec<Toast>,
    pub modal: Option<Modal>,
    pub rain: Rain,
    pub settings_tab: SettingsTab,
    pub report_preview: Option<String>,
    pub work: WorkFactor,
    lock_mode: Option<LockMode>,
    lock_after_save: Option<bool>,
    quit_after_lock: bool,
    quit_now: bool,
    pub was_focused: bool,
    pub notice_on_unlock: Option<(PendingNotice, Option<String>)>,
    last_edit_at: Option<Instant>,
    /// Editor ids whose egui state (undo history) must be forgotten.
    pub pending_forget: Vec<egui::Id>,
    /// Wipe egui's per-widget memory (text-edit undo history) next frame.
    pub wipe_egui_memory: bool,
}

impl NilavarApp {
    pub fn new(cc: &eframe::CreationContext<'_>, data: DataDirs, caps: Capabilities) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let _ = data.ensure();
        let settings = Settings::load(&data.settings_file());
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let mut diag = Diagnostics::default();
        diag.record(Event::AppStarted);
        let screen = match settings.last_vault.clone() {
            Some(p) if settings.remember_last_vault && p.exists() => Screen::Unlock(UnlockForm {
                path: p,
                pass: Zeroizing::new(String::new()),
                reveal: false,
                error: None,
                focus: true,
            }),
            _ => Screen::Welcome,
        };
        let work = std::env::var("NILAVAR_DEV_WORK_FACTOR")
            .ok()
            .filter(|_| cfg!(debug_assertions))
            .and_then(|v| v.parse::<u8>().ok())
            .and_then(|v| WorkFactor::new(v).ok())
            .unwrap_or(WorkFactor::DEFAULT);
        Self {
            worker: Worker::spawn(cc.egui_ctx.clone()),
            data,
            settings,
            diag,
            caps,
            epoch: 0,
            state: LockState::Locked,
            session: None,
            session_away: false,
            screen,
            ws: Workspace::default(),
            autosave: Autosave::new(AutosavePolicy::default()),
            save_status: SaveStatus::Clean,
            save_gen_in_flight: None,
            clipboard: SystemClipboard::new(),
            clip_guard: ClipboardGuard::default(),
            last_activity: Instant::now(),
            sleep: SleepDetector::new(),
            toasts: Vec::new(),
            modal: None,
            rain: Rain::default(),
            settings_tab: SettingsTab::Editor,
            report_preview: None,
            work,
            lock_mode: None,
            lock_after_save: None,
            quit_after_lock: false,
            quit_now: false,
            was_focused: true,
            notice_on_unlock: None,
            last_edit_at: None,
            pending_forget: Vec::new(),
            wipe_egui_memory: false,
        }
    }

    // ----- small helpers ---------------------------------------------------

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        let secs = match kind {
            ToastKind::Error => 8,
            ToastKind::Warn => 6,
            _ => 3,
        };
        self.toasts.push(Toast {
            text: text.into(),
            kind,
            until: Instant::now() + Duration::from_secs(secs),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    pub fn error_toast(&mut self, e: VaultError) {
        self.diag.record(Event::Error(e.code()));
        self.toast(
            ToastKind::Error,
            format!("{}  ({})", e.code().message(), e.code().id()),
        );
    }

    pub fn save_settings(&mut self) {
        if self.settings.save(&self.data.settings_file()).is_err() {
            self.toast(ToastKind::Warn, "Settings could not be saved.");
        }
    }

    pub fn session_opts(&self) -> SessionOptions {
        let mut o = SessionOptions::new(self.data.clone());
        o.work = self.work;
        o
    }

    fn transition(&mut self, ev: LockEvent) {
        match self.state.apply(ev) {
            Ok(s) => self.state = s,
            Err(_) => self.diag.record(Event::Error(ErrorCode::Internal)),
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.state.shows_workspace() && self.session.is_some()
    }

    /// Records that the active document changed.
    pub fn mark_edited(&mut self, doc: DocId) {
        if let Some(s) = self.session.as_mut() {
            s.mark_edited();
            if let Some(d) = s.vault_mut().document_mut(doc) {
                d.updated_at = vault_core::time::now_unix();
            }
        }
        let now = Instant::now();
        self.autosave.on_edit(now);
        self.last_edit_at = Some(now);
        self.ws.doc_edit_gen.insert(doc, self.edit_generation());
        if !matches!(self.save_status, SaveStatus::Saving) {
            self.save_status = SaveStatus::Dirty;
        }
        self.ws.search_key = None;
    }

    /// Records a structural change (rename, move, tags, trash…).
    pub fn mark_changed(&mut self) {
        if let Some(s) = self.session.as_mut() {
            s.mark_edited();
        }
        let now = Instant::now();
        self.autosave.on_edit(now);
        self.last_edit_at = Some(now);
        if !matches!(self.save_status, SaveStatus::Saving) {
            self.save_status = SaveStatus::Dirty;
        }
        self.ws.search_key = None;
    }

    fn edit_generation(&self) -> u64 {
        // A monotonic counter local to the UI for per-doc dirty tracking.
        self.ws.doc_edit_gen.values().copied().max().unwrap_or(0) + 1
    }

    pub fn doc_is_dirty(&self, doc: DocId) -> bool {
        self.ws.doc_edit_gen.contains_key(&doc)
    }

    // ----- unlocking / creating ------------------------------------------

    pub fn start_unlock(&mut self) {
        let Screen::Unlock(form) = &mut self.screen else {
            return;
        };
        if form.pass.is_empty() {
            form.error = Some(ErrorCode::PassphraseEmpty);
            return;
        }
        let pass = Passphrase::new(std::mem::take(&mut *form.pass));
        let path = form.path.clone();
        form.error = None;
        let opts = self.session_opts();
        if self
            .worker
            .submit(self.epoch, Job::Unlock { path, pass, opts })
            .is_err()
        {
            if let Screen::Unlock(f) = &mut self.screen {
                f.error = Some(ErrorCode::Busy);
            }
            return;
        }
        self.transition(LockEvent::BeginUnlock);
    }

    pub fn start_create(&mut self) {
        let Screen::Create(form) = &mut self.screen else {
            return;
        };
        let Some(path) = form.path.clone() else {
            form.error = Some(ErrorCode::NotFound);
            return;
        };
        if form.pass.is_empty() {
            form.error = Some(ErrorCode::PassphraseEmpty);
            return;
        }
        if form.pass.as_str() != form.confirm.as_str() {
            form.error = Some(ErrorCode::PassphraseMismatch);
            return;
        }
        if !form.ack_no_reset {
            return;
        }
        let pass = Passphrase::new(std::mem::take(&mut *form.pass));
        form.confirm = Zeroizing::new(String::new());
        form.error = None;
        let opts = self.session_opts();
        if self
            .worker
            .submit(self.epoch, Job::Create { path, pass, opts })
            .is_err()
        {
            return;
        }
        self.transition(LockEvent::BeginUnlock);
    }

    fn on_unlocked(&mut self, session: Box<Session>, created: bool) {
        let path = session.path().to_path_buf();
        self.ws = Workspace::default();
        // Open the most recently updated document.
        let first = session
            .vault()
            .documents
            .iter()
            .max_by_key(|d| d.updated_at)
            .map(|d| d.id);
        self.session = Some(session);
        self.transition(LockEvent::UnlockSucceeded);
        self.diag.record(if created {
            Event::VaultCreated
        } else {
            Event::UnlockSucceeded
        });
        self.save_status = SaveStatus::Clean;
        self.autosave = Autosave::new(AutosavePolicy::default());
        self.last_activity = Instant::now();
        self.screen = Screen::Welcome;
        if let Some(id) = first {
            self.open_doc(id);
        }
        if self.settings.remember_last_vault {
            self.settings.last_vault = Some(path);
        }
        if let Some(n) = self.settings.pending_notice.take() {
            let code = self.settings.pending_notice_code.take();
            self.notice_on_unlock = Some((n, code));
        }
        self.save_settings();
        if let Some((n, code)) = self.notice_on_unlock.take() {
            let code = code.unwrap_or_default();
            let body = match n {
                PendingNotice::RecoveryCopy => format!(
                    "The last automatic lock could not save your changes to the vault file ({code}). \
                     They were written as an encrypted unsaved-changes recovery copy instead.\n\n\
                     Open Settings → Vault → Recovery copies to restore it as a new vault and compare."
                ),
                PendingNotice::Discarded => format!(
                    "The last automatic lock could not save your changes ({code}), and writing an \
                     encrypted recovery copy also failed. Those unsaved changes were discarded. \
                     No plaintext was written to disk."
                ),
            };
            self.modal = Some(Modal::Notice {
                title: "Unsaved changes from last session",
                body,
            });
        }
    }

    // ----- documents & tabs ----------------------------------------------

    pub fn open_doc(&mut self, id: DocId) {
        if !self.ws.tabs.contains(&id) {
            self.ws.tabs.push(id);
        }
        self.ws.active = Some(id);
        self.ws.recent.retain(|d| *d != id);
        self.ws.recent.insert(0, id);
        self.ws.recent.truncate(20);
        self.ws.history_sel = None;
        self.ws.editor.find.invalidate();
        self.ws.editor.request_focus = true;
    }

    pub fn close_tab(&mut self, id: DocId) {
        if let Some(i) = self.ws.tabs.iter().position(|d| *d == id) {
            self.ws.tabs.remove(i);
            if self.ws.active == Some(id) {
                self.ws.active = self
                    .ws
                    .recent
                    .iter()
                    .copied()
                    .find(|d| *d != id && self.ws.tabs.contains(d))
                    .or_else(|| self.ws.tabs.get(i.saturating_sub(1)).copied());
            }
            // Drop this editor's undo history (plaintext snapshots).
            self.pending_forget.push(crate::editor::editor_id(id));
        }
    }

    pub fn new_document(&mut self) {
        let folder = match &self.ws.filter {
            Filter::Folder(f) => Some(*f),
            _ => None,
        };
        let Some(s) = self.session.as_mut() else {
            return;
        };
        match s.vault_mut().create_document("Untitled", folder) {
            Ok(id) => {
                if let Filter::Tag(t) = &self.ws.filter {
                    let t = t.to_string();
                    let _ = s.vault_mut().set_tags(id, &[t.as_str()]);
                }
                if matches!(self.ws.filter, Filter::Trash) {
                    self.ws.filter = Filter::All;
                }
                self.mark_changed();
                self.open_doc(id);
                self.modal = Some(Modal::RenameDoc {
                    doc: id,
                    name: Zeroizing::new(String::new()),
                });
            }
            Err(e) => self.error_toast(e),
        }
    }

    // ----- saving -----------------------------------------------------------

    pub fn dispatch_save(&mut self, trigger: SaveTrigger) -> bool {
        if self.session_away || !matches!(self.state, LockState::Unlocked) {
            return false;
        }
        let Some(s) = self.session.as_mut() else {
            return false;
        };
        if s.save_in_flight() {
            return false;
        }
        match s.prepare_save(trigger) {
            Ok(job) => {
                if let Err(job) = self.worker.submit(self.epoch, Job::Save(job)) {
                    // Queue full: drop the job (wipes its payload) and restore state.
                    drop(job);
                    if let Some(s) = self.session.as_mut() {
                        let _ = s.finish_save(Err((VaultError::new(ErrorCode::Busy), 0)));
                    }
                    return false;
                }
                self.save_gen_in_flight = Some(self.edit_generation() - 1);
                self.autosave.on_dispatched();
                self.save_status = SaveStatus::Saving;
                self.transition(LockEvent::BeginSave);
                true
            }
            Err(e) => {
                self.save_status = SaveStatus::Failed(e.code());
                self.diag.record(Event::SaveFailed(e.code()));
                self.error_toast(e);
                false
            }
        }
    }

    fn on_saved(&mut self, result: vault_core::session::SaveResult) {
        let Some(s) = self.session.as_mut() else {
            self.transition(LockEvent::SaveFinished);
            return;
        };
        let res = s.finish_save(result);
        let dirty = s.is_dirty();
        self.transition(LockEvent::SaveFinished);
        let save_gen = self.save_gen_in_flight.take();
        match res {
            Ok(()) => {
                self.diag.record(Event::SaveSucceeded);
                self.autosave.on_succeeded();
                if let Some(g) = save_gen {
                    self.ws.doc_edit_gen.retain(|_, v| *v > g);
                }
                if !dirty {
                    self.ws.doc_edit_gen.clear();
                }
                self.save_status = if dirty {
                    SaveStatus::Dirty
                } else {
                    SaveStatus::Clean
                };
                if let Some(quit) = self.lock_after_save.take() {
                    if dirty {
                        // Edits arrived during the save; save again.
                        self.lock_after_save = Some(quit);
                        self.dispatch_save(SaveTrigger::Explicit);
                    } else {
                        self.modal = None;
                        self.quit_after_lock = quit;
                        self.begin_lock(LockReason::Manual, LockMode::Release);
                    }
                }
            }
            Err(e) => {
                self.diag.record(Event::SaveFailed(e.code()));
                self.save_status = SaveStatus::Failed(e.code());
                self.autosave.on_failed(
                    Instant::now(),
                    self.last_edit_at.unwrap_or_else(Instant::now),
                );
                if let Some(quit) = self.lock_after_save.take() {
                    self.modal = Some(Modal::SaveFailedOnLock {
                        code: e.code(),
                        quit,
                    });
                } else if e.code() == ErrorCode::ConflictDetected {
                    self.diag.record(Event::ConflictDetected);
                    self.modal = Some(Modal::Conflict);
                } else {
                    self.error_toast(e);
                }
            }
        }
    }

    // ----- locking ------------------------------------------------------------

    /// Entry point for every lock request.
    pub fn request_lock(&mut self, reason: LockReason) {
        if !matches!(self.state, LockState::Unlocked | LockState::Saving) || self.session_away {
            return;
        }
        let dirty = self.session.as_ref().is_some_and(|s| s.is_dirty());
        match reason {
            LockReason::Manual | LockReason::WindowClosed if dirty => {
                self.modal = Some(Modal::UnsavedOnLock {
                    quit: reason == LockReason::WindowClosed,
                });
            }
            LockReason::Manual | LockReason::WindowClosed => {
                self.quit_after_lock = reason == LockReason::WindowClosed;
                self.begin_lock(reason, LockMode::Release);
            }
            _ => self.begin_lock(
                reason,
                if dirty {
                    LockMode::Auto
                } else {
                    LockMode::Release
                },
            ),
        }
    }

    pub fn lock_choice_save(&mut self, quit: bool) {
        self.lock_after_save = Some(quit);
        self.modal = Some(Modal::SavingBeforeLock);
        if !self.dispatch_save(SaveTrigger::Explicit) {
            let in_flight = self.session.as_ref().is_some_and(|s| s.save_in_flight());
            if !in_flight {
                self.lock_after_save = None;
                let code = match self.save_status {
                    SaveStatus::Failed(c) => c,
                    _ => ErrorCode::Busy,
                };
                self.modal = Some(Modal::SaveFailedOnLock { code, quit });
            }
        }
    }

    pub fn lock_choice_discard(&mut self, quit: bool) {
        self.modal = None;
        self.quit_after_lock = quit;
        self.begin_lock(LockReason::Manual, LockMode::Release);
    }

    fn begin_lock(&mut self, reason: LockReason, mode: LockMode) {
        if !matches!(self.state, LockState::Unlocked | LockState::Saving) {
            return;
        }
        self.transition(LockEvent::BeginLock);
        self.lock_mode = Some(mode);
        self.diag.record(Event::Locked(reason));
        // Mask immediately: drop all plaintext UI state now.
        self.ws = Workspace::default();
        self.modal = None;
        if self.clip_guard.clear_now(&mut self.clipboard) == ClearOutcome::Cleared {
            self.diag.record(Event::ClipboardCleared);
        }
        self.wipe_egui_memory = true;
    }

    /// Advances an in-progress lock. Called every frame.
    fn drive_lock(&mut self) {
        if self.state != LockState::Locking {
            return;
        }
        if self.session_away {
            return; // a session op is running; wait for it to come back
        }
        if let Some(s) = self.session.take() {
            if s.save_in_flight() {
                self.session = Some(s);
                return;
            }
            let mode = self.lock_mode.unwrap_or(LockMode::Release);
            if mode == LockMode::Auto && s.is_dirty() {
                match self
                    .worker
                    .submit(self.epoch, Job::AutoLock(s, self.data.clone()))
                {
                    Ok(()) => {}
                    Err(job) => {
                        // Queue full; retry next frame.
                        if let Job::AutoLock(s, _) = *job {
                            self.session = Some(s);
                        }
                        return;
                    }
                }
            } else {
                drop(s);
            }
            // Results issued before this point refer to the old session.
            self.epoch += 1;
        }
        if !self.worker.busy() {
            self.transition(LockEvent::LockFinished { workers_idle: true });
            self.lock_mode = None;
            self.save_status = SaveStatus::Clean;
            self.screen = match self.settings.last_vault.clone() {
                Some(p) if self.settings.remember_last_vault => Screen::Unlock(UnlockForm {
                    path: p,
                    pass: Zeroizing::new(String::new()),
                    reveal: false,
                    error: None,
                    focus: true,
                }),
                _ => Screen::Welcome,
            };
            if self.quit_after_lock {
                self.quit_now = true;
            }
        }
    }

    fn on_auto_locked(&mut self, outcome: AutoLockOutcome) {
        match outcome {
            AutoLockOutcome::Clean | AutoLockOutcome::Saved => {}
            AutoLockOutcome::RecoveryCopy(code) => {
                self.diag.record(Event::RecoveryCopyWritten);
                self.settings.pending_notice = Some(PendingNotice::RecoveryCopy);
                self.settings.pending_notice_code = Some(code.id().to_owned());
                self.save_settings();
            }
            AutoLockOutcome::Discarded(code) => {
                self.diag.record(Event::LockDiscardedChanges);
                self.settings.pending_notice = Some(PendingNotice::Discarded);
                self.settings.pending_notice_code = Some(code.id().to_owned());
                self.save_settings();
            }
        }
    }

    // ----- worker results ---------------------------------------------------

    fn process_results(&mut self) {
        while let Some(env) = self.worker.try_recv() {
            let current = env.epoch == self.epoch;
            match env.item {
                Done::Unlocked(r) | Done::Created(r)
                    if !current || self.state != LockState::Unlocking =>
                {
                    // Stale: drop (zeroizes) any session that arrived late.
                    drop(r);
                    if self.state == LockState::Unlocking {
                        self.transition(LockEvent::UnlockFailed);
                    }
                }
                Done::Unlocked(Ok(s)) => self.on_unlocked(s, false),
                Done::Created(Ok(s)) => self.on_unlocked(s, true),
                Done::Unlocked(Err(e)) | Done::Created(Err(e)) => {
                    self.transition(LockEvent::UnlockFailed);
                    self.diag.record(Event::UnlockFailed(e.code()));
                    match &mut self.screen {
                        Screen::Unlock(f) => f.error = Some(e.code()),
                        Screen::Create(f) => f.error = Some(e.code()),
                        Screen::Welcome => self.error_toast(e),
                    }
                }
                Done::Saved(r) => {
                    if current {
                        self.on_saved(r);
                    } else if self.state == LockState::Saving {
                        self.transition(LockEvent::SaveFinished);
                    }
                }
                Done::AutoLocked(outcome) => self.on_auto_locked(outcome),
                Done::Session(s, kind, res) => self.on_session_op(s, kind, res, current),
                Done::Exported(r) => {
                    match r {
                        Ok(()) => {
                            self.diag.record(Event::Exported { plaintext: false });
                            self.toast(ToastKind::Success, "Encrypted export written. It opens with age/rage or Import encrypted.");
                        }
                        Err(e) => {
                            self.diag.record(Event::ExportFailed(e.code()));
                            self.error_toast(e);
                        }
                    }
                }
                Done::ImportedEncrypted(r) => {
                    if !current {
                        continue;
                    }
                    match r {
                        Ok(doc) => self.add_imported(vec![doc]),
                        Err(e) => {
                            self.diag.record(Event::ImportFailed(e.code()));
                            self.error_toast(e);
                        }
                    }
                }
                Done::Restored(r) => match r {
                    Ok(s) => {
                        let p = s.path().display().to_string();
                        drop(s);
                        self.toast(ToastKind::Success, format!("Restored as a new vault: {p}"));
                    }
                    Err(e) => self.error_toast(e),
                },
            }
        }
    }

    fn on_session_op(
        &mut self,
        s: Box<Session>,
        kind: SessionOpKind,
        res: Result<Option<PathBuf>, VaultError>,
        current: bool,
    ) {
        self.session_away = false;
        if matches!(self.modal, Some(Modal::Busy(_))) {
            self.modal = None;
        }
        if !current || !matches!(self.state, LockState::Unlocked | LockState::Locking) {
            drop(s);
            return;
        }
        let dirty = s.is_dirty();
        self.session = Some(s);
        match (kind, res) {
            (SessionOpKind::ConflictCopy, Ok(Some(p))) => {
                self.diag.record(Event::ConflictCopyWritten);
                self.save_status = SaveStatus::Clean;
                self.ws.doc_edit_gen.clear();
                if self.settings.remember_last_vault {
                    self.settings.last_vault = Some(p.clone());
                    self.save_settings();
                }
                self.modal = Some(Modal::Notice {
                    title: "Conflict copy saved",
                    body: format!(
                        "Your version was saved as a new encrypted file:\n\n{}\n\nThe file that changed on disk was \
                         not modified. You are now editing the conflict copy. Compare the two and merge by hand; \
                         Nilavar never merges or overwrites automatically.",
                        p.display()
                    ),
                });
            }
            (SessionOpKind::ChangePassphrase, Ok(_)) => {
                self.diag.record(Event::PassphraseChanged);
                self.save_status = SaveStatus::Clean;
                self.ws.doc_edit_gen.clear();
                self.modal = Some(Modal::Notice {
                    title: "Passphrase changed",
                    body: "The vault file is now encrypted with the new passphrase.\n\n\
                           Older copies — backups, recovery copies, cloud-drive version history and \
                           conflict copies — still open with the passphrase that was current when they \
                           were written. Delete them if the old passphrase may be compromised."
                        .into(),
                });
            }
            (SessionOpKind::Backup, Ok(p)) => {
                self.diag.record(Event::BackupWritten);
                let where_ = p.map(|p| p.display().to_string()).unwrap_or_default();
                self.toast(
                    ToastKind::Success,
                    format!("Encrypted backup written to {where_}"),
                );
            }
            (SessionOpKind::Reload, Ok(_)) => {
                self.ws.doc_edit_gen.clear();
                self.save_status = SaveStatus::Clean;
                self.ws.tabs.retain(|d| {
                    self.session
                        .as_ref()
                        .is_some_and(|s| s.vault().document(*d).is_some())
                });
                if let Some(a) = self.ws.active
                    && !self.ws.tabs.contains(&a)
                {
                    self.ws.active = self.ws.tabs.first().copied();
                }
                self.wipe_egui_memory = true;
                self.toast(
                    ToastKind::Info,
                    "Reloaded the vault from disk; your unsaved edits were discarded.",
                );
            }
            (SessionOpKind::ChangePassphrase, Err(e)) => {
                self.diag.record(Event::PassphraseChangeFailed(e.code()));
                self.save_status = if dirty {
                    SaveStatus::Dirty
                } else {
                    SaveStatus::Clean
                };
                self.modal = Some(Modal::Notice {
                    title: "Passphrase not changed",
                    body: format!(
                        "{} ({})\n\nThe vault file still opens with the previous passphrase.",
                        e.code().message(),
                        e.code().id()
                    ),
                });
            }
            (_, Err(e)) => self.error_toast(e),
            _ => {}
        }
    }

    pub fn run_session_op(&mut self, op: SessionOp, label: &'static str) {
        if self.session_away || self.state != LockState::Unlocked {
            self.toast(ToastKind::Warn, ErrorCode::Busy.message());
            return;
        }
        if self.session.as_ref().is_some_and(|s| s.save_in_flight()) {
            self.toast(
                ToastKind::Warn,
                "A save is still running. Try again in a moment.",
            );
            return;
        }
        let Some(s) = self.session.take() else { return };
        match self.worker.submit(self.epoch, Job::WithSession(s, op)) {
            Ok(()) => {
                self.session_away = true;
                self.modal = Some(Modal::Busy(label));
            }
            Err(job) => {
                if let Job::WithSession(s, _) = *job {
                    self.session = Some(s);
                }
                self.toast(ToastKind::Warn, ErrorCode::Busy.message());
            }
        }
    }

    pub fn add_imported(&mut self, docs: Vec<vault_core::model::Document>) {
        let folder = match &self.ws.filter {
            Filter::Folder(f) => Some(*f),
            _ => None,
        };
        let mut last = None;
        let mut n = 0;
        for mut d in docs {
            d.folder = folder;
            let Some(s) = self.session.as_mut() else {
                return;
            };
            match s.vault_mut().add_document(d) {
                Ok(id) => {
                    last = Some(id);
                    n += 1;
                }
                Err(e) => {
                    self.error_toast(e);
                    break;
                }
            }
        }
        if n > 0 {
            self.diag.record(Event::Imported);
            self.mark_changed();
            if let Some(id) = last {
                self.open_doc(id);
            }
            self.modal = Some(Modal::Notice {
                title: "Imported",
                body: format!(
                    "Imported {n} document(s) into the encrypted vault.\n\nThe original files were not \
                     changed or erased. If they contain secrets, delete them yourself — and remember that \
                     deleting a file does not guarantee it is unrecoverable from disk or backups."
                ),
            });
        }
    }

    // ----- commands -------------------------------------------------------------

    pub fn run_command(&mut self, cmd: Command, ctx: &egui::Context) {
        if !self.is_unlocked() {
            return;
        }
        let active = self.ws.active;
        if cmd.needs_document() && active.is_none() {
            return;
        }
        let doc = active.unwrap_or_else(DocId::random);
        match cmd {
            Command::NewDocument => self.new_document(),
            Command::NewFolder => {
                let parent = match &self.ws.filter {
                    Filter::Folder(f) => Some(*f),
                    _ => None,
                };
                self.modal = Some(Modal::NewFolder {
                    parent,
                    name: Zeroizing::new(String::new()),
                });
            }
            Command::Save => {
                if !self.dispatch_save(SaveTrigger::Explicit)
                    && self.session.as_ref().is_some_and(|s| !s.save_in_flight())
                    && !matches!(self.save_status, SaveStatus::Failed(_))
                {
                    self.save_status = SaveStatus::Clean;
                }
            }
            Command::Find | Command::FindReplace => {
                let f = &mut self.ws.editor.find;
                f.open = true;
                f.show_replace = cmd == Command::FindReplace || f.show_replace;
                f.focus_query = true;
            }
            Command::SearchVault => {
                self.ws.sidebar_open = true;
                self.ws.focus_mode = false;
                self.ws.search_focus = true;
            }
            Command::CommandPalette => {
                self.ws.palette = Some(PaletteState {
                    docs_mode: false,
                    query: Zeroizing::new(String::new()),
                    selected: 0,
                    just_opened: true,
                })
            }
            Command::QuickSwitch => {
                self.ws.palette = Some(PaletteState {
                    docs_mode: true,
                    query: Zeroizing::new(String::new()),
                    selected: 0,
                    just_opened: true,
                })
            }
            Command::CloseTab => self.close_tab(doc),
            Command::NextTab | Command::PrevTab => {
                if let Some(a) = self.ws.active
                    && let Some(i) = self.ws.tabs.iter().position(|d| *d == a)
                {
                    let n = self.ws.tabs.len();
                    let j = if cmd == Command::NextTab {
                        (i + 1) % n
                    } else {
                        (i + n - 1) % n
                    };
                    let id = self.ws.tabs[j];
                    self.open_doc(id);
                }
            }
            Command::ToggleSidebar => self.ws.sidebar_open = !self.ws.sidebar_open,
            Command::TogglePreview => self.toggle_right(RightPane::Preview),
            Command::ToggleHistory => self.toggle_right(RightPane::History),
            Command::ToggleInfo => self.toggle_right(RightPane::Info),
            Command::FocusMode => self.ws.focus_mode = !self.ws.focus_mode,
            Command::ToggleSoftWrap => {
                self.settings.soft_wrap = !self.settings.soft_wrap;
                self.save_settings();
            }
            Command::ToggleLineNumbers => {
                self.settings.line_numbers = !self.settings.line_numbers;
                self.save_settings();
            }
            Command::ToggleDocMode => {
                if let Some(d) = self
                    .session
                    .as_mut()
                    .and_then(|s| s.vault_mut().document_mut(doc))
                {
                    d.mode = match d.mode {
                        DocMode::Markdown => DocMode::Plain,
                        DocMode::Plain => DocMode::Markdown,
                    };
                    self.mark_changed();
                }
            }
            Command::RenameDocument => {
                let name = self
                    .session
                    .as_ref()
                    .and_then(|s| s.vault().document(doc))
                    .map(|d| Zeroizing::new(d.title.expose().to_owned()))
                    .unwrap_or_default();
                self.modal = Some(Modal::RenameDoc { doc, name });
            }
            Command::EditTags => {
                self.ws.right = RightPane::Info;
                self.ws.focus_mode = false;
            }
            Command::TogglePin => {
                if let Some(d) = self
                    .session
                    .as_mut()
                    .and_then(|s| s.vault_mut().document_mut(doc))
                {
                    d.pinned = !d.pinned;
                    self.mark_changed();
                }
            }
            Command::DeleteDocument => self.modal = Some(Modal::ConfirmTrash(doc)),
            Command::ShowTrash => {
                self.ws.filter = Filter::Trash;
                self.ws.sidebar_open = true;
            }
            Command::ShowAll => self.ws.filter = Filter::All,
            Command::CopyDocument => {
                if let Some(t) = self
                    .session
                    .as_ref()
                    .and_then(|s| s.vault().document(doc))
                    .map(|d| d.text.duplicate())
                {
                    self.copy_secret(t.expose());
                }
            }
            Command::ImportFiles => self.import_files(),
            Command::ImportEncrypted => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Import an age-encrypted text file")
                    .add_filter("age", &["age"])
                    .pick_file()
                {
                    self.modal = Some(Modal::ImportEncrypted {
                        path,
                        pass: Zeroizing::new(String::new()),
                        error: None,
                    });
                }
            }
            Command::ExportEncrypted => {
                self.modal = Some(Modal::ExportEncrypted {
                    doc,
                    pass: Zeroizing::new(String::new()),
                    confirm: Zeroizing::new(String::new()),
                    reveal: false,
                    error: None,
                })
            }
            Command::ExportPlaintext => self.export_plaintext_pick(doc),
            Command::Backup => self.backup_pick(),
            Command::RestoreBackup => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Choose a backup or recovery copy to restore")
                    .add_filter("Nilavar vault", &["age"])
                    .pick_file()
                {
                    self.modal = Some(Modal::RestoreBackup {
                        path,
                        pass: Zeroizing::new(String::new()),
                        error: None,
                    });
                }
            }
            Command::ChangePassphrase => {
                self.modal = Some(Modal::ChangePassphrase {
                    current: Zeroizing::new(String::new()),
                    new: Zeroizing::new(String::new()),
                    confirm: Zeroizing::new(String::new()),
                    reveal: false,
                    error: None,
                })
            }
            Command::Settings => self.modal = Some(Modal::Settings),
            Command::LockNow => self.request_lock(LockReason::Manual),
            Command::ZoomIn | Command::ZoomOut | Command::ZoomReset => {
                let z = match cmd {
                    Command::ZoomIn => (self.settings.ui_scale + 0.1).min(1.6),
                    Command::ZoomOut => (self.settings.ui_scale - 0.1).max(0.8),
                    _ => 1.0,
                };
                self.settings.ui_scale = (z * 10.0).round() / 10.0;
                ctx.set_zoom_factor(self.settings.ui_scale);
                self.save_settings();
            }
            Command::RecoveryChecklist => {
                self.settings_tab = SettingsTab::About;
                self.modal = Some(Modal::Settings);
            }
            Command::SecurityInfo => {
                self.settings_tab = SettingsTab::About;
                self.modal = Some(Modal::Settings);
            }
            Command::Diagnostics => {
                self.settings_tab = SettingsTab::Diagnostics;
                self.modal = Some(Modal::Settings);
            }
        }
    }

    fn toggle_right(&mut self, pane: RightPane) {
        self.ws.focus_mode = false;
        self.ws.right = if self.ws.right == pane {
            RightPane::None
        } else {
            pane
        };
    }

    pub fn copy_secret(&mut self, text: &str) {
        let secs = self
            .session
            .as_ref()
            .map(|s| s.vault().prefs.clipboard_clear_secs)
            .unwrap_or(30);
        if self.clip_guard.copy(
            &mut self.clipboard,
            text,
            Instant::now(),
            Duration::from_secs(u64::from(secs)),
        ) {
            self.toast(
                ToastKind::Info,
                format!("Copied. The clipboard will be cleared in {secs}s if unchanged."),
            );
        } else {
            self.toast(ToastKind::Warn, "The clipboard is unavailable.");
        }
    }

    pub fn import_files(&mut self) {
        let Some(files) = rfd::FileDialog::new()
            .set_title("Import UTF-8 text files")
            .add_filter(
                "Text",
                &[
                    "txt", "md", "markdown", "text", "env", "pem", "key", "json", "yaml", "yml",
                    "toml", "csv",
                ],
            )
            .add_filter("All files", &["*"])
            .pick_files()
        else {
            return;
        };
        let mut docs = Vec::new();
        for f in files {
            match io_export::import_text_file(&f) {
                Ok(d) => docs.push(d),
                Err(e) => {
                    self.diag.record(Event::ImportFailed(e.code()));
                    self.error_toast(e);
                }
            }
        }
        if !docs.is_empty() {
            self.add_imported(docs);
        }
    }

    pub fn export_plaintext_pick(&mut self, doc: DocId) {
        let Some(title) = self
            .session
            .as_ref()
            .and_then(|s| s.vault().document(doc))
            .map(|d| d.title.expose().to_owned())
        else {
            return;
        };
        let name = io_export::suggested_filename(&title, "md");
        if let Some(dest) = rfd::FileDialog::new()
            .set_title("Export PLAINTEXT — choose destination")
            .set_file_name(name)
            .save_file()
        {
            self.modal = Some(Modal::ExportPlaintext {
                doc,
                dest,
                ack: false,
            });
        }
    }

    pub fn backup_pick(&mut self) {
        let name = format!(
            "vault-backup-{}.{}",
            vault_core::time::format_compact(vault_core::time::now_unix()),
            vault_core::app::VAULT_EXTENSION
        );
        if let Some(dest) = rfd::FileDialog::new()
            .set_title("Save encrypted backup")
            .set_file_name(name)
            .save_file()
        {
            if self.session.as_ref().is_some_and(|s| s.path() == dest) {
                self.toast(
                    ToastKind::Warn,
                    "Choose a different file than the open vault.",
                );
                return;
            }
            self.run_session_op(SessionOp::Backup { dest }, "Writing encrypted backup…");
        }
    }

    // ----- per-frame plumbing ---------------------------------------------------

    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if !self.is_unlocked() {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let alt = Modifiers::COMMAND | Modifiers::ALT;
        let table: &[(Modifiers, Key, Command)] = &[
            (shift, Key::L, Command::LockNow),
            (shift, Key::P, Command::CommandPalette),
            (shift, Key::F, Command::SearchVault),
            (alt, Key::F, Command::FindReplace),
            (cmd, Key::N, Command::NewDocument),
            (cmd, Key::S, Command::Save),
            (cmd, Key::F, Command::Find),
            (cmd, Key::P, Command::QuickSwitch),
            (cmd, Key::W, Command::CloseTab),
            (cmd, Key::E, Command::TogglePreview),
            (shift, Key::Backslash, Command::FocusMode),
            (cmd, Key::Backslash, Command::ToggleSidebar),
            (cmd, Key::Comma, Command::Settings),
            (cmd, Key::Plus, Command::ZoomIn),
            (cmd, Key::Equals, Command::ZoomIn),
            (cmd, Key::Minus, Command::ZoomOut),
            (cmd, Key::Num0, Command::ZoomReset),
            (
                Modifiers::CTRL | Modifiers::SHIFT,
                Key::Tab,
                Command::PrevTab,
            ),
            (Modifiers::CTRL, Key::Tab, Command::NextTab),
        ];
        for (m, k, c) in table {
            let hit = ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(*m, *k)));
            if hit {
                self.run_command(*c, ctx);
            }
        }
    }

    /// Housekeeping that must run even when the window is hidden.
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.process_results();
        self.drive_lock();
        let now = Instant::now();

        if self.clip_guard.tick(&mut self.clipboard, now) == ClearOutcome::Cleared {
            self.diag.record(Event::ClipboardCleared);
        }

        if self.is_unlocked() && self.state == LockState::Unlocked && !self.session_away {
            let prefs = self
                .session
                .as_ref()
                .map(|s| s.vault().prefs)
                .unwrap_or_default();
            if self.sleep.check() && prefs.lock_on_sleep {
                self.request_lock(LockReason::SystemSleep);
                return;
            }
            let idle = Duration::from_secs(u64::from(prefs.idle_lock_secs));
            if now.saturating_duration_since(self.last_activity) >= idle {
                self.request_lock(LockReason::Idle);
                return;
            }
            let in_flight = self.session.as_ref().is_some_and(|s| s.save_in_flight());
            let dirty = self.session.as_ref().is_some_and(|s| s.is_dirty());
            if dirty
                && !matches!(self.modal, Some(Modal::Conflict))
                && self.autosave.due(now, in_flight, prefs.autosave)
            {
                self.dispatch_save(SaveTrigger::Auto);
            }
        } else {
            self.sleep.check();
        }

        // Keep timers ticking at a low rate (no busy loop).
        let wake = if self.state == LockState::Locked {
            5.0
        } else {
            1.0
        };
        ctx.request_repaint_after(Duration::from_secs_f32(wake));
        if self.clip_guard.has_pending()
            && let Some(d) = self.clip_guard.due_in(now)
        {
            ctx.request_repaint_after(d + Duration::from_millis(50));
        }
    }

    pub fn quit_requested(&mut self) -> bool {
        std::mem::take(&mut self.quit_now)
    }
}
