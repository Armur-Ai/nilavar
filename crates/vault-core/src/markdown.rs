//! Inert Markdown preview model.
//!
//! Markdown is parsed locally (pulldown-cmark) into plain data — blocks of
//! styled text spans — that the GUI draws with native widgets. There is no
//! HTML renderer and no embedded browser:
//!
//! * raw HTML (block and inline) is shown as literal, inert text;
//! * images are **never** loaded: they appear as a placeholder showing the
//!   alt text and the destination;
//! * links are data: the destination is kept for display, and following it
//!   requires a separate explicit user action in the UI;
//! * there are no file includes, scripts or remote fetches of any kind.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Maximum container nesting kept in the model; deeper content is flattened.
pub const MAX_NESTING: usize = 24;

/// Inline style flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    /// Bold.
    pub strong: bool,
    /// Italic.
    pub emphasis: bool,
    /// Strikethrough.
    pub strike: bool,
    /// Inline code.
    pub code: bool,
}

/// What a span represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// Ordinary text.
    Text,
    /// Link text; `usize` indexes [`Preview::links`].
    Link(usize),
    /// Image placeholder (never loaded); `usize` indexes [`Preview::links`].
    Image(usize),
    /// Raw HTML shown literally (inert).
    RawHtml,
    /// Line break.
    Break,
}

/// A run of text with one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Text.
    pub text: String,
    /// Style.
    pub style: Style,
    /// Kind.
    pub kind: SpanKind,
}

/// One list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// Task checkbox state, if a task item.
    pub task: Option<bool>,
    /// Content blocks.
    pub blocks: Vec<Block>,
}

/// Block-level element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// Heading level 1–6.
    Heading(u8, Vec<Span>),
    /// Paragraph.
    Paragraph(Vec<Span>),
    /// Fenced or indented code.
    Code {
        /// Info string language (may be empty).
        lang: String,
        /// Code text.
        text: String,
    },
    /// Block quote.
    Quote(Vec<Block>),
    /// List (`start` is `Some(n)` for ordered lists).
    List {
        /// First number for ordered lists.
        start: Option<u64>,
        /// Items.
        items: Vec<ListItem>,
    },
    /// Thematic break.
    Rule,
    /// Table: header cells then rows of cells.
    Table {
        /// Header cells.
        header: Vec<Vec<Span>>,
        /// Body rows.
        rows: Vec<Vec<Vec<Span>>>,
    },
    /// Raw HTML block shown literally (inert).
    Html(String),
}

/// A parsed preview.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    /// Top-level blocks.
    pub blocks: Vec<Block>,
    /// Link/image destinations (data only — never fetched).
    pub links: Vec<String>,
}

enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List(Option<u64>, Vec<ListItem>),
    Item(Option<bool>, Vec<Block>, Vec<Span>),
    Table(Vec<Vec<Span>>, Vec<Vec<Vec<Span>>>, bool),
    Row(Vec<Vec<Span>>),
    Cell(Vec<Span>),
}

struct Builder {
    stack: Vec<Frame>,
    inline: Option<Vec<Span>>,
    heading: Option<u8>,
    code: Option<(String, String)>,
    html: Option<String>,
    style: Style,
    links: Vec<String>,
    link_stack: Vec<SpanKind>,
    image_depth: usize,
}

impl Builder {
    fn push_block(&mut self, b: Block) {
        for f in self.stack.iter_mut().rev() {
            match f {
                Frame::Root(v) | Frame::Quote(v) => {
                    v.push(b);
                    return;
                }
                Frame::Item(_, v, pending) => {
                    if !pending.is_empty() {
                        let spans = std::mem::take(pending);
                        v.push(Block::Paragraph(spans));
                    }
                    v.push(b);
                    return;
                }
                _ => continue,
            }
        }
    }

    fn spans_target(&mut self) -> &mut Vec<Span> {
        if self.inline.is_some() {
            return self.inline.as_mut().unwrap_or_else(|| unreachable!());
        }
        // Tight list items and table cells receive inline content directly.
        let idx = self
            .stack
            .iter()
            .rposition(|f| matches!(f, Frame::Item(..) | Frame::Cell(..)));
        match idx.map(|i| &mut self.stack[i]) {
            Some(Frame::Item(_, _, pending)) => pending,
            Some(Frame::Cell(c)) => c,
            _ => self.inline.get_or_insert_with(Vec::new),
        }
    }

    fn text(&mut self, t: &str, kind_override: Option<SpanKind>) {
        let kind = kind_override
            .unwrap_or_else(|| self.link_stack.last().copied().unwrap_or(SpanKind::Text));
        let style = self.style;
        let target = self.spans_target();
        if let Some(last) = target.last_mut()
            && last.style == style
            && last.kind == kind
            && kind != SpanKind::Break
        {
            last.text.push_str(t);
            return;
        }
        target.push(Span {
            text: t.to_owned(),
            style,
            kind,
        });
    }

    fn container_depth(&self) -> usize {
        self.stack.len()
    }
}

/// Parses Markdown into an inert preview model.
pub fn parse(src: &str) -> Preview {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(src, opts);
    let mut b = Builder {
        stack: vec![Frame::Root(Vec::new())],
        inline: None,
        heading: None,
        code: None,
        html: None,
        style: Style::default(),
        links: Vec::new(),
        link_stack: Vec::new(),
        image_depth: 0,
    };
    // Depth beyond MAX_NESTING: containers are not pushed; their content is
    // flattened into the nearest kept container.
    let mut skipped_containers: Vec<bool> = Vec::new();

    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    if b.inline.is_none() && !in_cell(&b) {
                        b.inline = Some(Vec::new());
                    }
                }
                Tag::Heading { level, .. } => {
                    b.heading = Some(heading_num(level));
                    b.inline = Some(Vec::new());
                }
                Tag::BlockQuote(_) => {
                    let keep = b.container_depth() < MAX_NESTING;
                    skipped_containers.push(!keep);
                    if keep {
                        flush_item_pending(&mut b);
                        b.stack.push(Frame::Quote(Vec::new()));
                    }
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            info.split_whitespace().next().unwrap_or("").to_owned()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    b.code = Some((lang, String::new()));
                }
                Tag::HtmlBlock => b.html = Some(String::new()),
                Tag::List(start) => {
                    let keep = b.container_depth() < MAX_NESTING;
                    skipped_containers.push(!keep);
                    if keep {
                        flush_item_pending(&mut b);
                        b.stack.push(Frame::List(start, Vec::new()));
                    }
                }
                Tag::Item => {
                    let keep = b.container_depth() < MAX_NESTING
                        && matches!(b.stack.last(), Some(Frame::List(..)));
                    skipped_containers.push(!keep);
                    if keep {
                        b.stack.push(Frame::Item(None, Vec::new(), Vec::new()));
                    }
                }
                Tag::Table(_) => b.stack.push(Frame::Table(Vec::new(), Vec::new(), true)),
                Tag::TableHead => {
                    if let Some(Frame::Table(_, _, head)) = b.stack.last_mut() {
                        *head = true;
                    }
                    b.stack.push(Frame::Row(Vec::new()));
                }
                Tag::TableRow => b.stack.push(Frame::Row(Vec::new())),
                Tag::TableCell => b.stack.push(Frame::Cell(Vec::new())),
                Tag::Emphasis => b.style.emphasis = true,
                Tag::Strong => b.style.strong = true,
                Tag::Strikethrough => b.style.strike = true,
                Tag::Link { dest_url, .. } => {
                    b.links.push(dest_url.to_string());
                    b.link_stack.push(SpanKind::Link(b.links.len() - 1));
                }
                Tag::Image { dest_url, .. } => {
                    b.links.push(dest_url.to_string());
                    let idx = b.links.len() - 1;
                    b.link_stack.push(SpanKind::Image(idx));
                    b.image_depth += 1;
                    b.text("[image not loaded: ", Some(SpanKind::Image(idx)));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    if let Some(spans) = b.inline.take() {
                        b.push_block(Block::Paragraph(spans));
                    }
                }
                TagEnd::Heading(_) => {
                    let spans = b.inline.take().unwrap_or_default();
                    let lvl = b.heading.take().unwrap_or(1);
                    b.push_block(Block::Heading(lvl, spans));
                }
                TagEnd::BlockQuote(_) => {
                    if skipped_containers.pop() == Some(false) {
                        flush_item_pending(&mut b);
                        if let Some(Frame::Quote(v)) = b.stack.pop() {
                            b.push_block(Block::Quote(v));
                        }
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, text)) = b.code.take() {
                        b.push_block(Block::Code { lang, text });
                    }
                }
                TagEnd::HtmlBlock => {
                    if let Some(h) = b.html.take() {
                        b.push_block(Block::Html(h));
                    }
                }
                TagEnd::List(_) => {
                    if skipped_containers.pop() == Some(false)
                        && let Some(Frame::List(start, items)) = b.stack.pop()
                    {
                        b.push_block(Block::List { start, items });
                    }
                }
                TagEnd::Item => {
                    if skipped_containers.pop() == Some(false)
                        && let Some(Frame::Item(task, mut blocks, pending)) = b.stack.pop()
                    {
                        if !pending.is_empty() {
                            blocks.push(Block::Paragraph(pending));
                        }
                        if let Some(Frame::List(_, items)) = b.stack.last_mut() {
                            items.push(ListItem { task, blocks });
                        }
                    }
                }
                TagEnd::TableCell => {
                    if let Some(Frame::Cell(c)) = b.stack.pop()
                        && let Some(Frame::Row(r)) = b.stack.last_mut()
                    {
                        r.push(c);
                    }
                }
                TagEnd::TableHead | TagEnd::TableRow => {
                    if let Some(Frame::Row(r)) = b.stack.pop()
                        && let Some(Frame::Table(h, rows, head)) = b.stack.last_mut()
                    {
                        if *head {
                            *h = r;
                            *head = false;
                        } else {
                            rows.push(r);
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(Frame::Table(header, rows, _)) = b.stack.pop() {
                        b.push_block(Block::Table { header, rows });
                    }
                }
                TagEnd::Emphasis => b.style.emphasis = false,
                TagEnd::Strong => b.style.strong = false,
                TagEnd::Strikethrough => b.style.strike = false,
                TagEnd::Link => {
                    b.link_stack.pop();
                }
                TagEnd::Image => {
                    if let Some(SpanKind::Image(idx)) = b.link_stack.last().copied() {
                        b.text("]", Some(SpanKind::Image(idx)));
                    }
                    b.link_stack.pop();
                    b.image_depth = b.image_depth.saturating_sub(1);
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some((_, code)) = b.code.as_mut() {
                    code.push_str(&t);
                } else if let Some(h) = b.html.as_mut() {
                    h.push_str(&t);
                } else {
                    b.text(&t, None);
                }
            }
            Event::Code(t) => {
                let prev = b.style;
                b.style.code = true;
                b.text(&t, None);
                b.style = prev;
            }
            Event::Html(h) => {
                if let Some(acc) = b.html.as_mut() {
                    acc.push_str(&h);
                } else {
                    b.text(&h, Some(SpanKind::RawHtml));
                }
            }
            Event::InlineHtml(h) => b.text(&h, Some(SpanKind::RawHtml)),
            Event::SoftBreak => b.text(" ", None),
            Event::HardBreak => b.text("\n", Some(SpanKind::Break)),
            Event::Rule => b.push_block(Block::Rule),
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item(task, _, _)) = b
                    .stack
                    .iter_mut()
                    .rev()
                    .find(|f| matches!(f, Frame::Item(..)))
                {
                    *task = Some(checked);
                }
            }
            Event::FootnoteReference(r) => b.text(&format!("[^{r}]"), None),
            Event::InlineMath(m) | Event::DisplayMath(m) => {
                let prev = b.style;
                b.style.code = true;
                b.text(&m, None);
                b.style = prev;
            }
        }
    }
    // Close anything left open (malformed input is still rendered inertly).
    if let Some(spans) = b.inline.take() {
        b.push_block(Block::Paragraph(spans));
    }
    let blocks = match b.stack.drain(..).next() {
        Some(Frame::Root(v)) => v,
        _ => Vec::new(),
    };
    Preview {
        blocks,
        links: b.links,
    }
}

fn in_cell(b: &Builder) -> bool {
    matches!(b.stack.last(), Some(Frame::Cell(_)))
}

fn flush_item_pending(b: &mut Builder) {
    if let Some(Frame::Item(_, blocks, pending)) = b.stack.last_mut()
        && !pending.is_empty()
    {
        let spans = std::mem::take(pending);
        blocks.push(Block::Paragraph(spans));
    }
}

fn heading_num(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Classifies a link destination for the explicit-open dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkClass {
    /// `http://` or `https://` — may be opened in the system browser on
    /// explicit confirmation.
    Web,
    /// `mailto:` — may be opened on explicit confirmation.
    Mail,
    /// Anything else (`file:`, relative paths, `javascript:`, custom
    /// schemes): copy only, never opened by the app.
    CopyOnly,
}

/// Classifies a destination.
pub fn classify_link(dest: &str) -> LinkClass {
    let lower = dest.trim().to_ascii_lowercase();
    if (lower.starts_with("https://") || lower.starts_with("http://"))
        && !dest.chars().any(char::is_control)
    {
        LinkClass::Web
    } else if lower.starts_with("mailto:") && !dest.chars().any(char::is_control) {
        LinkClass::Mail
    } else {
        LinkClass::CopyOnly
    }
}

/// Plain text of a span list (for tests and accessibility).
pub fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_text(p: &Preview) -> String {
        fn walk(b: &[Block], out: &mut String) {
            for x in b {
                match x {
                    Block::Heading(_, s) | Block::Paragraph(s) => out.push_str(&spans_text(s)),
                    Block::Code { text, .. } | Block::Html(text) => out.push_str(text),
                    Block::Quote(v) => walk(v, out),
                    Block::List { items, .. } => items.iter().for_each(|i| walk(&i.blocks, out)),
                    Block::Table { header, rows } => {
                        header.iter().for_each(|c| out.push_str(&spans_text(c)));
                        rows.iter()
                            .flatten()
                            .for_each(|c| out.push_str(&spans_text(c)));
                    }
                    Block::Rule => {}
                }
                out.push('\n');
            }
        }
        let mut s = String::new();
        walk(&p.blocks, &mut s);
        s
    }

    #[test]
    fn basic_structure() {
        let p = parse(
            "# Title\n\nSome **bold** and `code`.\n\n- [x] done\n- [ ] todo\n\n1. one\n2. two\n\n> quote\n\n---\n\n```rust\nfn main() {}\n```\n",
        );
        assert!(matches!(&p.blocks[0], Block::Heading(1, _)));
        match &p.blocks[1] {
            Block::Paragraph(s) => {
                assert!(s.iter().any(|x| x.style.strong && x.text == "bold"));
                assert!(s.iter().any(|x| x.style.code && x.text == "code"));
            }
            other => panic!("{other:?}"),
        }
        match &p.blocks[2] {
            Block::List { start: None, items } => {
                assert_eq!(items[0].task, Some(true));
                assert_eq!(items[1].task, Some(false));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&p.blocks[3], Block::List { start: Some(1), .. }));
        assert!(matches!(&p.blocks[4], Block::Quote(_)));
        assert!(matches!(&p.blocks[5], Block::Rule));
        assert!(matches!(&p.blocks[6], Block::Code { lang, .. } if lang == "rust"));
    }

    #[test]
    fn html_is_inert_literal_text() {
        let p = parse("<script>alert(1)</script>\n\nhi <img src=x onerror=alert(1)> there\n");
        assert!(matches!(&p.blocks[0], Block::Html(h) if h.contains("<script>")));
        match &p.blocks[1] {
            Block::Paragraph(s) => assert!(
                s.iter()
                    .any(|x| x.kind == SpanKind::RawHtml && x.text.contains("onerror"))
            ),
            other => panic!("{other:?}"),
        }
        assert!(
            p.links.is_empty(),
            "raw HTML must not produce fetchable resources"
        );
    }

    #[test]
    fn images_become_placeholders_and_links_are_data() {
        let p = parse(
            "![logo](https://example.invalid/x.png) [site](https://example.invalid) [f](file:///etc/hosts)",
        );
        assert_eq!(p.links.len(), 3);
        let text = all_text(&p);
        assert!(text.contains("[image not loaded: logo]"));
        assert_eq!(classify_link(&p.links[1]), LinkClass::Web);
        assert_eq!(classify_link(&p.links[2]), LinkClass::CopyOnly);
        assert_eq!(classify_link("javascript:alert(1)"), LinkClass::CopyOnly);
        assert_eq!(
            classify_link("mailto:someone@example.invalid"),
            LinkClass::Mail
        );
    }

    #[test]
    fn tables() {
        let p = parse("| a | b |\n|---|---|\n| 1 | 2 |\n");
        match &p.blocks[0] {
            Block::Table { header, rows } => {
                assert_eq!(spans_text(&header[1]), "b");
                assert_eq!(spans_text(&rows[0][0]), "1");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let src = format!("{}deep", "> ".repeat(200));
        let p = parse(&src);
        fn depth(b: &[Block]) -> usize {
            b.iter()
                .map(|x| match x {
                    Block::Quote(v) => 1 + depth(v),
                    _ => 0,
                })
                .max()
                .unwrap_or(0)
        }
        assert!(depth(&p.blocks) <= MAX_NESTING);
        assert!(all_text(&p).contains("deep"));
    }

    #[test]
    fn loose_list_items_keep_paragraphs() {
        let p = parse("- a\n\n  more\n- b\n");
        match &p.blocks[0] {
            Block::List { items, .. } => {
                assert_eq!(items.len(), 2);
                assert!(
                    spans_text(match &items[0].blocks[0] {
                        Block::Paragraph(s) => s,
                        _ => panic!(),
                    })
                    .contains('a')
                );
            }
            other => panic!("{other:?}"),
        }
    }
}
