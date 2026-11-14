//! Editor text operations must produce edits on char boundaries for any
//! text and cursor positions.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vault_core::textops;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let (a, b) = (usize::from(data[0]), usize::from(data[1]));
    let Ok(s) = std::str::from_utf8(&data[2..]) else { return };
    let n = s.chars().count();
    let (a, b) = (a % (n + 1), b % (n + 1));
    let mut edits = vec![textops::indent(s, a, b), textops::outdent(s, a, b)];
    edits.extend(textops::continue_list(s, b));
    edits.extend(textops::toggle_task(s, b));
    for e in edits {
        assert!(s.is_char_boundary(e.range.start) && s.is_char_boundary(e.range.end));
        let mut t = s.to_owned();
        e.apply_to(&mut t);
        let m = t.chars().count();
        assert!(e.selection.0 <= m && e.selection.1 <= m);
    }
    let _ = vault_core::search::find_all(s, "ab", false);
});
