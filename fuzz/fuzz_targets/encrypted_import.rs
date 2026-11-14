//! Bounded encrypted import: arbitrary bytes as an age file must never
//! panic, and plaintext output is bounded.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vault_core::Passphrase;

fuzz_target!(|data: &[u8]| {
    let _ = vault_core::crypto::inspect_header(data);
    // Skip inputs that would legitimately run an expensive KDF; header
    // parsing and the work-factor bound are still exercised above.
    let text = String::from_utf8_lossy(&data[..data.len().min(256)]);
    if text.contains("scrypt") {
        return;
    }
    let pass = Passphrase::from("fuzz");
    if let Ok(p) = vault_core::crypto::decrypt(&pass, data, 4096) {
        assert!(p.len() <= 4096);
    }
});
