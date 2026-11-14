//! Renders the inert Markdown preview model with native egui widgets.
//! Links are never followed on click: clicking requests an explicit
//! confirmation dialog that shows the destination.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, RichText, Stroke};
use vault_core::markdown::{Block, Preview, Span, SpanKind};

use crate::highlight::PALETTE;
use crate::theme;

/// What the user asked for while viewing the preview.
pub enum PreviewAction {
    /// Show the link dialog for this destination.
    Link(String),
    /// Toggle the n-th task checkbox (document order).
    ToggleTask(usize),
}

pub fn show(ui: &mut egui::Ui, p: &Preview, base_size: f32) -> Option<PreviewAction> {
    let mut action = None;
    let mut task_counter = 0usize;
    ui.spacing_mut().item_spacing.y = 8.0;
    for b in &p.blocks {
        block(ui, b, p, base_size, &mut action, &mut task_counter, 0);
    }
    action
}

fn spans_job(spans: &[Span], size: f32, base: Color32, bold: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for s in spans {
        let font = if s.style.code {
            theme::mono(size * 0.92)
        } else if s.style.strong || bold {
            theme::ui_bold(size)
        } else {
            egui::FontId::proportional(size)
        };
        let mut color = base;
        let mut fmt = TextFormat::simple(font, color);
        match s.kind {
            SpanKind::Link(_) => {
                color = theme::CYAN;
                fmt.color = color;
                fmt.underline = Stroke::new(1.0, theme::CYAN.gamma_multiply(0.6));
            }
            SpanKind::Image(_) => {
                fmt.color = theme::AMBER;
            }
            SpanKind::RawHtml => {
                fmt.font_id = theme::mono(size * 0.9);
                fmt.color = theme::MUTED;
            }
            _ => {}
        }
        if s.style.code {
            fmt.background = PALETTE.code_bg;
            fmt.color = PALETTE.code;
        }
        if s.style.emphasis {
            fmt.italics = true;
        }
        if s.style.strike {
            fmt.strikethrough = Stroke::new(1.0, base);
        }
        job.append(&s.text, 0.0, fmt);
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

fn paragraph(
    ui: &mut egui::Ui,
    spans: &[Span],
    p: &Preview,
    size: f32,
    color: Color32,
    bold: bool,
    action: &mut Option<PreviewAction>,
) {
    let mut job = spans_job(spans, size, color, bold);
    job.wrap.max_width = ui.available_width();
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let resp = ui.add(egui::Label::new(galley.clone()).sense(egui::Sense::click()));
    // Find which link (if any) was clicked: map pointer to char index.
    let links: Vec<(usize, usize, usize)> = {
        let mut out = Vec::new();
        let mut pos = 0usize;
        for s in spans {
            let n = s.text.chars().count();
            if let SpanKind::Link(i) | SpanKind::Image(i) = s.kind {
                out.push((pos, pos + n, i));
            }
            pos += n;
        }
        out
    };
    if !links.is_empty() {
        if resp.hovered()
            && let Some(ptr) = resp.hover_pos()
        {
            let c = galley.cursor_from_pos(ptr - resp.rect.min).index.0;
            if let Some((_, _, i)) = links.iter().find(|(a, b, _)| c >= *a && c < *b) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                resp.clone()
                    .on_hover_text(format!("{}  (click for options)", p.links[*i]));
            }
        }
        if resp.clicked()
            && let Some(ptr) = resp.interact_pointer_pos()
        {
            let c = galley.cursor_from_pos(ptr - resp.rect.min).index.0;
            if let Some((_, _, i)) = links.iter().find(|(a, b, _)| c >= *a && c < *b) {
                *action = Some(PreviewAction::Link(p.links[*i].clone()));
            }
        }
    }
}

fn block(
    ui: &mut egui::Ui,
    b: &Block,
    p: &Preview,
    size: f32,
    action: &mut Option<PreviewAction>,
    tasks: &mut usize,
    depth: usize,
) {
    match b {
        Block::Heading(level, spans) => {
            let scale = match level {
                1 => 1.6,
                2 => 1.35,
                3 => 1.18,
                _ => 1.05,
            };
            ui.add_space(if *level <= 2 { 6.0 } else { 2.0 });
            let color = if *level == 1 {
                theme::GREEN
            } else {
                PALETTE.heading
            };
            paragraph(ui, spans, p, size * scale, color, true, action);
            if *level <= 2 {
                let r = ui.min_rect();
                let y = ui.cursor().top() - 3.0;
                ui.painter().hline(
                    r.x_range(),
                    y,
                    Stroke::new(1.0, theme::BORDER.gamma_multiply(0.8)),
                );
            }
        }
        Block::Paragraph(spans) => paragraph(ui, spans, p, size, theme::TEXT, false, action),
        Block::Code { lang, text } => {
            egui::Frame::new()
                .fill(PALETTE.code_bg)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if !lang.is_empty() {
                        ui.label(
                            RichText::new(lang)
                                .font(theme::mono(size * 0.75))
                                .color(theme::MUTED),
                        );
                    }
                    let t = text.strip_suffix('\n').unwrap_or(text);
                    ui.add(
                        egui::Label::new(
                            RichText::new(t)
                                .font(theme::mono(size * 0.9))
                                .color(PALETTE.code),
                        )
                        .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                });
        }
        Block::Quote(inner) => {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 14,
                    right: 0,
                    top: 2,
                    bottom: 2,
                })
                .show(ui, |ui| {
                    let top = ui.cursor().top();
                    for x in inner {
                        block(ui, x, p, size, action, tasks, depth + 1);
                    }
                    let bottom = ui.min_rect().bottom();
                    let x = ui.min_rect().left() - 9.0;
                    ui.painter().vline(
                        x,
                        top..=bottom,
                        Stroke::new(2.0, theme::GREEN.gamma_multiply(0.5)),
                    );
                });
        }
        Block::List { start, items } => {
            for (n, item) in items.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    ui.add_space(4.0 + 6.0 * depth.min(4) as f32);
                    let marker = match (item.task, start) {
                        (Some(done), _) => {
                            let idx = *tasks;
                            *tasks += 1;
                            let icon = if done {
                                crate::icons::CHECK_SQUARE
                            } else {
                                crate::icons::SQUARE
                            };
                            let col = if done { theme::GREEN } else { theme::MUTED };
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(icon).color(col).size(size))
                                        .frame(false),
                                )
                                .on_hover_text("Toggle task in the document")
                                .clicked()
                            {
                                *action = Some(PreviewAction::ToggleTask(idx));
                            }
                            None
                        }
                        (None, Some(s)) => Some(format!("{}.", s + n as u64)),
                        (None, None) => Some("•".to_owned()),
                    };
                    if let Some(m) = marker {
                        ui.label(
                            RichText::new(m)
                                .color(theme::GREEN.gamma_multiply(0.85))
                                .size(size),
                        );
                    }
                    ui.vertical(|ui| {
                        for x in &item.blocks {
                            block(ui, x, p, size, action, tasks, depth + 1);
                        }
                    });
                });
            }
        }
        Block::Rule => {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
        }
        Block::Table { header, rows } => {
            egui::Frame::new()
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(4)
                .inner_margin(8)
                .show(ui, |ui| {
                    egui::Grid::new(ui.next_auto_id())
                        .striped(true)
                        .spacing(egui::vec2(16.0, 6.0))
                        .show(ui, |ui| {
                            for c in header {
                                paragraph(ui, c, p, size, theme::GREEN, true, action);
                            }
                            ui.end_row();
                            for r in rows {
                                for c in r {
                                    paragraph(ui, c, p, size, theme::TEXT, false, action);
                                }
                                ui.end_row();
                            }
                        });
                });
        }
        Block::Html(h) => {
            egui::Frame::new()
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(4)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("raw HTML (not rendered)")
                            .font(theme::mono(size * 0.72))
                            .color(theme::MUTED),
                    );
                    ui.label(
                        RichText::new(h.trim_end())
                            .font(theme::mono(size * 0.85))
                            .color(theme::MUTED),
                    );
                });
        }
    }
}

/// Finds the byte offset of the n-th task checkbox ("[ ]"/"[x]") in list
/// items of `text`, for toggling from the preview.
pub fn nth_task_offset(text: &str, n: usize) -> Option<usize> {
    let mut seen = 0;
    let mut in_fence = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if !in_fence {
            let indent = line.len() - t.len();
            let marker = if t.len() >= 2
                && matches!(t.as_bytes()[0], b'-' | b'*' | b'+')
                && t.as_bytes()[1] == b' '
            {
                Some(2)
            } else {
                let d = t.bytes().take_while(u8::is_ascii_digit).count();
                (d > 0
                    && t.len() > d + 1
                    && matches!(t.as_bytes()[d], b'.' | b')')
                    && t.as_bytes()[d + 1] == b' ')
                    .then_some(d + 2)
            };
            if let Some(m) = marker {
                let r = &t[m..];
                if r.starts_with("[ ]") || r.starts_with("[x]") || r.starts_with("[X]") {
                    if seen == n {
                        return Some(offset + indent + m);
                    }
                    seen += 1;
                }
            }
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_task_offsets() {
        let t = "- [ ] a\n```\n- [ ] not\n```\n1. [x] b\n";
        assert_eq!(nth_task_offset(t, 0), Some(2));
        let o = nth_task_offset(t, 1).unwrap();
        assert_eq!(&t[o..o + 3], "[x]");
        assert_eq!(nth_task_offset(t, 2), None);
    }
}
