//! Markdown preview parsing never panics and never yields remote fetches.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let p = vault_core::markdown::parse(s);
        for l in &p.links {
            let _ = vault_core::markdown::classify_link(l);
        }
    }
});
