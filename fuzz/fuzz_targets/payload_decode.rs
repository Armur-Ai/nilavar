//! Decrypted-payload parsing: arbitrary bytes must either decode into a
//! vault that re-encodes and re-decodes identically, or fail with an error.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(v) = vault_core::format::decode(data) {
        let bytes = vault_core::format::encode(&v).expect("decoded vault must re-encode");
        let again = vault_core::format::decode(&bytes).expect("re-encoded vault must decode");
        assert_eq!(again.documents.len(), v.documents.len());
        assert_eq!(&*vault_core::format::encode(&again).unwrap(), &*bytes);
    }
});
