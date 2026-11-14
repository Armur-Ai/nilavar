//! Local, restrained Markdown syntax highlighting for the editor.
//!
//! Produces an egui `LayoutJob` from the text. Highlighting is line based
//! (headings, lists, quotes, fences, rules) plus a few inline spans (code,
//! emphasis, links). It never interprets HTML or fetches anything.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Stroke};

use crate::theme;

#[derive(Clone, Copy)]
pub struct Palette {
    pub text: Color32,
    pub muted: Color32,
    pub marker: Color32,
    pub heading: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub link: Color32,
    pub quote: Color32,
}

pub const PALETTE: Palette = Palette {
    text: theme::TEXT,
    muted: theme::MUTED,
    marker: Color32::from_rgb(0x2F, 0xC9, 0x74),
    heading: Color32::from_rgb(0xEE, 0xFA, 0xF2),
    code: Color32::from_rgb(0x9F, 0xE3, 0xD6),
    code_bg: Color32::from_rgb(0x0C, 0x17, 0x14),
    link: theme::CYAN,
    quote: Color32::from_rgb(0xA6, 0xBD, 0xB1),
};

struct Ctx<'a> {
    job: &'a mut LayoutJob,
    size: f32,
}

impl Ctx<'_> {
    fn push(&mut self, s: &str, font: FontId, color: Color32, bg: Color32, underline: bool) {
        if s.is_empty() {
            return;
        }
        let mut fmt = TextFormat::simple(font, color);
        fmt.background = bg;
        if underline {
            fmt.underline = Stroke::new(1.0, color.gamma_multiply(0.6));
        }
        self.job.append(s, 0.0, fmt);
    }
    fn plain(&mut self, s: &str, color: Color32) {
        let f = theme::mono(self.size);
        self.push(s, f, color, Color32::TRANSPARENT, false);
    }
    fn bold(&mut self, s: &str, color: Color32) {
        let f = theme::mono_bold(self.size);
        self.push(s, f, color, Color32::TRANSPARENT, false);
    }
}

/// Builds a plain (unhighlighted) job.
pub fn plain_job(text: &str, size: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat::simple(theme::mono(size), PALETTE.text),
    );
    job
}

/// Builds a Markdown-highlighted job.
pub fn markdown_job(text: &str, size: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let p = PALETTE;
    let mut c = Ctx {
        job: &mut job,
        size,
    };
    let mut in_fence: Option<String> = None;
    for line in text.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let trimmed = body.trim_start();
        let indent = &body[..body.len() - trimmed.len()];

        // Fenced code blocks.
        let fence_marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        if let Some(open) = in_fence.as_deref() {
            if fence_marker == Some(open) {
                c.plain(body, p.muted);
                in_fence = None;
            } else {
                let f = theme::mono(size);
                c.push(body, f, p.code, p.code_bg, false);
            }
            c.plain(nl, p.text);
            continue;
        }
        if let Some(m) = fence_marker {
            in_fence = Some(m.to_owned());
            c.plain(indent, p.text);
            c.plain(&trimmed[..3], p.muted);
            c.plain(&trimmed[3..], p.marker);
            c.plain(nl, p.text);
            continue;
        }

        // Headings.
        let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
        if (1..=6).contains(&hashes)
            && (trimmed.len() == hashes || trimmed[hashes..].starts_with(' '))
        {
            c.plain(indent, p.text);
            c.plain(&trimmed[..hashes], p.marker);
            c.bold(&trimmed[hashes..], p.heading);
            c.plain(nl, p.text);
            continue;
        }

        // Thematic break.
        let compact: String = trimmed.chars().filter(|ch| !ch.is_whitespace()).collect();
        if compact.len() >= 3
            && (compact.chars().all(|ch| ch == '-')
                || compact.chars().all(|ch| ch == '*')
                || compact.chars().all(|ch| ch == '_'))
        {
            c.plain(body, p.muted);
            c.plain(nl, p.text);
            continue;
        }

        // Block quotes.
        if let Some(rest) = trimmed.strip_prefix('>') {
            c.plain(indent, p.text);
            c.plain(">", p.marker);
            inline(&mut c, rest, p.quote);
            c.plain(nl, p.text);
            continue;
        }

        // List items (with optional task box).
        let marker_len = list_marker_len(trimmed);
        if marker_len > 0 {
            c.plain(indent, p.text);
            c.plain(&trimmed[..marker_len], p.marker);
            let rest = &trimmed[marker_len..];
            let (task, rest) =
                if rest.starts_with("[ ] ") || rest.starts_with("[x] ") || rest.starts_with("[X] ")
                {
                    (Some(&rest[..3]), &rest[3..])
                } else {
                    (None, rest)
                };
            if let Some(t) = task {
                let col = if t == "[ ]" { p.muted } else { theme::GREEN };
                c.plain(t, col);
                let done = t != "[ ]";
                inline(&mut c, rest, if done { p.muted } else { p.text });
            } else {
                inline(&mut c, rest, p.text);
            }
            c.plain(nl, p.text);
            continue;
        }

        // Indented code (4 spaces / tab) outside lists.
        if body.starts_with("    ") || body.starts_with('\t') {
            let f = theme::mono(size);
            c.push(body, f, p.code, Color32::TRANSPARENT, false);
            c.plain(nl, p.text);
            continue;
        }

        inline(&mut c, body, p.text);
        c.plain(nl, p.text);
    }
    job
}

fn list_marker_len(s: &str) -> usize {
    let b = s.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return 2;
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits)
        && b.len() > digits + 1
        && matches!(b[digits], b'.' | b')')
        && b[digits + 1] == b' '
    {
        return digits + 2;
    }
    0
}

/// Inline spans: `code`, **strong**, *em*/_em_, [link](dest).
fn inline(c: &mut Ctx<'_>, s: &str, base: Color32) {
    let p = PALETTE;
    let mut rest = s;
    while !rest.is_empty() {
        // Next special char.
        let Some(i) = rest.find(['`', '*', '_', '[']) else {
            c.plain(rest, base);
            break;
        };
        c.plain(&rest[..i], base);
        rest = &rest[i..];
        if let Some(r) = rest.strip_prefix('`') {
            if let Some(end) = r.find('`') {
                let f = theme::mono(c.size);
                c.push("`", f.clone(), p.muted, p.code_bg, false);
                c.push(&r[..end], f.clone(), p.code, p.code_bg, false);
                c.push("`", f, p.muted, p.code_bg, false);
                rest = &r[end + 1..];
                continue;
            }
        } else if let Some(r) = rest.strip_prefix("**").or_else(|| rest.strip_prefix("__")) {
            let delim = &rest[..2];
            if let Some(end) = r.find(delim).filter(|e| *e > 0) {
                c.plain(delim, p.muted);
                c.bold(&r[..end], p.heading);
                c.plain(delim, p.muted);
                rest = &r[end + 2..];
                continue;
            }
        } else if rest.starts_with('*') || rest.starts_with('_') {
            let delim = &rest[..1];
            let r = &rest[1..];
            if let Some(end) = r.find(delim).filter(|e| *e > 0 && !r.starts_with(' ')) {
                c.plain(delim, p.muted);
                c.plain(&r[..end], Color32::from_rgb(0xC4, 0xF2, 0xD8));
                c.plain(delim, p.muted);
                rest = &r[end + 1..];
                continue;
            }
        } else if let Some(r) = rest.strip_prefix('[')
            && let Some(close) = r.find("](")
            && let Some(end) = r[close + 2..].find(')')
        {
            let text = &r[..close];
            let dest = &r[close + 2..close + 2 + end];
            if !text.contains('\n') {
                c.plain("[", p.muted);
                let f = theme::mono(c.size);
                c.push(text, f, p.link, Color32::TRANSPARENT, true);
                c.plain("](", p.muted);
                c.plain(dest, p.muted);
                c.plain(")", p.muted);
                rest = &r[close + 2 + end + 1..];
                continue;
            }
        }
        // Not a span: emit the char literally.
        let ch_len = rest.chars().next().map(char::len_utf8).unwrap_or(1);
        c.plain(&rest[..ch_len], base);
        rest = &rest[ch_len..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(job: &LayoutJob) -> &str {
        &job.text
    }

    #[test]
    fn highlighting_preserves_text_exactly() {
        for src in [
            "# Title\n\n- [x] done\n- item with `code` and **bold**\n> quote\n```\nfn x() {}\n```\n---\n[link](https://example.invalid) _em_ 1. not\n",
            "unicode ✓ 🔐 é\n\t tab\n    indented\n* * *\n",
            "",
            "[broken link(",
            "**unterminated",
        ] {
            let job = markdown_job(src, 14.0);
            assert_eq!(text_of(&job), src);
        }
    }
}
