//! Measures the cost of an encrypted save/open at the production work
//! factor, for documenting autosave overhead.
//!
//!     cargo run --release -p vault-core --example kdf_bench

#![allow(clippy::print_stdout)]

use std::time::Instant;

use vault_core::crypto::{self, WorkFactor};
use vault_core::Passphrase;

fn main() {
    let pass = Passphrase::from("benchmark passphrase (synthetic)");
    for (label, size) in [("10 KiB", 10 * 1024), ("1 MiB", 1 << 20), ("8 MiB", 8 << 20), ("32 MiB", 32 << 20)] {
        let plain = vec![b'x'; size];
        let mut enc_ms = Vec::new();
        let mut dec_ms = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let ct = crypto::encrypt(&pass, WorkFactor::DEFAULT, &plain).unwrap();
            enc_ms.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let pt = crypto::decrypt(&pass, &ct, size).unwrap();
            dec_ms.push(t.elapsed().as_secs_f64() * 1e3);
            assert_eq!(pt.len(), size);
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        println!(
            "{label:>7}: encrypt (scrypt 2^18 + ChaCha20-Poly1305) {:7.1} ms, decrypt {:7.1} ms",
            med(&mut enc_ms),
            med(&mut dec_ms)
        );
    }
}
