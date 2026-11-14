//! Revision/history validation: a byte-driven sequence of edits, saves,
//! restores, trash and folder operations must always leave a vault that
//! passes validation and survives an encode/decode round trip.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vault_core::history::{record_save, SaveTrigger};
use vault_core::model::Vault;

fuzz_target!(|data: &[u8]| {
    let mut v = Vault::new();
    let mut now = 0i64;
    for chunk in data.chunks(3).take(400) {
        let op = chunk[0] % 10;
        let a = *chunk.get(1).unwrap_or(&0) as usize;
        let b = *chunk.get(2).unwrap_or(&0);
        now += i64::from(b) * 7;
        match op {
            0 => {
                let _ = v.create_document(&format!("d{a}"), None);
            }
            1 => {
                let n = v.documents.len().max(1);
                if let Some(d) = v.documents.get_mut(a % n) {
                    let t = format!("{}{}", d.text.expose(), char::from(b'a' + b % 26));
                    d.text.set(&t);
                }
            }
            2 => {
                let _ = record_save(&mut v, now, if b % 2 == 0 { SaveTrigger::Auto } else { SaveTrigger::Explicit });
            }
            3 => {
                if let Some(d) = v.documents.get(a % v.documents.len().max(1)) {
                    let id = d.id;
                    if let Some(h) = d.history.get(usize::from(b) % d.history.len().max(1)).map(|h| h.id) {
                        let _ = v.restore_history_entry(id, h);
                    }
                }
            }
            4 => {
                if let Some(id) = v.documents.get(a % v.documents.len().max(1)).map(|d| d.id) {
                    let _ = v.trash_document(id);
                }
            }
            5 => {
                if let Some(id) = v.trash.get(a % v.trash.len().max(1)).map(|t| t.document.id) {
                    let _ = v.restore_from_trash(id);
                }
            }
            6 => {
                let parent = v.folders.get(a % v.folders.len().max(1)).map(|f| f.id);
                let _ = v.create_folder(&format!("f{b}"), if b % 2 == 0 { None } else { parent });
            }
            7 => {
                if let Some(f) = v.folders.get(a % v.folders.len().max(1)).map(|f| f.id) {
                    let _ = v.delete_folder(f);
                }
            }
            8 => {
                let doc = v.documents.get(a % v.documents.len().max(1)).map(|d| d.id);
                let folder = v.folders.get(usize::from(b) % v.folders.len().max(1)).map(|f| f.id);
                if let Some(d) = doc {
                    let _ = v.move_document(d, folder);
                }
            }
            _ => {
                if let Some(id) = v.documents.get(a % v.documents.len().max(1)).map(|d| d.id) {
                    let _ = v.purge_history(id);
                }
            }
        }
    }
    vault_core::format::validate(&v).expect("operations must preserve invariants");
    let bytes = vault_core::format::encode(&v).expect("encode");
    let back = vault_core::format::decode(&bytes).expect("decode");
    assert_eq!(back.total_history_entries(), v.total_history_entries());
});
