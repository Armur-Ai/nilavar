//! Interoperability with the reference age (Go) and rage (Rust) tools.
//!
//! * Committed synthetic fixtures produced by `age -p` and `rage -p` must
//!   decrypt with vault-core (always runs).
//! * Live round-trips in both directions run when the tools are installed;
//!   otherwise they print `SKIPPED` (and fail if NILAVAR_REQUIRE_INTEROP is set).

mod common;

use std::path::Path;

use common::*;
use vault_core::crypto::{self, WorkFactor};
use vault_core::io_export;
use vault_core::{ErrorCode, Passphrase, SecretText};

const FIXTURE_PASS: &str = "synthetic-fixture-passphrase";

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn committed_reference_fixtures_decrypt() {
    let expected = std::fs::read(fixture("reference-plain.txt")).unwrap();
    for f in ["reference-age.txt.age", "reference-rage.txt.age"] {
        let ct = std::fs::read(fixture(f)).unwrap();
        let pt = crypto::decrypt(&Passphrase::from(FIXTURE_PASS), &ct, 1 << 20).unwrap();
        assert_eq!(&*pt, &expected[..], "{f}");
        assert_eq!(
            crypto::decrypt(&Passphrase::from("not it"), &ct, 1 << 20)
                .unwrap_err()
                .code(),
            ErrorCode::WrongPassphraseOrNotAVault
        );
    }
    // Encrypted document import accepts reference files.
    let d = io_export::import_encrypted_text(
        &fixture("reference-age.txt.age"),
        &Passphrase::from(FIXTURE_PASS),
    )
    .unwrap();
    assert!(d.text.expose().contains("héllo wörld 🔐"));
}

fn tool_or_skip(name: &str) -> Option<std::path::PathBuf> {
    match find_tool(name) {
        Some(p) => Some(p),
        None => {
            assert!(!interop_required(), "{name} required but not installed");
            eprintln!("SKIPPED: reference tool `{name}` is not installed");
            None
        }
    }
}

#[test]
fn our_exports_decrypt_with_reference_tools() {
    for tool in ["age", "rage"] {
        let Some(bin) = tool_or_skip(tool) else {
            continue;
        };
        let dir = tempfile::tempdir().unwrap();
        let enc = dir.path().join("export.txt.age");
        let out = dir.path().join("out.txt");
        let body = "Interop body ✓ — línea 2\nmore";
        // Default work factor so the file is exactly what the app writes.
        io_export::export_encrypted(
            &SecretText::from(body),
            &enc,
            &Passphrase::from("interop pass ü"),
            WorkFactor::DEFAULT,
        )
        .unwrap();
        let ok = run_with_pty(
            &bin,
            &["-d", "-o", out.to_str().unwrap(), enc.to_str().unwrap()],
            &["interop pass ü"],
        );
        assert!(ok, "{tool} failed to decrypt our export");
        assert_eq!(std::fs::read_to_string(&out).unwrap(), body);
    }
}

#[test]
fn reference_tool_output_decrypts_with_core() {
    for tool in ["age", "rage"] {
        let Some(bin) = tool_or_skip(tool) else {
            continue;
        };
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("p.txt");
        let enc = dir.path().join("p.txt.age");
        std::fs::write(&plain, "from reference tool ✓").unwrap();
        let ok = run_with_pty(
            &bin,
            &["-p", "-o", enc.to_str().unwrap(), plain.to_str().unwrap()],
            &["ref pass", "ref pass"],
        );
        assert!(ok, "{tool} failed to encrypt");
        let ct = std::fs::read(&enc).unwrap();
        match crypto::decrypt(&Passphrase::from("ref pass"), &ct, 1 << 20) {
            Ok(pt) => assert_eq!(&*pt, "from reference tool ✓".as_bytes()),
            // rage benchmarks its work factor; on very fast machines it may
            // exceed our documented maximum. That rejection is deliberate.
            Err(e) => assert_eq!(e.code(), ErrorCode::ExcessiveWorkFactor, "{tool}"),
        }
    }
}

#[test]
fn whole_vault_files_are_plain_age_files() {
    // A vault file decrypts with the reference tool to the documented JSON.
    let Some(bin) = tool_or_skip("age") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("v.nvault.age");
    let mut opts = test_opts(&dir.path().join("data"));
    opts.work = WorkFactor::DEFAULT;
    let s = vault_core::session::Session::create(
        &vault_path,
        Passphrase::from("vault pass"),
        vault_core::model::Vault::new_with_welcome(),
        opts,
    )
    .unwrap();
    drop(s);
    let out = dir.path().join("payload.json");
    assert!(run_with_pty(
        &bin,
        &[
            "-d",
            "-o",
            out.to_str().unwrap(),
            vault_path.to_str().unwrap()
        ],
        &["vault pass"]
    ));
    let json = std::fs::read_to_string(out).unwrap();
    assert!(json.starts_with("{\"format\":\"nilavar-vault\",\"schema_version\":1,"));
}
