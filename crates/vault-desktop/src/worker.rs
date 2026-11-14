//! A single background worker for costly crypto and disk work.
//!
//! The queue holds at most one waiting job (`sync_channel(1)`), so secret
//! snapshots can never pile up. Every job carries the lock *epoch* it was
//! issued in; results from an older epoch (i.e. issued before a lock) are
//! dropped by the UI immediately, zeroizing any session they carry.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use vault_core::crypto::WorkFactor;
use vault_core::lifecycle::{self, AutoLockOutcome};
use vault_core::model::{Document, Vault};
use vault_core::paths::DataDirs;
use vault_core::session::{self, SaveJob, SaveResult, Session, SessionOptions};
use vault_core::{Passphrase, SecretText, VaultError, io_export};

pub enum SessionOp {
    ConflictCopy,
    ChangePassphrase {
        current: Passphrase,
        new: Passphrase,
    },
    Backup {
        dest: PathBuf,
    },
    Reload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionOpKind {
    ConflictCopy,
    ChangePassphrase,
    Backup,
    Reload,
}

pub enum Job {
    Unlock {
        path: PathBuf,
        pass: Passphrase,
        opts: SessionOptions,
    },
    Create {
        path: PathBuf,
        pass: Passphrase,
        opts: SessionOptions,
    },
    Save(SaveJob),
    WithSession(Box<Session>, SessionOp),
    AutoLock(Box<Session>, DataDirs),
    ExportEncrypted {
        text: SecretText,
        dest: PathBuf,
        pass: Passphrase,
        work: WorkFactor,
    },
    ImportEncrypted {
        path: PathBuf,
        pass: Passphrase,
    },
    RestoreAsNew {
        backup: PathBuf,
        pass: Passphrase,
        new_path: PathBuf,
        opts: SessionOptions,
    },
}

pub enum Done {
    Unlocked(Result<Box<Session>, VaultError>),
    Created(Result<Box<Session>, VaultError>),
    Saved(SaveResult),
    Session(
        Box<Session>,
        SessionOpKind,
        Result<Option<PathBuf>, VaultError>,
    ),
    AutoLocked(AutoLockOutcome),
    Exported(Result<(), VaultError>),
    ImportedEncrypted(Result<Document, VaultError>),
    Restored(Result<Box<Session>, VaultError>),
}

pub struct Envelope<T> {
    pub epoch: u64,
    pub item: T,
}

pub struct Worker {
    tx: SyncSender<Envelope<Job>>,
    rx: Receiver<Envelope<Done>>,
    in_flight: usize,
}

impl Worker {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (job_tx, job_rx) = mpsc::sync_channel::<Envelope<Job>>(1);
        let (done_tx, done_rx) = mpsc::channel::<Envelope<Done>>();
        thread::Builder::new()
            .name("nilavar-crypto".into())
            .spawn(move || {
                while let Ok(Envelope { epoch, item }) = job_rx.recv() {
                    let done = run(item);
                    if done_tx.send(Envelope { epoch, item: done }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok();
        Self {
            tx: job_tx,
            rx: done_rx,
            in_flight: 0,
        }
    }

    /// Submits a job; returns it back if the queue is full.
    pub fn submit(&mut self, epoch: u64, job: Job) -> Result<(), Box<Job>> {
        match self.tx.try_send(Envelope { epoch, item: job }) {
            Ok(()) => {
                self.in_flight += 1;
                Ok(())
            }
            Err(TrySendError::Full(e)) | Err(TrySendError::Disconnected(e)) => {
                Err(Box::new(e.item))
            }
        }
    }

    pub fn try_recv(&mut self) -> Option<Envelope<Done>> {
        match self.rx.try_recv() {
            Ok(d) => {
                self.in_flight = self.in_flight.saturating_sub(1);
                Some(d)
            }
            Err(_) => None,
        }
    }

    pub fn busy(&self) -> bool {
        self.in_flight > 0
    }
}

fn run(job: Job) -> Done {
    match job {
        Job::Unlock { path, pass, opts } => {
            Done::Unlocked(Session::open(&path, pass, opts).map(Box::new))
        }
        Job::Create { path, pass, opts } => Done::Created(
            Session::create(&path, pass, Vault::new_with_welcome(), opts).map(Box::new),
        ),
        Job::Save(job) => Done::Saved(job.run()),
        Job::WithSession(mut s, op) => {
            let (kind, res) = match op {
                SessionOp::ConflictCopy => (
                    SessionOpKind::ConflictCopy,
                    s.save_conflict_copy().map(Some),
                ),
                SessionOp::ChangePassphrase { current, new } => (
                    SessionOpKind::ChangePassphrase,
                    s.change_passphrase(&current, new).map(|_| None),
                ),
                SessionOp::Backup { dest } => (
                    SessionOpKind::Backup,
                    s.backup_to(&dest, None).map(|_| Some(dest)),
                ),
                SessionOp::Reload => (SessionOpKind::Reload, s.reload_from_disk().map(|_| None)),
            };
            Done::Session(s, kind, res)
        }
        Job::AutoLock(s, data) => Done::AutoLocked(lifecycle::auto_lock(*s, &data)),
        Job::ExportEncrypted {
            text,
            dest,
            pass,
            work,
        } => Done::Exported(io_export::export_encrypted(&text, &dest, &pass, work)),
        Job::ImportEncrypted { path, pass } => {
            Done::ImportedEncrypted(io_export::import_encrypted_text(&path, &pass))
        }
        Job::RestoreAsNew {
            backup,
            pass,
            new_path,
            opts,
        } => {
            let new_pass = pass.duplicate();
            Done::Restored(
                session::restore_as_new(&backup, &pass, &new_path, new_pass, opts).map(Box::new),
            )
        }
    }
}
