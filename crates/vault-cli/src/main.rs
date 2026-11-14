//! `nilavar-cli` — verify, inspect and recover Nilavar vaults.
//!
//! This tool shares `vault-core` with the desktop app; it is not an
//! independent cryptographic implementation. For an independent route, see
//! `docs/RECOVERY.md` (reference `age`/`rage` + the published payload schema).
//!
//! Safety rules:
//! * passphrases are read from the terminal (no echo) or, with
//!   `--passphrase-stdin`, one per line from standard input — never from
//!   arguments or environment variables;
//! * recovered plaintext is never printed to stdout; plaintext output needs
//!   an explicit destination and a confirmation naming that destination;
//! * errors are reported as fixed codes.

#![deny(unsafe_code)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use vault_core::crypto::WorkFactor;
use vault_core::history::SaveTrigger;
use vault_core::ids::DocId;
use vault_core::io_export::{self, PLAINTEXT_CONFIRM_PHRASE};
use vault_core::model::Vault;
use vault_core::paths::DataDirs;
use vault_core::session::{self, Session, SessionOptions};
use vault_core::time::format_utc;
use vault_core::{ErrorCode, Passphrase, VaultError, strength};
use zeroize::Zeroize;

#[derive(Parser)]
#[command(
    name = "nilavar-cli",
    version,
    about = "Verify, inspect and recover Nilavar vaults (unaudited alpha)",
    long_about = "Verify, inspect and recover Nilavar vaults.\n\nPassphrases are prompted on the terminal. \
                  Recovered plaintext is never printed to standard output.\n\nThis is an unaudited alpha."
)]
struct Cli {
    /// Read passphrases from standard input, one per line, instead of the
    /// terminal. Only the line terminator is removed.
    #[arg(long, global = true)]
    passphrase_stdin: bool,

    /// scrypt work factor (log2 N) for files this command writes.
    #[arg(long, global = true, default_value_t = vault_core::limits::DEFAULT_SCRYPT_LOG_N, hide = true)]
    work_factor: u8,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Decrypt and fully validate a vault; print non-sensitive statistics.
    Verify {
        /// Vault file.
        vault: PathBuf,
    },
    /// List documents by id, size and date (titles only with --titles).
    List {
        /// Vault file.
        vault: PathBuf,
        /// Also print document titles to the terminal.
        #[arg(long)]
        titles: bool,
        /// Include trashed documents.
        #[arg(long)]
        trash: bool,
    },
    /// Export one document as an age-encrypted file (default) or plaintext.
    Export {
        /// Vault file.
        vault: PathBuf,
        /// Document id (from `list`).
        #[arg(long)]
        doc: String,
        /// Output file (must not exist).
        #[arg(long)]
        out: PathBuf,
        /// Write plaintext instead of an encrypted file.
        #[arg(long)]
        plaintext: bool,
        /// Non-interactive plaintext confirmation: must repeat --out exactly.
        #[arg(long, value_name = "PATH")]
        confirm_plaintext_to: Option<PathBuf>,
    },
    /// Recover every document into a directory (plaintext, confirmed) or
    /// re-encrypt the whole vault into a new file.
    Recover {
        /// Vault file (or a recovery copy / backup).
        vault: PathBuf,
        /// Write each document as a plaintext file into this new directory.
        #[arg(long, conflicts_with = "to_vault")]
        plaintext_dir: Option<PathBuf>,
        /// Non-interactive plaintext confirmation: must repeat --plaintext-dir.
        #[arg(long, value_name = "PATH")]
        confirm_plaintext_to: Option<PathBuf>,
        /// Re-encrypt into a new vault file with a fresh identity.
        #[arg(long)]
        to_vault: Option<PathBuf>,
    },
    /// Create a new, empty vault.
    Create {
        /// New vault file (must not exist).
        vault: PathBuf,
    },
    /// Import UTF-8 text files as new documents.
    Import {
        /// Vault file.
        vault: PathBuf,
        /// Files to import (originals are left unchanged).
        files: Vec<PathBuf>,
    },
    /// Change the vault passphrase.
    ChangePassphrase {
        /// Vault file.
        vault: PathBuf,
    },
    /// Write an encrypted backup copy of the vault.
    Backup {
        /// Vault file.
        vault: PathBuf,
        /// Destination file (must not exist).
        #[arg(long)]
        out: PathBuf,
    },
    /// List encrypted recovery copies kept for this vault.
    RecoveryCopies {
        /// Vault file.
        vault: PathBuf,
    },
    /// Print local passphrase-strength guidance for a passphrase you type.
    Strength,
    /// Suggest a random passphrase (printed to the terminal).
    Suggest {
        /// Number of words.
        #[arg(long, default_value_t = 7)]
        words: usize,
    },
}

struct Input {
    stdin_mode: bool,
    lines: Option<io::StdinLock<'static>>,
}

impl Input {
    fn new(stdin_mode: bool) -> Self {
        Self {
            stdin_mode,
            lines: None,
        }
    }

    fn passphrase(&mut self, prompt: &str) -> Result<Passphrase, VaultError> {
        if self.stdin_mode {
            let lock = self.lines.get_or_insert_with(|| io::stdin().lock());
            let mut line = String::new();
            let n = lock
                .read_line(&mut line)
                .map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?;
            if n == 0 {
                return Err(VaultError::new(ErrorCode::PassphraseEmpty));
            }
            // Strip only the line terminator; the passphrase itself is kept exactly.
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(Passphrase::new(line))
        } else {
            if !io::stdin().is_terminal() {
                return Err(VaultError::new(ErrorCode::Cancelled));
            }
            let s = rpassword::prompt_password(prompt)
                .map_err(|e| VaultError::io(ErrorCode::ReadFailed, &e))?;
            Ok(Passphrase::new(s))
        }
    }

    fn new_passphrase(&mut self, what: &str) -> Result<Passphrase, VaultError> {
        let a = self.passphrase(&format!("New {what} passphrase: "))?;
        if a.is_empty() {
            return Err(VaultError::new(ErrorCode::PassphraseEmpty));
        }
        let b = self.passphrase(&format!("Repeat new {what} passphrase: "))?;
        if !a.matches(&b) {
            return Err(VaultError::new(ErrorCode::PassphraseMismatch));
        }
        let est = strength::estimate(&a);
        eprintln!(
            "Strength (local estimate): {} (~{:.0} bits; rough offline guessing time: {}).",
            est.rating.label(),
            est.bits,
            est.offline_time_label()
        );
        Ok(a)
    }

    fn confirm_plaintext(&mut self, dest: &Path, flag: Option<&Path>) -> bool {
        if let Some(f) = flag {
            return f == dest;
        }
        if self.stdin_mode || !io::stdin().is_terminal() {
            eprintln!(
                "Plaintext export needs --confirm-plaintext-to <same path> when not interactive."
            );
            return false;
        }
        eprintln!(
            "WARNING: this writes UNENCRYPTED text to:\n  {}",
            dest.display()
        );
        eprintln!("Anyone or any program that can read that location can read it.");
        eprint!("Type '{PLAINTEXT_CONFIRM_PHRASE}' to continue: ");
        let _ = io::stderr().flush();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        let ok = line.trim_end_matches(['\r', '\n']) == PLAINTEXT_CONFIRM_PHRASE;
        line.zeroize();
        ok
    }
}

fn opts(work: WorkFactor, lock: bool) -> Result<SessionOptions, VaultError> {
    let data = DataDirs::resolve().map_err(|e| VaultError::io(ErrorCode::NotFound, &e))?;
    let mut o = SessionOptions::new(data);
    o.work = work;
    o.take_lock = lock;
    Ok(o)
}

/// macOS: apply the built-in "no-network" sandbox profile so the kernel
/// denies all sockets for this process (file access is unaffected).
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn deny_network() -> bool {
    use std::ffi::c_char;
    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> i32;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }
    let mut err: *mut c_char = std::ptr::null_mut();
    // SAFETY: NUL-terminated static profile name, valid out-pointer; the
    // error buffer (if any) is released once with `sandbox_free_error`.
    let rc = unsafe { sandbox_init(c"no-network".as_ptr(), 1, &mut err) };
    if !err.is_null() {
        // SAFETY: allocated by `sandbox_init`, freed exactly once.
        unsafe { sandbox_free_error(err) };
    }
    rc == 0
}

#[cfg(not(target_os = "macos"))]
fn deny_network() -> bool {
    false
}

fn main() -> ExitCode {
    let _ = deny_network();
    std::panic::set_hook(Box::new(|info| {
        // Never print panic payloads: they can contain fragments of data.
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        eprintln!("error: NV-499: internal error ({loc}); no data was written by this step");
    }));
    let cli = Cli::parse();
    let work = match WorkFactor::new(cli.work_factor) {
        Ok(w) => w,
        Err(e) => return fail(e),
    };
    let mut input = Input::new(cli.passphrase_stdin);
    match run(cli.cmd, &mut input, work) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

fn fail(e: VaultError) -> ExitCode {
    eprintln!("error: {e}");
    ExitCode::from(match e.code() {
        ErrorCode::WrongPassphraseOrNotAVault => 3,
        ErrorCode::ConflictDetected | ErrorCode::VaultInUse => 4,
        ErrorCode::PlaintextExportNotConfirmed | ErrorCode::Cancelled => 5,
        _ => 2,
    })
}

fn find_doc(vault: &Vault, id: &str) -> Result<DocId, VaultError> {
    let id = DocId::parse(id)?;
    if vault.document(id).is_some() || vault.trash.iter().any(|t| t.document.id == id) {
        Ok(id)
    } else {
        Err(VaultError::new(ErrorCode::NoSuchItem))
    }
}

fn run(cmd: Cmd, input: &mut Input, work: WorkFactor) -> Result<(), VaultError> {
    match cmd {
        Cmd::Verify { vault } => {
            let pass = input.passphrase("Vault passphrase: ")?;
            let (v, fp) = session::open_vault_file(&vault, &pass)?;
            let bytes: usize = v.approx_payload_bytes();
            println!("OK: vault decrypted, authenticated and validated.");
            println!("  schema version   : {}", vault_core::model::SCHEMA_VERSION);
            println!("  vault id         : {}", v.vault_id.short());
            println!(
                "  revision         : {} (save #{})",
                v.revision_id.short(),
                v.save_counter
            );
            println!("  last saved       : {}", format_utc(v.saved_at));
            println!("  documents        : {}", v.documents.len());
            println!("  folders          : {}", v.folders.len());
            println!("  trash            : {}", v.trash.len());
            println!("  history entries  : {}", v.total_history_entries());
            println!("  payload (approx) : {} KiB", bytes / 1024);
            println!("  file fingerprint : {}", fp.short_hex());
            Ok(())
        }
        Cmd::List {
            vault,
            titles,
            trash,
        } => {
            let pass = input.passphrase("Vault passphrase: ")?;
            let (v, _) = session::open_vault_file(&vault, &pass)?;
            let mut rows: Vec<(&vault_core::model::Document, bool)> =
                v.documents.iter().map(|d| (d, false)).collect();
            if trash {
                rows.extend(v.trash.iter().map(|t| (&t.document, true)));
            }
            for (d, in_trash) in rows {
                let mut line = format!(
                    "{}  {:>9} B  {:>3} rev  {}{}",
                    d.id,
                    d.text.len(),
                    d.history.len(),
                    format_utc(d.updated_at),
                    if in_trash { "  [trash]" } else { "" }
                );
                if titles {
                    line.push_str("  ");
                    line.push_str(d.title.expose());
                }
                println!("{line}");
            }
            Ok(())
        }
        Cmd::Export {
            vault,
            doc,
            out,
            plaintext,
            confirm_plaintext_to,
        } => {
            if vault_core::storage::check_target(&out)? {
                return Err(VaultError::new(ErrorCode::AlreadyExists));
            }
            let pass = input.passphrase("Vault passphrase: ")?;
            let (v, _) = session::open_vault_file(&vault, &pass)?;
            let id = find_doc(&v, &doc)?;
            let d = v
                .document(id)
                .or_else(|| {
                    v.trash
                        .iter()
                        .find(|t| t.document.id == id)
                        .map(|t| &t.document)
                })
                .ok_or(VaultError::new(ErrorCode::NoSuchItem))?;
            if plaintext {
                let ok = input.confirm_plaintext(&out, confirm_plaintext_to.as_deref());
                let c = io_export::confirm_plaintext_export(&out, ok)?;
                io_export::export_plaintext(&d.text, c)?;
                eprintln!("Wrote plaintext to {}", out.display());
            } else {
                let ep = input.new_passphrase("export")?;
                io_export::export_encrypted(&d.text, &out, &ep, work)?;
                eprintln!(
                    "Wrote encrypted export to {} (decrypt with `age -d` or `rage -d`).",
                    out.display()
                );
            }
            Ok(())
        }
        Cmd::Recover {
            vault,
            plaintext_dir,
            confirm_plaintext_to,
            to_vault,
        } => {
            if plaintext_dir.is_none() && to_vault.is_none() {
                eprintln!("Choose --plaintext-dir <DIR> or --to-vault <FILE>.");
                return Err(VaultError::new(ErrorCode::Cancelled));
            }
            let pass = input.passphrase("Passphrase of the file to recover: ")?;
            if let Some(dest) = to_vault {
                let np = input.new_passphrase("vault")?;
                let s = session::restore_as_new(&vault, &pass, &dest, np, opts(work, false)?)?;
                eprintln!(
                    "Recovered {} documents into new vault {}",
                    s.vault().documents.len(),
                    dest.display()
                );
                return Ok(());
            }
            let dir = plaintext_dir.unwrap_or_default();
            if dir.exists() {
                return Err(VaultError::new(ErrorCode::AlreadyExists));
            }
            let (v, _) = session::open_vault_file(&vault, &pass)?;
            let ok = input.confirm_plaintext(&dir, confirm_plaintext_to.as_deref());
            io_export::confirm_plaintext_export(&dir, ok)?;
            vault_core::paths::create_private_dir_all(&dir)
                .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
            let mut n = 0;
            for d in &v.documents {
                let name = format!(
                    "{}-{}",
                    d.id.short(),
                    io_export::suggested_filename(d.title.expose(), "md")
                );
                let path = dir.join(name);
                let c = io_export::confirm_plaintext_export(&path, true)?;
                io_export::export_plaintext(&d.text, c)?;
                n += 1;
            }
            eprintln!("Wrote {n} plaintext files to {}", dir.display());
            Ok(())
        }
        Cmd::Create { vault } => {
            let np = input.new_passphrase("vault")?;
            let s = Session::create(&vault, np, Vault::new_with_welcome(), opts(work, true)?)?;
            eprintln!("Created vault {}", s.path().display());
            Ok(())
        }
        Cmd::Import { vault, files } => {
            let pass = input.passphrase("Vault passphrase: ")?;
            let mut s = Session::open(&vault, pass, opts(work, true)?)?;
            for f in &files {
                let doc = io_export::import_text_file(f)?;
                s.vault_mut().add_document(doc)?;
                s.mark_edited();
            }
            s.save(SaveTrigger::Explicit)?;
            eprintln!(
                "Imported {} file(s). The original plaintext files were not modified or erased.",
                files.len()
            );
            Ok(())
        }
        Cmd::ChangePassphrase { vault } => {
            let pass = input.passphrase("Current passphrase: ")?;
            let current = pass.duplicate();
            let mut session = Session::open(&vault, pass, opts(work, true)?)?;
            let np = input.new_passphrase("vault")?;
            session.change_passphrase(&current, np)?;
            eprintln!(
                "Passphrase changed. Older backups and recovery copies still open with the old passphrase."
            );
            Ok(())
        }
        Cmd::Backup { vault, out } => {
            if vault_core::storage::check_target(&out)? {
                return Err(VaultError::new(ErrorCode::AlreadyExists));
            }
            let pass = input.passphrase("Vault passphrase: ")?;
            let s = Session::open(&vault, pass, opts(work, false)?)?;
            s.backup_to(&out, None)?;
            eprintln!(
                "Wrote encrypted backup to {} (same passphrase as the vault).",
                out.display()
            );
            Ok(())
        }
        Cmd::RecoveryCopies { vault } => {
            let pass = input.passphrase("Vault passphrase: ")?;
            let s = Session::open(&vault, pass, opts(work, false)?)?;
            let store = s.recovery_store();
            println!("Recovery copies in {}:", store.dir().display());
            for c in store.list() {
                println!("  {:?}  {}", c.kind, c.path.display());
            }
            println!(
                "Copies are encrypted with the passphrase that was current when they were written."
            );
            Ok(())
        }
        Cmd::Strength => {
            let p = input.passphrase("Passphrase to evaluate: ")?;
            let e = strength::estimate(&p);
            println!(
                "Local estimate: {} (~{:.0} bits). Rough offline guessing time at {:.0e} guesses/s: {}.",
                e.rating.label(),
                e.bits,
                strength::ASSUMED_GUESSES_PER_SEC,
                e.offline_time_label()
            );
            for h in e.hints {
                println!("  - {}", h.text());
            }
            Ok(())
        }
        Cmd::Suggest { words } => {
            if !(4..=16).contains(&words) {
                return Err(VaultError::new(ErrorCode::Cancelled));
            }
            if !io::stdout().is_terminal() {
                eprintln!("Refusing to write a passphrase suggestion to a non-terminal output.");
                return Err(VaultError::new(ErrorCode::Cancelled));
            }
            let p = strength::suggest_passphrase(words);
            let e = strength::estimate(&p);
            println!("{}", p.reveal());
            eprintln!(
                "(~{:.0} bits from {} random words; write it down somewhere safe)",
                e.bits, words
            );
            Ok(())
        }
    }
}
