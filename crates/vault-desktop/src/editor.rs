//! The document editor: egui `TextEdit` over a zeroizing buffer, with a line
//! number gutter that stays correct under soft wrap, current-line highlight,
//! Markdown highlighting, list continuation, indent/outdent and find/replace.
//!
//! Plaintext lifetimes reviewed here:
//! * the text lives in the document's `SecretText`; edits go through
//!   [`EditorBuffer`], which zeroizes old allocations when growing;
//! * egui's `TextEditState` (cursor + undo history, which contains text
//!   snapshots) lives in egui memory; it is never persisted (eframe's
//!   `persistence` feature is disabled) and is wiped on lock by resetting
//!   egui memory;
//! * laid-out galleys contain copies of the text; our layout cache is
//!   dropped on lock and egui's galley cache evicts galleys not used in the
//!   previous frame (the lock screen shows no document text). These copies
//!   are freed but not zeroized — a documented residual.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::ops::Range;
use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, CharIndex};
use egui::{Align, Color32, Galley, Id, Key, Modifiers, Rect, Sense, Stroke, TextBuffer};
use vault_core::SecretText;
use vault_core::model::DocMode;
use vault_core::search;
use vault_core::textops::{self, Edit};
use zeroize::Zeroizing;

use crate::highlight;
use crate::theme;

/// `TextBuffer` adapter that routes egui's edits through `SecretText`'s
/// zeroizing insert/remove operations.
pub struct EditorBuffer<'a> {
    pub text: &'a mut SecretText,
    pub changed: bool,
}

impl TextBuffer for EditorBuffer<'_> {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.text.expose()
    }

    fn insert_text(&mut self, text: &str, char_index: CharIndex) -> usize {
        let at = textops::char_to_byte(self.text.expose(), char_index.0);
        self.text.insert_str(at, text);
        self.changed = true;
        text.chars().count()
    }

    fn delete_char_range(&mut self, char_range: Range<CharIndex>) {
        let s = self.text.expose();
        let a = textops::char_to_byte(s, char_range.start.0);
        let b = textops::char_to_byte(s, char_range.end.0);
        if a < b {
            self.text.remove_range(a, b);
            self.changed = true;
        }
    }

    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<EditorBuffer<'static>>()
    }
}

/// Find/replace bar state (plaintext; cleared on lock).
#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub show_replace: bool,
    pub query: Zeroizing<String>,
    pub replacement: Zeroizing<String>,
    pub case_sensitive: bool,
    pub current: usize,
    pub focus_query: bool,
    pub matches: Vec<Range<usize>>,
    matches_key: Option<(u64, u64, bool)>,
}

impl FindState {
    /// Forces match recomputation.
    pub fn invalidate(&mut self) {
        self.matches_key = None;
    }
}

/// Per-session editor state (dropped on lock).
pub struct EditorState {
    hasher: RandomState,
    cache: Option<LayoutCache>,
    pub find: FindState,
    /// Pending selection to apply to a document's editor (char indices).
    pub pending_selection: Option<(usize, usize)>,
    pub scroll_to_cursor: bool,
    pub request_focus: bool,
    last_cursor_row: Option<Rect>,
    pub cursor: (usize, usize),
    pub selection_chars: usize,
    cursor_key: Option<(vault_core::ids::DocId, u64, usize)>,
}

struct LayoutCache {
    key: (u64, u32, u32, u32, bool),
    galley: Arc<Galley>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            hasher: RandomState::new(),
            cache: None,
            find: FindState::default(),
            pending_selection: None,
            scroll_to_cursor: false,
            request_focus: false,
            last_cursor_row: None,
            cursor: (1, 1),
            selection_chars: 0,
            cursor_key: None,
        }
    }
}

impl EditorState {
    fn hash_text(&self, s: &str) -> u64 {
        self.hasher.hash_one(s)
    }
}

pub struct EditorOptions {
    pub font_size: f32,
    pub soft_wrap: bool,
    pub line_numbers: bool,
    pub highlight_line: bool,
    pub mode: DocMode,
    /// Content revision counter (cache key for cursor line/column).
    pub rev: u64,
}

pub struct EditorOutput {
    pub changed: bool,
}

pub fn editor_id(doc: vault_core::ids::DocId) -> Id {
    Id::unique(("nilavar-editor", doc.to_hex()))
}

fn apply_edit(text: &mut SecretText, e: &Edit) {
    if e.range.start < e.range.end {
        text.remove_range(e.range.start, e.range.end);
    }
    if !e.insert.is_empty() {
        text.insert_str(e.range.start, &e.insert);
    }
}

fn set_selection(ctx: &egui::Context, id: Id, anchor: usize, cursor: usize) {
    if let Some(mut st) = egui::TextEdit::load_state(ctx, id) {
        st.cursor.set_char_range(Some(CCursorRange {
            primary: CCursor::new(cursor),
            secondary: CCursor::new(anchor),
            h_pos: None,
        }));
        egui::TextEdit::store_state(ctx, id, st);
    }
}

fn current_selection(ctx: &egui::Context, id: Id) -> Option<(usize, usize)> {
    let st = egui::TextEdit::load_state(ctx, id)?;
    let r = st.cursor.char_range()?;
    Some((r.secondary.index.0, r.primary.index.0))
}

/// Shows the editor for one document's text.
pub fn show(
    ui: &mut egui::Ui,
    doc_id: vault_core::ids::DocId,
    text: &mut SecretText,
    st: &mut EditorState,
    opts: &EditorOptions,
) -> EditorOutput {
    let id = editor_id(doc_id);
    let ctx = ui.ctx().clone();
    let mut changed = false;

    // Apply a pending programmatic selection (search hit, find next, …).
    if let Some((a, b)) = st.pending_selection.take() {
        if egui::TextEdit::load_state(&ctx, id).is_none() {
            let mut s = egui::text_edit::TextEditState::default();
            s.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(a.min(b)),
                CCursor::new(a.max(b)),
            )));
            egui::TextEdit::store_state(&ctx, id, s);
        } else {
            set_selection(&ctx, id, a, b);
        }
        st.scroll_to_cursor = true;
    }

    // Editor key handling before the widget consumes keys.
    let focused = ctx.memory(|m| m.has_focus(id));
    if focused {
        let ime_active = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Ime(_))));
        if !ime_active
            && opts.mode == DocMode::Markdown
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            let sel = current_selection(&ctx, id);
            let mut handled = false;
            if let Some((a, b)) = sel
                && a == b
                && let Some(e) = textops::continue_list(text.expose(), b)
            {
                apply_edit(text, &e);
                set_selection(&ctx, id, e.selection.0, e.selection.1);
                handled = true;
            }
            if !handled {
                // Default newline (replacing any selection).
                if let Some((a, b)) = sel {
                    let (lo, hi) = (a.min(b), a.max(b));
                    let s = text.expose();
                    let (bl, bh) = (textops::char_to_byte(s, lo), textops::char_to_byte(s, hi));
                    apply_edit(
                        text,
                        &Edit {
                            range: bl..bh,
                            insert: "\n".into(),
                            selection: (lo + 1, lo + 1),
                        },
                    );
                    set_selection(&ctx, id, lo + 1, lo + 1);
                }
            }
            changed = true;
            st.scroll_to_cursor = true;
        }
        let shift_tab = ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab));
        let tab = !shift_tab && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
        if (tab || shift_tab)
            && let Some((a, b)) = current_selection(&ctx, id)
        {
            let multi_line = {
                let s = text.expose();
                let (lo, hi) = (a.min(b), a.max(b));
                let bl = textops::char_to_byte(s, lo);
                let bh = textops::char_to_byte(s, hi);
                s[bl..bh].contains('\n')
            };
            let e = if shift_tab {
                textops::outdent(text.expose(), a, b)
            } else if multi_line {
                textops::indent(text.expose(), a, b)
            } else {
                let s = text.expose();
                let lo = a.min(b);
                let hi = a.max(b);
                let bl = textops::char_to_byte(s, lo);
                let bh = textops::char_to_byte(s, hi);
                let n = textops::INDENT.chars().count();
                Edit {
                    range: bl..bh,
                    insert: textops::INDENT.into(),
                    selection: (lo + n, lo + n),
                }
            };
            apply_edit(text, &e);
            set_selection(&ctx, id, e.selection.0, e.selection.1);
            changed = true;
        }
    }

    let wrap = opts.soft_wrap;
    let font_size = opts.font_size;
    let mode = opts.mode;
    let row_height = ui.fonts_mut(|f| f.row_height(&theme::mono(font_size)));

    // Gutter width from the number of logical lines.
    let line_count = text.expose().bytes().filter(|b| *b == b'\n').count() + 1;
    let digits = line_count.to_string().len().max(2) as f32;
    let char_w = ui.fonts_mut(|f| f.glyph_width(&theme::mono(font_size * 0.86), '0'));
    let gutter_w = if opts.line_numbers {
        digits * char_w + 22.0
    } else {
        10.0
    };

    let scroll = if wrap {
        egui::ScrollArea::vertical()
    } else {
        egui::ScrollArea::both()
    };
    scroll
        .id_salt(("editor-scroll", doc_id.to_hex()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (gutter_rect, _) = ui.allocate_exact_size(
                    egui::vec2(gutter_w, ui.available_height().max(row_height)),
                    Sense::hover(),
                );

                // Current-line highlight (from last frame's layout).
                if opts.highlight_line
                    && let Some(r) = st.last_cursor_row
                {
                    let full = Rect::from_x_y_ranges(ui.max_rect().x_range(), r.y_range());
                    ui.painter().rect_filled(full, 0.0, theme::LINE_HI);
                }

                let cache = &mut st.cache;
                let hasher = &st.hasher;
                let mut layouter =
                    |ui: &egui::Ui, buf: &dyn TextBuffer, wrap_width: f32| -> Arc<Galley> {
                        let s = buf.as_str();
                        let ppp = ui.ctx().pixels_per_point();
                        let ww = if wrap { wrap_width } else { f32::INFINITY };
                        let key = (
                            hasher.hash_one(s),
                            ww.to_bits(),
                            font_size.to_bits(),
                            ppp.to_bits(),
                            mode == DocMode::Markdown,
                        );
                        if let Some(c) = cache.as_ref()
                            && c.key == key
                        {
                            return c.galley.clone();
                        }
                        let mut job = match mode {
                            DocMode::Markdown => highlight::markdown_job(s, font_size),
                            DocMode::Plain => highlight::plain_job(s, font_size),
                        };
                        job.wrap.max_width = ww;
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        *cache = Some(LayoutCache {
                            key,
                            galley: g.clone(),
                        });
                        g
                    };

                let mut buf = EditorBuffer {
                    text: &mut *text,
                    changed: false,
                };
                let te = egui::TextEdit::multiline(&mut buf)
                    .id(id)
                    .font(theme::mono(font_size))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin {
                        left: 6,
                        right: 16,
                        top: 8,
                        bottom: 120,
                    })
                    .desired_width(if wrap {
                        ui.available_width()
                    } else {
                        f32::INFINITY
                    })
                    .desired_rows(30)
                    .lock_focus(true)
                    .layouter(&mut layouter);
                let out = te.show(ui);
                changed |= buf.changed;

                if st.request_focus {
                    out.response.request_focus();
                    st.request_focus = false;
                }

                let galley = &out.galley;
                let origin = out.galley_pos;

                // Cursor info + current row rect for next frame.
                if let Some(r) = out.cursor_range {
                    let p = r.primary.index.0;
                    let sorted = r.as_sorted_char_range();
                    st.selection_chars = sorted.end.0 - sorted.start.0;
                    let key = (doc_id, opts.rev, p);
                    if buf.changed || st.cursor_key != Some(key) {
                        st.cursor = textops::line_col(buf.as_str(), p);
                        st.cursor_key = if buf.changed { None } else { Some(key) };
                    }
                    let rect = galley
                        .pos_from_cursor(r.primary)
                        .translate(origin.to_vec2());
                    st.last_cursor_row = Some(rect.expand2(egui::vec2(0.0, 1.0)));
                    if st.scroll_to_cursor {
                        ui.scroll_to_rect(rect.expand(40.0), Some(Align::Center));
                        st.scroll_to_cursor = false;
                    }
                } else {
                    st.last_cursor_row = None;
                }

                // Find matches.
                if st.find.open && !st.find.query.is_empty() {
                    let key = (
                        st.hasher.hash_one(buf.as_str()),
                        st.hasher.hash_one(st.find.query.as_str()),
                        st.find.case_sensitive,
                    );
                    if st.find.matches_key != Some(key) {
                        st.find.matches = search::find_all_limited(
                            buf.as_str(),
                            &st.find.query,
                            st.find.case_sensitive,
                            5000,
                        );
                        st.find.matches_key = Some(key);
                        if st.find.current >= st.find.matches.len() {
                            st.find.current = 0;
                        }
                    }
                    let s = buf.as_str();
                    let clip = ui.clip_rect();
                    for (i, m) in st.find.matches.iter().enumerate() {
                        let a = textops::byte_to_char(s, m.start);
                        let b = a + s[m.clone()].chars().count();
                        let ra = galley
                            .pos_from_cursor(CCursor::new(a))
                            .translate(origin.to_vec2());
                        if ra.max.y < clip.min.y || ra.min.y > clip.max.y {
                            continue;
                        }
                        let rb = galley
                            .pos_from_cursor(CCursor::new(b))
                            .translate(origin.to_vec2());
                        let rect = if (ra.min.y - rb.min.y).abs() < 1.0 {
                            Rect::from_min_max(
                                ra.min,
                                egui::pos2(rb.max.x.max(ra.min.x + 2.0), ra.max.y),
                            )
                        } else {
                            Rect::from_min_max(ra.min, egui::pos2(ra.min.x + 6.0, ra.max.y))
                        };
                        let current = i == st.find.current;
                        let stroke = if current {
                            Stroke::new(1.5, theme::AMBER)
                        } else {
                            Stroke::new(1.0, theme::AMBER.gamma_multiply(0.45))
                        };
                        ui.painter()
                            .rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Outside);
                    }
                } else {
                    st.find.matches.clear();
                    st.find.matches_key = None;
                }

                // Gutter line numbers (logical lines; wrapped rows unnumbered).
                if opts.line_numbers {
                    let painter = ui.painter_at(gutter_rect.expand2(egui::vec2(0.0, 100_000.0)));
                    let cur_line = st.cursor.0;
                    let clip = ui.clip_rect();
                    let mut line = 1usize;
                    let mut starts_line = true;
                    let font = theme::mono(font_size * 0.86);
                    for row in &galley.rows {
                        let r = row.rect().translate(origin.to_vec2());
                        if starts_line && r.max.y >= clip.min.y && r.min.y <= clip.max.y {
                            let color = if line == cur_line && out.cursor_range.is_some() {
                                theme::GREEN
                            } else {
                                Color32::from_rgb(0x4A, 0x66, 0x59)
                            };
                            painter.text(
                                egui::pos2(gutter_rect.right() - 10.0, r.center().y),
                                egui::Align2::RIGHT_CENTER,
                                line.to_string(),
                                font.clone(),
                                color,
                            );
                        }
                        if row.ends_with_newline {
                            line += 1;
                            starts_line = true;
                        } else {
                            starts_line = false;
                        }
                    }
                    painter.vline(
                        gutter_rect.right() - 2.0,
                        gutter_rect.y_range(),
                        Stroke::new(1.0, theme::BORDER.gamma_multiply(0.6)),
                    );
                }
            });
        });

    EditorOutput { changed }
}

/// Moves to the next/previous find match and selects it.
pub fn find_step(st: &mut EditorState, text: &str, forward: bool) {
    let n = st.find.matches.len();
    if n == 0 {
        return;
    }
    st.find.current = if forward {
        (st.find.current + 1) % n
    } else {
        (st.find.current + n - 1) % n
    };
    select_match(st, text);
}

pub fn select_match(st: &mut EditorState, text: &str) {
    if let Some(m) = st.find.matches.get(st.find.current).cloned() {
        let a = textops::byte_to_char(text, m.start);
        let b = a + text[m].chars().count();
        st.pending_selection = Some((a, b));
    }
}

/// Replaces the current match; returns whether text changed.
pub fn replace_current(st: &mut EditorState, text: &mut SecretText) -> bool {
    let Some(m) = st.find.matches.get(st.find.current).cloned() else {
        return false;
    };
    let rep = st.find.replacement.clone();
    text.remove_range(m.start, m.end);
    text.insert_str(m.start, &rep);
    st.find.matches_key = None;
    let a = textops::byte_to_char(text.expose(), m.start);
    st.pending_selection = Some((a, a + rep.chars().count()));
    true
}

/// Replaces all matches; returns the count.
pub fn replace_all(st: &mut EditorState, text: &mut SecretText) -> usize {
    let (new, n) = search::replace_all(
        text.expose(),
        &st.find.query,
        &st.find.replacement,
        st.find.case_sensitive,
    );
    if n > 0 {
        text.set(&new);
        st.find.matches_key = None;
    }
    n
}

/// Hash helper for other modules needing a keyed text hash.
pub fn keyed_hash(st: &EditorState, s: &str) -> u64 {
    st.hash_text(s)
}
