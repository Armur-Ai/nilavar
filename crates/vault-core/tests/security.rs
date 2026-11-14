//! Security and reliability acceptance tests (synthetic data only).

mod common;

use std::fs;
use std::path::Path;

use common::*;
use vault_core::crypto::{self, WorkFactor};
use vault_core::diagnostics::{Diagnostics, Event};
use vault_core::history::SaveTrigger;
use vault_core::lifecycle::{AutoLockOutcome, auto_lock};
use vault_core::limits;
use vault_core::model::Vault;
use vault_core::paths::DataDirs;
use vault_core::session::{self, Session};
use vault_core::storage::{FaultPoint, Faults};
use vault_core::{ErrorCode, Passphrase, io_export};

fn pw(s: &str) -> Passphrase {
    Passphrase::from(s)
}

/// A vault with canaries in every sensitive field.
fn canary_vault() -> Vault {
    let mut v = Vault::new();
    let f = v.create_folder(CANARY_FOLDER, None).unwrap();
    let d = v.create_document(CANARY_TITLE, Some(f)).unwrap();
    v.document_mut(d)
        .unwrap()
        .text
        .set(&format!("line one\n{CANARY_TEXT}\nünïcødé ✓ 🔐\n"));
    v.set_tags(d, &[CANARY_TAG]).unwrap();
    v
}

fn header_end(ct: &[u8]) -> usize {
    // "--- <mac>\n" ends the header; the 16-byte nonce follows.
    let pos = ct.windows(4).position(|w| w == b"\n---").expect("header");
    pos + ct[pos + 1..].iter().position(|b| *b == b'\n').unwrap() + 2
}

// ---------------------------------------------------------------------------
// Round trips

#[test]
fn roundtrip_unicode_empty_and_large_documents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.nvault.age");
    let mut v = Vault::new();
    let empty = v.create_document("", None).unwrap();
    let uni = v.create_document("Ünïcode 标题 🔐", None).unwrap();
    v.document_mut(uni)
        .unwrap()
        .text
        .set("ℵ₀ — עברית — العربية — 日本語 — \u{1F600}\r\n\t\0end");
    let big = v.create_document("big", None).unwrap();
    let big_text: String = "abcdefghij0123456789ü"
        .chars()
        .cycle()
        .take(limits::MAX_DOCUMENT_BYTES / 2)
        .collect();
    v.document_mut(big).unwrap().text.set(&big_text);
    let s = Session::create(&path, pw("p"), v, test_opts(&dir.path().join("d"))).unwrap();
    let before: Vec<(String, String)> = s
        .vault()
        .documents
        .iter()
        .map(|d| (d.title.expose().to_owned(), d.text.expose().to_owned()))
        .collect();
    drop(s);
    let (back, _) = session::open_vault_file(&path, &pw("p")).unwrap();
    let after: Vec<(String, String)> = back
        .documents
        .iter()
        .map(|d| (d.title.expose().to_owned(), d.text.expose().to_owned()))
        .collect();
    assert_eq!(before, after);
    assert_eq!(back.document(empty).unwrap().text.expose(), "");
}

// ---------------------------------------------------------------------------
// Tampering / truncation / malformed

fn sample_ciphertext(len: usize) -> (Vec<u8>, Vec<u8>) {
    let plain: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    (
        crypto::encrypt(&pw("pass"), WorkFactor::TESTING, &plain).unwrap(),
        plain,
    )
}

#[test]
fn flipped_bytes_anywhere_fail_without_output() {
    let (ct, _) = sample_ciphertext(150_000); // 3 chunks
    let hdr = header_end(&ct);
    let mut positions = vec![
        0,
        10,
        30,
        hdr - 5,
        hdr - 2,
        hdr + 3,
        hdr + 16 + 100,
        ct.len() / 2,
        ct.len() - 1,
    ];
    positions.dedup();
    for pos in positions {
        let mut bad = ct.clone();
        bad[pos] ^= 0x01;
        let r = crypto::decrypt(&pw("pass"), &bad, 1 << 20);
        assert!(r.is_err(), "flip at {pos} not detected");
    }
}

#[test]
fn truncation_at_header_body_and_end_is_detected() {
    let (ct, _) = sample_ciphertext(150_000);
    let hdr = header_end(&ct);
    for cut in [
        5,
        hdr - 10,
        hdr,
        hdr + 10,
        hdr + 16 + 65_536 + 16,
        ct.len() - 1,
    ] {
        let r = crypto::decrypt(&pw("pass"), &ct[..cut], 1 << 20);
        assert!(r.is_err(), "truncation at {cut} accepted");
    }
    // Truncating exactly at a chunk boundary (dropping the final chunk) is
    // caught by age's last-chunk flag.
    let boundary = hdr + 16 + 2 * (65_536 + 16);
    let e = crypto::decrypt(&pw("pass"), &ct[..boundary], 1 << 20).unwrap_err();
    assert!(
        matches!(e.code(), ErrorCode::Truncated | ErrorCode::BodyTampered),
        "{e:?}"
    );
}

#[test]
fn reordered_and_replaced_chunks_are_detected() {
    let (ct, _) = sample_ciphertext(200_000);
    let body = header_end(&ct) + 16;
    let chunk = 65_536 + 16;
    // Swap chunks 0 and 1.
    let mut swapped = ct.clone();
    let (a, b) = (body..body + chunk, body + chunk..body + 2 * chunk);
    let c0 = ct[a.clone()].to_vec();
    let c1 = ct[b.clone()].to_vec();
    swapped[a].copy_from_slice(&c1);
    swapped[b].copy_from_slice(&c0);
    assert_eq!(
        crypto::decrypt(&pw("pass"), &swapped, 1 << 20)
            .unwrap_err()
            .code(),
        ErrorCode::BodyTampered
    );
    // Splice the body of a different file under this header.
    let (other, _) = sample_ciphertext(200_000);
    let mut spliced = ct[..body].to_vec();
    spliced.extend_from_slice(&other[header_end(&other) + 16..]);
    assert!(crypto::decrypt(&pw("pass"), &spliced, 1 << 20).is_err());
}

#[test]
fn excessive_work_factor_rejected_quickly() {
    let ct = crypto::encrypt(&pw("p"), WorkFactor::TESTING, b"x").unwrap();
    let text = String::from_utf8_lossy(&ct[..header_end(&ct)]).into_owned();
    let line = text
        .lines()
        .find(|l| l.starts_with("-> scrypt "))
        .unwrap()
        .to_owned();
    let evil_line = format!("{} 30", line.rsplit_once(' ').unwrap().0);
    let mut evil = ct.clone();
    let start = ct
        .windows(line.len())
        .position(|w| w == line.as_bytes())
        .unwrap();
    evil.splice(start..start + line.len(), evil_line.bytes());
    let t = std::time::Instant::now();
    let e = crypto::decrypt(&pw("p"), &evil, 1024).unwrap_err();
    assert_eq!(e.code(), ErrorCode::ExcessiveWorkFactor);
    assert!(t.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn non_passphrase_and_garbage_inputs_rejected() {
    assert_eq!(
        crypto::decrypt(&pw("p"), b"", 10).unwrap_err().code(),
        ErrorCode::InvalidHeader
    );
    assert_eq!(
        crypto::decrypt(&pw("p"), b"hello world", 10)
            .unwrap_err()
            .code(),
        ErrorCode::InvalidHeader
    );
    // An X25519-recipient age file (header text only; never decrypted).
    let x = b"age-encryption.org/v1\n-> X25519 abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ\nYWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXphYmNkZWY\n--- AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";
    let e = crypto::decrypt(&pw("p"), x, 10).unwrap_err();
    assert!(
        matches!(
            e.code(),
            ErrorCode::NotPassphraseEncrypted | ErrorCode::InvalidHeader
        ),
        "{e:?}"
    );
}

#[test]
fn oversized_inputs_rejected_before_work() {
    let dir = tempfile::tempdir().unwrap();
    let huge = dir.path().join("huge.nvault.age");
    let f = fs::File::create(&huge).unwrap();
    f.set_len(limits::MAX_ENCRYPTED_BYTES as u64 + 1).unwrap();
    let e = session::open_vault_file(&huge, &pw("p")).unwrap_err();
    assert_eq!(e.code(), ErrorCode::EncryptedInputTooLarge);
    // Decrypted payload above the limit (valid crypto, oversized JSON).
    let big = vec![b' '; limits::MAX_PAYLOAD_BYTES + 1];
    let ct = crypto::encrypt(&pw("p"), WorkFactor::TESTING, &big).unwrap();
    let p = dir.path().join("big-payload.nvault.age");
    fs::write(&p, ct).unwrap();
    assert_eq!(
        session::open_vault_file(&p, &pw("p")).unwrap_err().code(),
        ErrorCode::PayloadTooLarge
    );
}

fn encrypt_json(dir: &Path, name: &str, json: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(
        &p,
        crypto::encrypt(&pw("p"), WorkFactor::TESTING, json.as_bytes()).unwrap(),
    )
    .unwrap();
    p
}

#[test]
fn malformed_payloads_rejected_and_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let id = "0123456789abcdef0123456789abcdef";
    let base = |extra: &str| {
        format!(
            "{{\"format\":\"nilavar-vault\",\"schema_version\":1,\"vault_id\":\"{id}\",\"revision_id\":\"{id}\",\
             \"parent_revision_id\":null,\"save_counter\":1,\"created_at\":0,\"saved_at\":0,\
             \"prefs\":{{\"idle_lock_secs\":300,\"clipboard_clear_secs\":30,\"autosave\":true,\"lock_on_sleep\":true}},\
             {extra}}}"
        )
    };
    let doc = |id: &str, folder: &str| {
        format!(
            "{{\"id\":\"{id}\",\"title\":\"t\",\"text\":\"x\",\"folder\":{folder},\"tags\":[],\"mode\":\"plain\",\
             \"pinned\":false,\"created_at\":0,\"updated_at\":0,\"history\":[]}}"
        )
    };
    let d1 = "11111111111111111111111111111111";
    let f1 = "22222222222222222222222222222222";
    let f2 = "33333333333333333333333333333333";
    let cases: Vec<(&str, String, ErrorCode)> = vec![
        (
            "ok",
            base(&format!(
                "\"folders\":[],\"documents\":[{}],\"trash\":[]",
                doc(d1, "null")
            )),
            ErrorCode::Internal,
        ),
        (
            "newer schema",
            base("\"folders\":[],\"documents\":[],\"trash\":[]")
                .replace("\"schema_version\":1", "\"schema_version\":7"),
            ErrorCode::UnsupportedSchema,
        ),
        (
            "dup ids",
            base(&format!(
                "\"folders\":[],\"documents\":[{},{}],\"trash\":[]",
                doc(d1, "null"),
                doc(d1, "null")
            )),
            ErrorCode::DuplicateId,
        ),
        (
            "dangling folder",
            base(&format!(
                "\"folders\":[],\"documents\":[{}],\"trash\":[]",
                doc(d1, &format!("\"{f1}\""))
            )),
            ErrorCode::DanglingReference,
        ),
        (
            "folder cycle",
            base(&format!(
                "\"folders\":[{{\"id\":\"{f1}\",\"name\":\"a\",\"parent\":\"{f2}\",\"created_at\":0}},{{\"id\":\"{f2}\",\"name\":\"b\",\"parent\":\"{f1}\",\"created_at\":0}}],\"documents\":[],\"trash\":[]"
            )),
            ErrorCode::FolderCycle,
        ),
        (
            "bad id",
            base(&format!(
                "\"folders\":[],\"documents\":[{}],\"trash\":[]",
                doc("XYZ", "null")
            )),
            ErrorCode::InvalidId,
        ),
        (
            "unknown field",
            base("\"folders\":[],\"documents\":[],\"trash\":[],\"surprise\":1"),
            ErrorCode::MalformedPayload,
        ),
        (
            "nesting",
            format!("{}{}", "[".repeat(100), "]".repeat(100)),
            ErrorCode::ExcessiveNesting,
        ),
        (
            "not json",
            "this is not json".to_owned(),
            ErrorCode::MalformedPayload,
        ),
    ];
    for (name, json, want) in cases {
        let p = encrypt_json(
            dir.path(),
            &format!("{}.age", name.replace(' ', "-")),
            &json,
        );
        let before = fs::read(&p).unwrap();
        let r = Session::open(&p, pw("p"), test_opts(&dir.path().join("d")));
        if want == ErrorCode::Internal {
            assert!(r.is_ok(), "{name}: {r:?}");
        } else {
            assert_eq!(r.unwrap_err().code(), want, "{name}");
        }
        assert_eq!(
            fs::read(&p).unwrap(),
            before,
            "{name}: file must not be modified"
        );
    }
}

// ---------------------------------------------------------------------------
// Failure-safe saving

#[test]
fn injected_faults_keep_a_complete_vault() {
    for at in [
        FaultPoint::CreateTemp,
        FaultPoint::Write,
        FaultPoint::Flush,
        FaultPoint::Replace,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.nvault.age");
        let mut opts = test_opts(&dir.path().join("d"));
        let mut s = Session::create(&path, pw("p"), canary_vault(), opts.clone()).unwrap();
        let good = fs::read(&path).unwrap();
        // Make an edit and save with a fault.
        let id = s.vault().documents[0].id;
        s.vault_mut().document_mut(id).unwrap().text.set("edited");
        s.mark_edited();
        drop(s);
        opts.faults = Faults { fail_at: Some(at) };
        let mut s2 = Session::open(&path, pw("p"), opts.clone()).unwrap();
        s2.vault_mut().document_mut(id).unwrap().text.set("edited");
        s2.mark_edited();
        let e = s2.save(SaveTrigger::Explicit).unwrap_err();
        assert_ne!(e.code(), ErrorCode::Internal, "{at:?}");
        assert!(
            s2.is_dirty(),
            "edits must remain in memory after a failed save"
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            good,
            "{at:?}: previous vault must be intact"
        );
        assert!(session::open_vault_file(&path, &pw("p")).is_ok());
        // No temp files left behind next to the vault.
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.ends_with(".tmp")),
            "{at:?}: {names:?}"
        );
        // Retry without faults succeeds and the new state opens.
        drop(s2);
        opts.faults = Faults::NONE;
        let mut s3 = Session::open(&path, pw("p"), opts).unwrap();
        s3.vault_mut().document_mut(id).unwrap().text.set("edited");
        s3.mark_edited();
        s3.save(SaveTrigger::Explicit).unwrap();
        let (v, _) = session::open_vault_file(&path, &pw("p")).unwrap();
        assert_eq!(v.document(id).unwrap().text.expose(), "edited");
    }
}

#[test]
fn oversized_save_is_refused_without_losing_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.nvault.age");
    let mut s = Session::create(
        &path,
        pw("p"),
        Vault::new(),
        test_opts(&dir.path().join("d")),
    )
    .unwrap();
    let good = fs::read(&path).unwrap();
    let big = "z".repeat(limits::MAX_DOCUMENT_BYTES);
    for i in 0..17 {
        let id = s
            .vault_mut()
            .create_document(&format!("d{i}"), None)
            .unwrap();
        s.vault_mut().document_mut(id).unwrap().text.set(&big);
    }
    s.mark_edited();
    let e = s.save(SaveTrigger::Explicit).unwrap_err();
    assert_eq!(e.code(), ErrorCode::SaveWouldExceedLimit);
    assert_eq!(s.vault().documents.len(), 17, "edits kept");
    assert_eq!(fs::read(&path).unwrap(), good, "last good vault kept");
    assert!(
        s.vault().documents.iter().all(|d| d.history.len() <= 1),
        "history not silently pruned"
    );
}

#[test]
fn passphrase_change_failure_keeps_old_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.nvault.age");
    let mut opts = test_opts(&dir.path().join("d"));
    Session::create(&path, pw("old pass"), canary_vault(), opts.clone()).unwrap();
    opts.faults = Faults {
        fail_at: Some(FaultPoint::Replace),
    };
    let mut s = Session::open(&path, pw("old pass"), opts.clone()).unwrap();
    assert!(
        s.change_passphrase(&pw("old pass"), pw("new pass"))
            .is_err()
    );
    assert!(session::open_vault_file(&path, &pw("old pass")).is_ok());
    assert!(session::open_vault_file(&path, &pw("new pass")).is_err());
    // Wrong current passphrase is refused.
    assert_eq!(
        s.change_passphrase(&pw("wrong"), pw("x"))
            .unwrap_err()
            .code(),
        ErrorCode::WrongPassphraseOrNotAVault
    );
    drop(s);
    // Success: opens with the new passphrase on a clean installation.
    opts.faults = Faults::NONE;
    let mut s = Session::open(&path, pw("old pass"), opts).unwrap();
    s.change_passphrase(&pw("old pass"), pw("new pass"))
        .unwrap();
    drop(s);
    let fresh = tempfile::tempdir().unwrap();
    let copied = fresh.path().join("copied.nvault.age");
    fs::copy(&path, &copied).unwrap();
    let s = Session::open(
        &copied,
        pw("new pass"),
        test_opts(&fresh.path().join("clean-install")),
    )
    .unwrap();
    assert!(s.vault().documents[0].text.expose().contains(CANARY_TEXT));
    assert!(session::open_vault_file(&copied, &pw("old pass")).is_err());
}

#[test]
fn external_modification_and_two_instances_never_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.nvault.age");
    let data = dir.path().join("d");
    drop(Session::create(&path, pw("p"), canary_vault(), test_opts(&data)).unwrap());

    // Cooperating instances: the second is refused while the first is open.
    let mut locked = test_opts(&data);
    locked.take_lock = true;
    let a = Session::open(&path, pw("p"), locked.clone()).unwrap();
    assert_eq!(
        Session::open(&path, pw("p"), locked.clone())
            .unwrap_err()
            .code(),
        ErrorCode::VaultInUse
    );
    drop(a);

    // Non-cooperating writers (e.g. another device via a sync client) do
    // not share the local lock: the fingerprint check must catch them.
    let mut unlocked = test_opts(&data);
    unlocked.take_lock = false;
    let mut a = Session::open(&path, pw("p"), unlocked.clone()).unwrap();
    let mut b = Session::open(&path, pw("p"), unlocked).unwrap();
    a.vault_mut().create_document("from a", None).unwrap();
    a.mark_edited();
    a.save(SaveTrigger::Explicit).unwrap();
    let after_a = fs::read(&path).unwrap();
    b.vault_mut().create_document("from b", None).unwrap();
    b.mark_edited();
    assert_eq!(
        b.save(SaveTrigger::Explicit).unwrap_err().code(),
        ErrorCode::ConflictDetected
    );
    assert_eq!(fs::read(&path).unwrap(), after_a);
    let copy = b.save_conflict_copy().unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        after_a,
        "conflict copy must not touch the original"
    );
    let (cv, _) = session::open_vault_file(&copy, &pw("p")).unwrap();
    assert!(cv.documents.iter().any(|d| d.title.expose() == "from b"));
    let (ov, _) = session::open_vault_file(&path, &pw("p")).unwrap();
    assert!(ov.documents.iter().any(|d| d.title.expose() == "from a"));
    assert!(!ov.documents.iter().any(|d| d.title.expose() == "from b"));
    // Deleting the file externally is also a conflict, not a silent recreate.
    fs::remove_file(&path).unwrap();
    a.vault_mut().create_document("again", None).unwrap();
    a.mark_edited();
    assert_eq!(
        a.save(SaveTrigger::Explicit).unwrap_err().code(),
        ErrorCode::ConflictDetected
    );
}

#[test]
fn history_trash_backup_and_restore_recover_exact_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.nvault.age");
    let data = dir.path().join("d");
    let mut s = Session::create(&path, pw("p"), Vault::new(), test_opts(&data)).unwrap();
    let id = s.vault_mut().create_document("doc", None).unwrap();
    for t in ["v1 text", "v2 text", "v3 text"] {
        s.vault_mut().document_mut(id).unwrap().text.set(t);
        s.mark_edited();
        s.save(SaveTrigger::Explicit).unwrap();
    }
    let h = s
        .vault()
        .document(id)
        .unwrap()
        .history
        .iter()
        .map(|h| h.text.expose().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(h, vec!["v1 text", "v2 text", "v3 text"]);
    let first = s.vault().document(id).unwrap().history[0].id;
    s.vault_mut().restore_history_entry(id, first).unwrap();
    s.mark_edited();
    s.save(SaveTrigger::Explicit).unwrap();
    let (v, _) = session::open_vault_file(&path, &pw("p")).unwrap();
    assert_eq!(v.document(id).unwrap().text.expose(), "v1 text");
    assert_eq!(v.document(id).unwrap().history.len(), 4);

    // Trash and restore.
    s.vault_mut().trash_document(id).unwrap();
    s.mark_edited();
    s.save(SaveTrigger::Explicit).unwrap();
    let (v, _) = session::open_vault_file(&path, &pw("p")).unwrap();
    assert!(v.document(id).is_none());
    assert_eq!(v.trash[0].document.text.expose(), "v1 text");
    s.vault_mut().restore_from_trash(id).unwrap();
    s.mark_edited();
    s.save(SaveTrigger::Explicit).unwrap();

    // Backup (different passphrase) and restore-as-new.
    let backup = dir.path().join("backup.nvault.age");
    s.backup_to(&backup, Some(&pw("backup pass"))).unwrap();
    let restored = dir.path().join("restored.nvault.age");
    let r = session::restore_as_new(
        &backup,
        &pw("backup pass"),
        &restored,
        pw("restored pass"),
        test_opts(&data),
    )
    .unwrap();
    assert_ne!(
        r.vault().vault_id,
        s.vault().vault_id,
        "restore gets a fresh identity"
    );
    assert_eq!(r.vault().document(id).unwrap().text.expose(), "v1 text");
    assert_eq!(
        r.vault().document(id).unwrap().history.len(),
        s.vault().document(id).unwrap().history.len()
    );
    // restore-as-new never overwrites.
    assert_eq!(
        session::restore_as_new(
            &backup,
            &pw("backup pass"),
            &restored,
            pw("x"),
            test_opts(&data)
        )
        .unwrap_err()
        .code(),
        ErrorCode::AlreadyExists
    );
}

// ---------------------------------------------------------------------------
// Lock policy

#[test]
fn auto_lock_policy_save_then_recovery_copy_then_discard() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.nvault.age");
    let data_root = dir.path().join("d");
    let data = DataDirs::at(&data_root);
    drop(Session::create(&path, pw("p"), canary_vault(), test_opts(&data_root)).unwrap());

    // 1. Clean session.
    let s = Session::open(&path, pw("p"), test_opts(&data_root)).unwrap();
    assert_eq!(auto_lock(s, &data), AutoLockOutcome::Clean);

    // 2. Dirty → saved.
    let mut s = Session::open(&path, pw("p"), test_opts(&data_root)).unwrap();
    s.vault_mut()
        .create_document("saved by lock", None)
        .unwrap();
    s.mark_edited();
    assert_eq!(auto_lock(s, &data), AutoLockOutcome::Saved);
    assert!(
        session::open_vault_file(&path, &pw("p"))
            .unwrap()
            .0
            .documents
            .iter()
            .any(|d| d.title.expose() == "saved by lock")
    );

    // 3. Save fails → encrypted unsaved-changes recovery copy.
    let mut o = test_opts(&data_root);
    o.faults = Faults {
        fail_at: Some(FaultPoint::Replace),
    };
    let mut s = Session::open(&path, pw("p"), o.clone()).unwrap();
    let vid = s.vault().vault_id;
    s.vault_mut().create_document("diverted", None).unwrap();
    s.mark_edited();
    let out = auto_lock(s, &data);
    assert!(matches!(out, AutoLockOutcome::RecoveryCopy(_)), "{out:?}");
    let store = vault_core::storage::RecoveryStore::for_vault(&data.recovery_dir(), vid);
    let unsaved = store
        .list()
        .into_iter()
        .find(|c| c.kind == vault_core::storage::RecoveryKind::Unsaved)
        .unwrap();
    let (rv, _) = session::open_vault_file(&unsaved.path, &pw("p")).unwrap();
    assert!(rv.documents.iter().any(|d| d.title.expose() == "diverted"));

    // 4. Save and recovery copy both fail → discarded, nothing plaintext.
    let blocked_root = dir.path().join("blocked");
    fs::write(&blocked_root, b"a file where a directory is needed").unwrap();
    let blocked = DataDirs::at(&blocked_root);
    let mut s = Session::open(&path, pw("p"), o).unwrap();
    s.vault_mut().create_document(CANARY_TITLE, None).unwrap();
    s.mark_edited();
    let out = auto_lock(s, &blocked);
    assert!(matches!(out, AutoLockOutcome::Discarded(_)), "{out:?}");
    assert!(out.needs_notice());
    assert_no_canary_in_tree(dir.path(), &[]);
}

// ---------------------------------------------------------------------------
// Leakage

fn encodings(canary: &str) -> Vec<Vec<u8>> {
    let utf16: Vec<u8> = canary
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let hex: String = canary.bytes().map(|b| format!("{b:02x}")).collect();
    let b64 = base64_std(canary.as_bytes());
    vec![
        canary.as_bytes().to_vec(),
        utf16,
        hex.into_bytes(),
        b64.trim_end_matches('=').as_bytes()[..12].to_vec(),
    ]
}

fn base64_std(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Scans every file under `root` (except `allowed` paths) for canaries in
/// raw, UTF-16LE, hex and base64 encodings.
fn assert_no_canary_in_tree(root: &Path, allowed: &[&Path]) {
    let canaries = [
        CANARY_PASS,
        CANARY_TEXT,
        CANARY_TITLE,
        CANARY_TAG,
        CANARY_FOLDER,
    ];
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            for e in fs::read_dir(&p).unwrap().flatten() {
                stack.push(e.path());
            }
            continue;
        }
        if allowed.iter().any(|a| *a == p) {
            continue;
        }
        let bytes = fs::read(&p).unwrap_or_default();
        for c in canaries {
            for enc in encodings(c) {
                assert!(
                    !bytes.windows(enc.len()).any(|w| w == enc.as_slice()),
                    "canary found in {}",
                    p.display()
                );
            }
        }
    }
}

#[test]
fn canaries_never_reach_disk_logs_or_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let data_root = dir.path().join("appdata");
    let data = DataDirs::at(&data_root);
    let path = dir.path().join("v.nvault.age");
    let mut diag = Diagnostics::default();
    let mut s = Session::create(
        &path,
        pw(CANARY_PASS),
        canary_vault(),
        test_opts(&data_root),
    )
    .unwrap();
    diag.record(Event::VaultCreated);

    // Successful operations.
    for i in 0..3 {
        let id = s.vault().documents[0].id;
        let t = format!(
            "{}\n{CANARY_TEXT} {i}",
            s.vault().document(id).unwrap().text.expose()
        );
        s.vault_mut().document_mut(id).unwrap().text.set(&t);
        s.mark_edited();
        s.save(SaveTrigger::Auto).unwrap();
        diag.record(Event::SaveSucceeded);
    }
    let backup = dir.path().join("backup.nvault.age");
    s.backup_to(&backup, None).unwrap();
    let export = dir.path().join("export.txt.age");
    io_export::export_encrypted(
        &s.vault().documents[0].text,
        &export,
        &pw(CANARY_PASS),
        WorkFactor::TESTING,
    )
    .unwrap();
    // Intentional plaintext export: excluded from the scan below.
    let plain = dir.path().join("intentional-plaintext.md");
    let c = io_export::confirm_plaintext_export(&plain, true).unwrap();
    io_export::export_plaintext(&s.vault().documents[0].text, c).unwrap();

    // Error paths.
    let e = session::open_vault_file(&path, &pw("wrong")).unwrap_err();
    diag.record(Event::UnlockFailed(e.code()));
    let shown = format!("{e} {e:?}");
    for c in [CANARY_PASS, CANARY_TEXT, CANARY_TITLE] {
        assert!(!shown.contains(c));
    }
    let mut faulty = test_opts(&data_root);
    faulty.faults = Faults {
        fail_at: Some(FaultPoint::Write),
    };
    drop(s);
    let mut s = Session::open(&path, pw(CANARY_PASS), faulty).unwrap();
    s.mark_edited();
    let e = s.save(SaveTrigger::Explicit).unwrap_err();
    diag.record(Event::SaveFailed(e.code()));

    // Debug output of secret-owning structures is redacted.
    let dbg = format!("{s:?}");
    for c in [
        CANARY_PASS,
        CANARY_TEXT,
        CANARY_TITLE,
        CANARY_TAG,
        CANARY_FOLDER,
    ] {
        assert!(!dbg.contains(c), "Debug leaked {c}");
    }

    // A panic inside secret-handling code: payload must not be printed by
    // our hook (the desktop/CLI install equivalent hooks).
    std::panic::set_hook(Box::new(|_| {}));
    let text = s.vault().documents[0].text.duplicate();
    let r = std::panic::catch_unwind(move || {
        let t = text.expose();
        // Slicing inside a multi-byte char panics with a message that quotes the string.
        let i = t.find('ü').unwrap() + 1;
        t[..i].len()
    });
    let _ = std::panic::take_hook();
    assert!(r.is_err());

    // Diagnostics report.
    let report = vault_core::diagnostics::DiagnosticReport::build(
        &diag,
        vault_core::diagnostics::Capabilities {
            no_app_networking: true,
            os_network_sandbox: false,
            core_dumps_disabled: true,
            sleep_lock_signals: true,
            clipboard_privacy_hints: true,
        },
    )
    .to_json();
    let report_path = data_root.join("report.json");
    fs::create_dir_all(&data_root).unwrap();
    fs::write(&report_path, &report).unwrap();
    drop(s);

    let _ = data;
    assert_no_canary_in_tree(dir.path(), &[plain.as_path()]);
    // Sanity: the intentional plaintext export does contain the canary.
    assert!(fs::read_to_string(&plain).unwrap().contains(CANARY_TEXT));
}

#[test]
fn recovery_copies_are_encrypted_and_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v.nvault.age");
    let data_root = dir.path().join("d");
    let mut s = Session::create(&path, pw("p"), canary_vault(), test_opts(&data_root)).unwrap();
    s.mark_edited();
    s.save(SaveTrigger::Explicit).unwrap();
    let copies = s.recovery_store().list();
    assert_eq!(
        copies.len(),
        1,
        "first save in a session keeps a last-known-good copy"
    );
    let (v, _) = session::open_vault_file(&copies[0].path, &pw("p")).unwrap();
    assert_eq!(v.documents.len(), 1);
    // Further saves within the interval do not add copies.
    s.mark_edited();
    s.save(SaveTrigger::Explicit).unwrap();
    assert_eq!(s.recovery_store().list().len(), 1);
    assert_no_canary_in_tree(&data_root, &[]);
}
