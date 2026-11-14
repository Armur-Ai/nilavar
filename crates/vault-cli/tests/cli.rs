//! End-to-end tests of `nilavar-cli` with synthetic data. Passphrases are
//! supplied on stdin (never argv/env).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_nilavar-cli");
const PASS: &str = "NVCANARY-cli-pass-77";
const BODY: &str = "NVCANARY-cli-body-99 synthetic secret";

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn p(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn run(&self, args: &[&str], stdin_lines: &[&str]) -> Output {
        let mut child = Command::new(BIN)
            .args(["--passphrase-stdin", "--work-factor", "10"])
            .args(args)
            .env("NILAVAR_DATA_DIR", self.p("appdata"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut si = child.stdin.take().unwrap();
            for l in stdin_lines {
                writeln!(si, "{l}").unwrap();
            }
        }
        child.wait_with_output().unwrap()
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn assert_no_canary(o: &Output) {
    for stream in [&o.stdout, &o.stderr] {
        let t = String::from_utf8_lossy(stream);
        assert!(!t.contains("NVCANARY-cli-pass"), "passphrase leaked: {t}");
        assert!(!t.contains("NVCANARY-cli-body"), "plaintext leaked: {t}");
    }
}

fn create_with_doc(env: &Env) -> (PathBuf, String) {
    let v = env.p("v.nvault.age");
    let o = env.run(&["create", s(&v)], &[PASS, PASS]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let note = env.p("note.md");
    std::fs::write(&note, BODY).unwrap();
    let o = env.run(&["import", s(&v), s(&note)], &[PASS]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(note.exists(), "import must leave the original in place");
    let o = env.run(&["list", s(&v)], &[PASS]);
    assert!(o.status.success());
    assert_no_canary(&o);
    let out = String::from_utf8(o.stdout).unwrap();
    // Two docs: welcome + imported (size of BODY).
    let line = out
        .lines()
        .find(|l| l.contains(&format!("{} B", BODY.len())))
        .expect("imported doc listed");
    (v, line.split_whitespace().next().unwrap().to_owned())
}

#[test]
fn create_verify_list_without_leaking() {
    let env = Env::new();
    let (v, _) = create_with_doc(&env);
    let o = env.run(&["verify", s(&v)], &[PASS]);
    assert!(o.status.success());
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("documents        : 2"));
    assert_no_canary(&o);
    // Titles are only printed on request.
    let o = env.run(&["list", "--titles", s(&v)], &[PASS]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("note"));
}

#[test]
fn wrong_passphrase_exit_code_and_message() {
    let env = Env::new();
    let (v, _) = create_with_doc(&env);
    let o = env.run(&["verify", s(&v)], &["wrong"]);
    assert_eq!(o.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&o.stderr).contains("NV-101"));
}

#[test]
fn encrypted_export_and_plaintext_confirmation() {
    let env = Env::new();
    let (v, id) = create_with_doc(&env);
    let enc = env.p("doc.age");
    let o = env.run(&["export", s(&v), "--doc", &id, "--out", s(&enc)], &[PASS, "exp", "exp"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let ct = std::fs::read(&enc).unwrap();
    assert!(ct.starts_with(b"age-encryption.org/v1\n"));
    assert!(!String::from_utf8_lossy(&ct).contains("NVCANARY"));

    // Plaintext without confirmation: refused, no file.
    let out = env.p("plain.txt");
    let o = env.run(&["export", s(&v), "--doc", &id, "--out", s(&out), "--plaintext"], &[PASS]);
    assert_eq!(o.status.code(), Some(5));
    assert!(!out.exists());
    // Wrong confirmation path: refused.
    let other = env.p("other.txt");
    let o = env.run(
        &["export", s(&v), "--doc", &id, "--out", s(&out), "--plaintext", "--confirm-plaintext-to", s(&other)],
        &[PASS],
    );
    assert_eq!(o.status.code(), Some(5));
    assert!(!out.exists());
    // Matching confirmation: written, but never to stdout.
    let o = env.run(
        &["export", s(&v), "--doc", &id, "--out", s(&out), "--plaintext", "--confirm-plaintext-to", s(&out)],
        &[PASS],
    );
    assert!(o.status.success());
    assert_no_canary(&o);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), BODY);
}

#[test]
fn change_passphrase_then_old_fails_new_works() {
    let env = Env::new();
    let (v, _) = create_with_doc(&env);
    let o = env.run(&["change-passphrase", s(&v)], &[PASS, "brand new pass", "brand new pass"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(env.run(&["verify", s(&v)], &[PASS]).status.code(), Some(3));
    assert!(env.run(&["verify", s(&v)], &["brand new pass"]).status.success());
}

#[test]
fn recover_to_new_vault_and_plaintext_dir() {
    let env = Env::new();
    let (v, _) = create_with_doc(&env);
    let nv = env.p("restored.nvault.age");
    let o = env.run(&["recover", s(&v), "--to-vault", s(&nv)], &[PASS, "np", "np"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(env.run(&["verify", s(&nv)], &["np"]).status.success());

    let dir = env.p("recovered");
    let o = env.run(
        &["recover", s(&v), "--plaintext-dir", s(&dir), "--confirm-plaintext-to", s(&dir)],
        &[PASS],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_no_canary(&o);
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(files.len(), 2);
    assert!(files
        .iter()
        .any(|f| std::fs::read_to_string(f.path()).unwrap() == BODY));
}

#[test]
fn backup_is_encrypted_and_verifies() {
    let env = Env::new();
    let (v, _) = create_with_doc(&env);
    let b = env.p("backup.nvault.age");
    let o = env.run(&["backup", s(&v), "--out", s(&b)], &[PASS]);
    assert!(o.status.success());
    assert!(env.run(&["verify", s(&b)], &[PASS]).status.success());
    // Refuses to overwrite.
    assert_eq!(env.run(&["backup", s(&v), "--out", s(&b)], &[PASS]).status.code(), Some(2));
}

#[test]
fn suggest_refuses_non_terminal_stdout() {
    let env = Env::new();
    let o = env.run(&["suggest"], &[]);
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
}
