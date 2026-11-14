//! Pure editor text operations (list continuation, indent/outdent, line
//! helpers). Positions are **char** indices (as used by the editor widget);
//! edits are returned as byte ranges so callers can apply them to a
//! zeroizing buffer and keep the widget's undo history coherent.

use std::ops::Range;

/// Indentation unit inserted by Tab / indent.
pub const INDENT: &str = "    ";

/// A single replacement plus the selection to apply afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// Byte range to replace.
    pub range: Range<usize>,
    /// Replacement text.
    pub insert: String,
    /// New selection as char indices `(anchor, cursor)`.
    pub selection: (usize, usize),
}

impl Edit {
    /// Applies the edit to a `String` (used by tests and simple callers).
    pub fn apply_to(&self, s: &mut String) {
        s.replace_range(self.range.clone(), &self.insert);
    }
}

/// Converts a char index into a byte offset (clamped to the end).
pub fn char_to_byte(text: &str, ci: usize) -> usize {
    text.char_indices()
        .nth(ci)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// Converts a byte offset into a char index.
pub fn byte_to_char(text: &str, bi: usize) -> usize {
    text[..bi.min(text.len())].chars().count()
}

/// Byte offset of the start of the line containing byte `b`.
pub fn line_start(text: &str, b: usize) -> usize {
    text[..b].rfind('\n').map(|i| i + 1).unwrap_or(0)
}

/// Byte offset of the end (excluding `\n`) of the line containing byte `b`.
pub fn line_end(text: &str, b: usize) -> usize {
    text[b..].find('\n').map(|i| b + i).unwrap_or(text.len())
}

/// 1-based line and column (chars) for a char index.
pub fn line_col(text: &str, ci: usize) -> (usize, usize) {
    let b = char_to_byte(text, ci);
    let line = text[..b].matches('\n').count() + 1;
    let col = text[line_start(text, b)..b].chars().count() + 1;
    (line, col)
}

/// Char index at the start of 1-based `line` (clamped).
pub fn line_to_char(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut seen = 1;
    for (ci, c) in text.chars().enumerate() {
        if c == '\n' {
            seen += 1;
            if seen == line {
                return ci + 1;
            }
        }
    }
    text.chars().count()
}

struct ListPrefix<'a> {
    indent: &'a str,
    marker: Marker,
    task: bool,
    /// Byte length of the whole prefix (indent + marker + checkbox + space).
    len: usize,
}

enum Marker {
    Bullet(char),
    Ordered(u64, char),
    Quote,
}

fn parse_list_prefix(line: &str) -> Option<ListPrefix<'_>> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    let (marker, mut used) =
        if let Some(c) = rest.chars().next().filter(|c| matches!(c, '-' | '*' | '+')) {
            (Marker::Bullet(c), 1)
        } else if rest.starts_with('>') {
            (Marker::Quote, 1)
        } else {
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 || digits > 9 {
                return None;
            }
            let delim = rest[digits..]
                .chars()
                .next()
                .filter(|c| matches!(c, '.' | ')'))?;
            let n = rest[..digits].parse().ok()?;
            (Marker::Ordered(n, delim), digits + 1)
        };
    let after = &rest[used..];
    if !after.starts_with(' ') {
        // "-" alone at end of line still counts as an (empty) item.
        if after.is_empty() && !matches!(marker, Marker::Quote) {
            return Some(ListPrefix {
                indent,
                marker,
                task: false,
                len: indent_len + used,
            });
        }
        if matches!(marker, Marker::Quote) {
            return Some(ListPrefix {
                indent,
                marker,
                task: false,
                len: indent_len + used,
            });
        }
        return None;
    }
    used += 1;
    let mut task = false;
    if !matches!(marker, Marker::Quote) {
        let a = &rest[used..];
        if a.starts_with("[ ] ") || a.starts_with("[x] ") || a.starts_with("[X] ") {
            task = true;
            used += 4;
        }
    }
    Some(ListPrefix {
        indent,
        marker,
        task,
        len: indent_len + used,
    })
}

/// Handles Enter at `cursor` (char index, empty selection) inside a list
/// item, task item or block quote. Returns `None` when default newline
/// behavior should apply.
pub fn continue_list(text: &str, cursor: usize) -> Option<Edit> {
    let b = char_to_byte(text, cursor);
    let ls = line_start(text, b);
    let le = line_end(text, b);
    let line = &text[ls..le];
    let p = parse_list_prefix(line)?;
    if b < ls + p.len {
        return None;
    }
    let content_empty = line[p.len..].trim().is_empty();
    if content_empty {
        // Enter on an empty item ends the list: remove the marker.
        return Some(Edit {
            range: ls..le,
            insert: String::new(),
            selection: (byte_to_char(text, ls), byte_to_char(text, ls)),
        });
    }
    let mut next = String::from("\n");
    next.push_str(p.indent);
    match p.marker {
        Marker::Bullet(c) => {
            next.push(c);
            next.push(' ');
        }
        Marker::Ordered(n, d) => {
            next.push_str(&(n.saturating_add(1)).to_string());
            next.push(d);
            next.push(' ');
        }
        Marker::Quote => next.push_str("> "),
    }
    if p.task {
        next.push_str("[ ] ");
    }
    let new_cursor = cursor + next.chars().count();
    Some(Edit {
        range: b..b,
        insert: next,
        selection: (new_cursor, new_cursor),
    })
}

/// Byte range of the full lines touched by the char selection `a..b`.
fn touched_lines(text: &str, a: usize, b: usize) -> Range<usize> {
    let (a, b) = (a.min(b), a.max(b));
    let sb = char_to_byte(text, a);
    let mut eb = char_to_byte(text, b);
    // A selection ending at column 0 of a following line doesn't include it.
    if b > a && eb > 0 && text.as_bytes()[eb - 1] == b'\n' {
        eb -= 1;
    }
    line_start(text, sb)..line_end(text, eb)
}

/// Indents every line touched by the selection.
pub fn indent(text: &str, anchor: usize, cursor: usize) -> Edit {
    let r = touched_lines(text, anchor, cursor);
    let block = &text[r.clone()];
    let mut out = String::with_capacity(block.len() + INDENT.len() * 4);
    let mut lines = 0;
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(INDENT);
        out.push_str(line);
        lines += 1;
    }
    let shift = INDENT.chars().count();
    let first_shift = shift;
    let total_shift = shift * lines;
    let (lo, hi) = order(anchor, cursor);
    let sel = (lo + first_shift, hi + total_shift);
    Edit {
        range: r,
        insert: out,
        selection: if anchor <= cursor {
            sel
        } else {
            (sel.1, sel.0)
        },
    }
}

/// Removes up to one indent unit (or one tab) from every touched line.
pub fn outdent(text: &str, anchor: usize, cursor: usize) -> Edit {
    let r = touched_lines(text, anchor, cursor);
    let block = &text[r.clone()];
    let mut out = String::with_capacity(block.len());
    let mut removed_total = 0usize;
    let mut removed_first = 0usize;
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let n = if line.starts_with('\t') {
            1
        } else {
            line.chars()
                .take(INDENT.len())
                .take_while(|c| *c == ' ')
                .count()
        };
        if i == 0 {
            removed_first = n;
        }
        removed_total += n;
        out.push_str(&line[n..]);
    }
    let (lo, hi) = order(anchor, cursor);
    let line_start_char = byte_to_char(text, r.start);
    let lo_new = lo
        .saturating_sub(removed_first)
        .max(line_start_char.min(lo));
    let hi_new = hi.saturating_sub(removed_total).max(lo_new);
    Edit {
        range: r,
        insert: out,
        selection: if anchor <= cursor {
            (lo_new, hi_new)
        } else {
            (hi_new, lo_new)
        },
    }
}

fn order(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// Toggles a task checkbox (`[ ]` ↔ `[x]`) on the line at `cursor`.
pub fn toggle_task(text: &str, cursor: usize) -> Option<Edit> {
    let b = char_to_byte(text, cursor);
    let ls = line_start(text, b);
    let le = line_end(text, b);
    let line = &text[ls..le];
    let p = parse_list_prefix(line)?;
    if !p.task {
        return None;
    }
    let box_start = ls + p.len - 4;
    let checked = &text[box_start..box_start + 3] != "[ ]";
    Some(Edit {
        range: box_start..box_start + 3,
        insert: if checked { "[ ]".into() } else { "[x]".into() },
        selection: (cursor, cursor),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, e: &Edit) -> String {
        let mut s = text.to_owned();
        e.apply_to(&mut s);
        s
    }

    #[test]
    fn continues_bullets_numbers_tasks_quotes() {
        let t = "- one";
        let e = continue_list(t, 5).unwrap();
        assert_eq!(apply(t, &e), "- one\n- ");
        assert_eq!(e.selection, (8, 8));

        let t = "  9. nine";
        assert_eq!(apply(t, &continue_list(t, 9).unwrap()), "  9. nine\n  10. ");

        let t = "- [x] done";
        assert_eq!(
            apply(t, &continue_list(t, 10).unwrap()),
            "- [x] done\n- [ ] "
        );

        let t = "> quoted";
        assert_eq!(apply(t, &continue_list(t, 8).unwrap()), "> quoted\n> ");

        assert!(continue_list("plain text", 5).is_none());
        assert!(continue_list("-not a list", 3).is_none());
    }

    #[test]
    fn empty_item_ends_list() {
        let t = "- a\n- ";
        let e = continue_list(t, 6).unwrap();
        assert_eq!(apply(t, &e), "- a\n");
        assert_eq!(e.selection, (4, 4));
    }

    #[test]
    fn unicode_cursor_positions() {
        let t = "- 🔐 clé";
        let n = t.chars().count();
        let e = continue_list(t, n).unwrap();
        assert_eq!(apply(t, &e), "- 🔐 clé\n- ");
        assert_eq!(e.selection.0, n + 3);
        assert_eq!(line_col("ab\ncdé", 6), (2, 4));
        assert_eq!(line_to_char("a\nb\nc", 3), 4);
    }

    #[test]
    fn indent_and_outdent_selection() {
        let t = "a\nb\nc";
        let e = indent(t, 0, 3);
        assert_eq!(apply(t, &e), "    a\n    b\nc");
        let t2 = apply(t, &e);
        let e2 = outdent(&t2, 0, 11);
        assert_eq!(apply(&t2, &e2), "a\nb\nc");
        let t3 = "\tx";
        assert_eq!(apply(t3, &outdent(t3, 1, 1)), "x");
        // Selection ending at column 0 of the next line doesn't indent it.
        let e4 = indent("a\nb", 0, 2);
        assert_eq!(apply("a\nb", &e4), "    a\nb");
    }

    #[test]
    fn task_toggle() {
        let t = "- [ ] todo";
        assert_eq!(apply(t, &toggle_task(t, 7).unwrap()), "- [x] todo");
        let t = "- [x] todo";
        assert_eq!(apply(t, &toggle_task(t, 7).unwrap()), "- [ ] todo");
    }
}
