//! Streaming markdown, committed once (psi-plan.md §11.2).
//!
//! [`MdStream`] splits streamed text into complete blocks (paragraphs, headings,
//! whole fenced code blocks) and [`render_block`] turns a block into lines: prose
//! through pulldown-cmark, fenced code verbatim and highlighted (§11.5).

use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};

use crate::style::{Line, Style, str_width};
use crate::theme::{self, Token};

/// A long block without blank lines is committed in pieces of this many lines, so
/// a streaming list appears progressively.
pub const PROGRESSIVE_LINES: usize = 20;

fn muted() -> Style {
    theme::style(Token::Muted)
}

fn code_style() -> Style {
    theme::style(Token::MdCode)
}

/// An opening fence: its char and length.
fn fence_open(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|&x| x == c).count();
    // A backtick fence's info string cannot contain backticks (that is inline code).
    (n >= 3 && (c == '~' || !t[n..].contains('`'))).then_some((c, n))
}

fn fence_close(line: &str, (c, n): (char, usize)) -> bool {
    let t = line.trim();
    t.chars().count() >= n && t.chars().all(|x| x == c)
}

/// The number of `#`s when `line` is an ATX heading.
fn heading_level(line: &str) -> Option<usize> {
    let n = line.chars().take_while(|&c| c == '#').count();
    ((1..=6).contains(&n) && line[n..].chars().next().is_none_or(|c| c == ' ')).then_some(n)
}

/// Splits streamed text into complete blocks.
#[derive(Clone, Debug, Default)]
pub struct MdStream {
    /// The incomplete block: complete lines, then the partial line.
    buf: String,
    /// Bytes of `buf` already looked at (always at a line start).
    scanned: usize,
    /// The open fence, while inside a code block.
    fence: Option<(char, usize)>,
    /// Lines in the current block outside a fence.
    lines: usize,
}

enum Kind {
    Blank,
    FenceOpen((char, usize)),
    Heading,
    Text,
}

impl MdStream {
    pub fn new() -> Self {
        MdStream::default()
    }

    /// Feeds a streamed delta and returns the blocks it completed, in order, as
    /// source text without the trailing newline.
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buf.extend(delta.chars().filter(|&c| c != '\r'));
        let mut out = Vec::new();
        while let Some(off) = self.buf[self.scanned..].find('\n') {
            let start = self.scanned;
            let next = start + off + 1;
            let line = &self.buf[start..start + off];
            if let Some(f) = self.fence {
                if fence_close(line, f) {
                    self.fence = None;
                    self.emit(next, &mut out);
                } else {
                    self.scanned = next;
                }
                continue;
            }
            let kind = if line.trim().is_empty() {
                Kind::Blank
            } else if let Some(f) = fence_open(line) {
                Kind::FenceOpen(f)
            } else if heading_level(line.trim_start()).is_some() {
                Kind::Heading
            } else {
                Kind::Text
            };
            match kind {
                Kind::Blank => {
                    self.emit(start, &mut out);
                    self.buf.drain(..off + 1);
                }
                Kind::FenceOpen(f) => {
                    self.emit(start, &mut out);
                    self.fence = Some(f);
                    self.scanned = off + 1;
                }
                Kind::Heading => {
                    self.emit(start, &mut out);
                    self.emit(off + 1, &mut out);
                }
                Kind::Text => {
                    self.scanned = next;
                    self.lines += 1;
                    // A table stays whole: a piece without its header
                    // wouldn't be a table (§11.5).
                    let table_row = line.trim_start().starts_with('|');
                    if self.lines >= PROGRESSIVE_LINES && !table_row {
                        self.emit(next, &mut out);
                    }
                }
            }
        }
        out
    }

    /// The incomplete tail, for a live preview.
    pub fn pending(&self) -> &str {
        &self.buf
    }

    /// Whether the stream is inside an unclosed code fence.
    pub fn in_fence(&self) -> bool {
        self.fence.is_some()
    }

    /// Ends the message: everything left, as final blocks.
    pub fn finish(&mut self) -> Vec<String> {
        let mut out = if self.buf.is_empty() || self.buf.ends_with('\n') {
            Vec::new()
        } else {
            self.push("\n")
        };
        let end = self.buf.len();
        self.emit(end, &mut out);
        self.fence = None;
        out
    }

    /// Moves `buf[..end]` out as a block (dropped when blank).
    fn emit(&mut self, end: usize, out: &mut Vec<String>) {
        let block: String = self.buf.drain(..end).collect();
        self.scanned = 0;
        self.lines = 0;
        if !block.trim().is_empty() {
            out.push(block.trim_end_matches('\n').to_string());
        }
    }
}

/// Lines for one block. Prose goes through a CommonMark parser (§11.5) and
/// is word-wrapped to `width`: headings bold (the `#`s dim), emphasis, `code`
/// in cyan, links underlined with the URL after them, list markers as
/// written with a hanging indent, quotes dim, tables as aligned columns.
/// Fenced code keeps its fence lines (dim) and its code lines verbatim and
/// unwrapped, colored when tagged with a known language, so a copy from
/// the terminal is the code.
pub fn render_block(block: &str, width: usize) -> Vec<Line> {
    let mut out = Vec::new();
    // Prose lines waiting to be rendered together.
    let mut prose_buf = String::new();
    // The open fence, its language tag, and its code lines so far.
    let mut fence: Option<((char, usize), String, Vec<&str>)> = None;
    for raw in block.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some((f, tag, code)) = &mut fence {
            if fence_close(line, *f) {
                out.extend(code_lines(tag, code));
                fence = None;
                out.push(Line::styled(line, theme::style(Token::MdCodeBlockBorder)));
            } else {
                code.push(line);
            }
        } else if let Some(f) = fence_open(line) {
            out.extend(prose(&std::mem::take(&mut prose_buf), width));
            let tag = line.trim_start()[f.1..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_owned();
            fence = Some((f, tag, Vec::new()));
            out.push(Line::styled(line, theme::style(Token::MdCodeBlockBorder)));
        } else {
            prose_buf.push_str(line);
            prose_buf.push('\n');
        }
    }
    out.extend(prose(&prose_buf, width));
    // A fence left open (the stream ended inside it).
    if let Some((_, tag, code)) = &fence {
        out.extend(code_lines(tag, code));
    }
    out
}

/// Code as written, colored when its tag names a known language (§11.5).
fn code_lines(tag: &str, code: &[&str]) -> Vec<Line> {
    if !tag.is_empty()
        && let Some(lines) = crate::highlight::highlight(tag, code)
    {
        return lines;
    }
    code.iter().map(|l| Line::raw(*l)).collect()
}

/// The list marker with its trailing space (`- `, `* `, `+ `, `12. `, `3) `).
fn list_marker(t: &str) -> Option<&str> {
    let b = t.as_bytes();
    let end = match b.first()? {
        b'-' | b'*' | b'+' => 1,
        b'0'..=b'9' => {
            let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits > 9 || !matches!(b.get(digits), Some(b'.' | b')')) {
                return None;
            }
            digits + 1
        }
        _ => return None,
    };
    (b.get(end) == Some(&b' ')).then(|| &t[..=end])
}

/// Wraps `body` after `first`; later rows start with `cont`.
fn wrap_under(first: Line, cont: &Line, body: Line, width: usize, out: &mut Vec<Line>) {
    let pw = first.width().max(cont.width());
    for (i, row) in body
        .wrap(width.saturating_sub(pw).max(1))
        .into_iter()
        .enumerate()
    {
        let mut l = if i == 0 { first.clone() } else { cont.clone() };
        l.append(row);
        out.push(l);
    }
}

/// What a paragraph's lines sit in: each adds to their prefix.
enum Container {
    Quote,
    /// A list item: its marker on its first line, spaces after.
    Item {
        marker: String,
        first: bool,
    },
}

struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Line>>,
    row: Vec<Line>,
    head_rows: usize,
}

/// Renders prose (no fenced code: [`render_block`] takes that out) from
/// pulldown-cmark's events.
struct Prose {
    width: usize,
    out: Vec<Line>,
    /// The paragraph being built: rows ended by hard breaks, and the current one.
    rows: Vec<Line>,
    cur: Line,
    styles: Vec<Style>,
    containers: Vec<Container>,
    /// Ordered lists' next number and delimiter; `None` for bullet lists.
    lists: Vec<Option<(u64, char)>>,
    /// The level of the heading being built.
    heading: Option<usize>,
    /// Open links: the URL and where their text starts in `cur`'s plain text.
    links: Vec<(String, usize)>,
    table: Option<Table>,
    /// Indented code being collected.
    code: Option<String>,
    saw_table: bool,
}

/// Prose lines rendered through a CommonMark parser (§11.5).
fn prose(src: &str, width: usize) -> Vec<Line> {
    if src.trim().is_empty() {
        return Vec::new();
    }
    let mut p = Prose {
        width,
        out: Vec::new(),
        rows: Vec::new(),
        cur: Line::new(),
        styles: vec![Style::new()],
        containers: Vec::new(),
        lists: Vec::new(),
        heading: None,
        links: Vec::new(),
        table: None,
        code: None,
        saw_table: false,
    };
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (ev, range) in Parser::new_ext(src, opts).into_offset_iter() {
        match ev {
            Event::Start(tag) => p.start(tag, &src[range.start..]),
            Event::End(tag) => p.end(tag),
            Event::Text(t) => match &mut p.code {
                Some(c) => c.push_str(&t),
                None => p.text(&t),
            },
            Event::Code(t) => {
                let st = p.style().patch(code_style());
                p.cur.push(&*t, st);
            }
            Event::SoftBreak => p.text(" "),
            Event::HardBreak => p.hard_break(),
            Event::InlineHtml(t) => p.text(&t),
            Event::Html(t) => {
                for l in t.lines() {
                    p.text(l);
                    p.hard_break();
                }
            }
            Event::Rule => {
                p.flush();
                let (mut first, _) = p.prefixes();
                first.push(src[range].trim(), muted());
                p.out.push(first);
            }
            Event::TaskListMarker(done) => p.text(if done { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(t) => p.text(&format!("[^{t}]")),
            _ => {}
        }
    }
    p.flush();
    // Rows without a header and separator aren't a table to the parser:
    // shown as written, unwrapped, like the rows of one.
    let rows: Vec<&str> = src.lines().filter(|l| !l.trim().is_empty()).collect();
    if !p.saw_table && rows.iter().all(|l| l.trim_start().starts_with('|')) {
        return rows.iter().map(|l| Line::raw(*l)).collect();
    }
    p.out
}

impl Prose {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, s: Style) {
        let top = self.style();
        self.styles.push(top.patch(s));
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn text(&mut self, t: &str) {
        let st = self.style();
        self.cur.push(t, st);
    }

    fn hard_break(&mut self) {
        let row = std::mem::take(&mut self.cur);
        self.rows.push(row);
    }

    /// The prefixes for a block's first and later lines; a list item's
    /// marker goes on its first line only.
    fn prefixes(&mut self) -> (Line, Line) {
        let mut first = Line::new();
        let mut cont = Line::new();
        for c in &mut self.containers {
            match c {
                Container::Quote => {
                    first.push("> ", muted());
                    cont.push("> ", muted());
                }
                Container::Item { marker, first: f } => {
                    let pad = " ".repeat(str_width(marker));
                    if *f {
                        first.push(marker.as_str(), Style::new());
                        *f = false;
                    } else {
                        first.push(&pad, Style::new());
                    }
                    cont.push(&pad, Style::new());
                }
            }
        }
        (first, cont)
    }

    /// Writes out the paragraph built so far, wrapped under its prefixes.
    fn flush(&mut self) {
        if self.cur.is_empty() && self.rows.is_empty() {
            return;
        }
        let mut rows = std::mem::take(&mut self.rows);
        rows.push(std::mem::take(&mut self.cur));
        let (mut first, mut cont) = self.prefixes();
        if let Some(n) = self.heading {
            let marks = format!("{} ", "#".repeat(n));
            first.push(&marks, muted());
            cont.push(" ".repeat(marks.len()), Style::new());
        }
        for (i, row) in rows.into_iter().enumerate() {
            let f = if i == 0 { first.clone() } else { cont.clone() };
            wrap_under(f, &cont, row, self.width, &mut self.out);
        }
    }

    /// `at` is the source from where the tag starts.
    fn start(&mut self, tag: Tag, at: &str) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level as usize);
                self.push_style(theme::style(Token::MdHeading).bold());
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.containers.push(Container::Quote);
                self.push_style(muted());
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.flush();
                let delim = list_marker(at.trim_start())
                    .and_then(|m| m.trim_end().chars().last())
                    .unwrap_or('.');
                self.lists.push(start.map(|n| (n, delim)));
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some((n, delim))) => {
                        let m = format!("{n}{delim} ");
                        *n += 1;
                        m
                    }
                    _ => list_marker(at.trim_start()).unwrap_or("- ").to_owned(),
                };
                self.containers.push(Container::Item {
                    marker,
                    first: true,
                });
            }
            Tag::Emphasis => self.push_style(Style::new().italic()),
            Tag::Strong => self.push_style(Style::new().bold()),
            Tag::Strikethrough => self.push_style(Style::new().strike()),
            Tag::Link { dest_url, .. } => {
                self.links
                    .push((dest_url.to_string(), self.cur.plain().len()));
                self.push_style(Style::new().underline());
            }
            Tag::Image { dest_url, .. } => {
                self.links
                    .push((dest_url.to_string(), self.cur.plain().len()));
                self.push_style(Style::new());
            }
            Tag::Table(aligns) => {
                self.flush();
                self.saw_table = true;
                self.table = Some(Table {
                    aligns,
                    rows: Vec::new(),
                    row: Vec::new(),
                    head_rows: 0,
                });
            }
            Tag::TableCell => self.cur = Line::new(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
                self.pop_style();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.containers.pop();
                self.pop_style();
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    let (first, cont) = self.prefixes();
                    for (i, l) in code.trim_end_matches('\n').split('\n').enumerate() {
                        let mut line = if i == 0 { first.clone() } else { cont.clone() };
                        line.push(l, Style::new());
                        self.out.push(line);
                    }
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush();
                self.containers.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link | TagEnd::Image => {
                self.pop_style();
                if let Some((url, at)) = self.links.pop() {
                    let plain = self.cur.plain();
                    let text = plain.get(at..).unwrap_or("");
                    let same = url == text || url.strip_prefix("mailto:") == Some(text);
                    if !url.is_empty() && !same {
                        self.cur.push(format!(" ({url})"), muted());
                    }
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.cur);
                if let Some(t) = &mut self.table {
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                    if tag == TagEnd::TableHead {
                        t.head_rows = 1;
                    }
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                }
            }
            _ => {}
        }
    }

    /// Aligned columns with the header bold and a rule under it; when that
    /// is wider than the terminal, the rows as `| a | b |`, unwrapped.
    fn render_table(&mut self, t: Table) {
        let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths: Vec<usize> = (0..cols)
            .map(|c| {
                t.rows
                    .iter()
                    .filter_map(|r| r.get(c))
                    .map(Line::width)
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let (first, cont) = self.prefixes();
        let total = first.width() + widths.iter().sum::<usize>() + 3 * cols.saturating_sub(1);
        let fits = total <= self.width;
        for (ri, row) in t.rows.iter().enumerate() {
            let mut line = if ri == 0 { first.clone() } else { cont.clone() };
            let head = ri < t.head_rows;
            if !fits {
                line.push("| ", Style::new());
            }
            for (c, width) in widths.iter().enumerate() {
                if c > 0 {
                    line.push(if fits { " │ " } else { " | " }, muted());
                }
                let cell = row.get(c).cloned().unwrap_or_else(Line::new);
                let cell = if head { bold(&cell) } else { cell };
                if !fits {
                    line.append(cell);
                    continue;
                }
                let pad = width.saturating_sub(cell.width());
                let (l, r) = match t.aligns.get(c) {
                    Some(Alignment::Right) => (pad, 0),
                    Some(Alignment::Center) => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                if l > 0 {
                    line.push(" ".repeat(l), Style::new());
                }
                line.append(cell);
                // No trailing spaces after the last column.
                if r > 0 && c + 1 < cols {
                    line.push(" ".repeat(r), Style::new());
                }
            }
            if !fits {
                line.push(" |", Style::new());
            }
            self.out.push(line);
            if fits && ri + 1 == t.head_rows {
                let mut rule = cont.clone();
                let parts: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                rule.push(parts.join("─┼─"), muted());
                self.out.push(rule);
            }
        }
    }
}

fn bold(l: &Line) -> Line {
    let mut out = Line::new();
    for s in &l.spans {
        out.push(&s.text, s.style.bold());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(chunks: &[&str]) -> (Vec<String>, MdStream) {
        let mut md = MdStream::new();
        let mut blocks = Vec::new();
        for c in chunks {
            blocks.extend(md.push(c));
        }
        (blocks, md)
    }

    fn plain(lines: &[Line]) -> Vec<String> {
        lines.iter().map(Line::plain).collect()
    }

    #[test]
    fn blocks_split_at_blank_lines() {
        let (blocks, mut md) = feed(&["Hello wor", "ld.\nSecond line.\n\nNext ", "para"]);
        assert_eq!(blocks, ["Hello world.\nSecond line."]);
        assert_eq!(md.pending(), "Next para");
        assert_eq!(md.finish(), ["Next para"]);
        assert_eq!(md.pending(), "");
        assert!(md.finish().is_empty());
    }

    #[test]
    fn extra_blank_lines_and_crlf() {
        let (blocks, mut md) = feed(&["\n\na\r\n\r\n\r\n\r\nb\r\n"]);
        assert_eq!(blocks, ["a"]);
        assert_eq!(md.finish(), ["b"]);
    }

    #[test]
    fn fence_kept_whole_with_blank_lines() {
        let src = "Intro:\n```rust\nfn a() {}\n\n\nfn b() {}\n```\nAfter.\n\n";
        let (blocks, _) = feed(&[src]);
        assert_eq!(
            blocks,
            ["Intro:", "```rust\nfn a() {}\n\n\nfn b() {}\n```", "After."]
        );
    }

    #[test]
    fn fence_split_across_awkward_deltas() {
        let (blocks, md) = feed(&["``", "`rust\nlet x", " = 1;\n\n", "`", "``", "\ntail"]);
        assert_eq!(blocks, ["```rust\nlet x = 1;\n\n```"]);
        assert!(!md.in_fence());
        assert_eq!(md.pending(), "tail");
        let (blocks, md) = feed(&["``", "`rust\nlet x = 1;\n\n``"]);
        assert!(blocks.is_empty());
        assert!(md.in_fence());
        assert_eq!(md.pending(), "```rust\nlet x = 1;\n\n``");
    }

    #[test]
    fn closing_fence_must_match() {
        let (blocks, md) = feed(&["````md\n```\ninner\n```\n````\n"]);
        assert_eq!(blocks, ["````md\n```\ninner\n```\n````"]);
        assert!(!md.in_fence());
        let (blocks, _) = feed(&["~~~\n```\n~~~\n"]);
        assert_eq!(blocks, ["~~~\n```\n~~~"]);
    }

    #[test]
    fn unclosed_fence_finishes_as_a_block() {
        let (blocks, mut md) = feed(&["```\ncode\n\nmore"]);
        assert!(blocks.is_empty());
        assert_eq!(md.finish(), ["```\ncode\n\nmore"]);
        assert!(!md.in_fence());
    }

    #[test]
    fn closing_fence_without_newline_at_finish() {
        let (_, mut md) = feed(&["```\nx\n```"]);
        assert_eq!(md.finish(), ["```\nx\n```"]);
    }

    #[test]
    fn headings_are_own_blocks() {
        let (blocks, _) = feed(&["text\n## Title\n- a\n- b\n\n"]);
        assert_eq!(blocks, ["text", "## Title", "- a\n- b"]);
    }

    #[test]
    fn inline_backticks_do_not_open_fence() {
        let (blocks, md) = feed(&["use ```x``` here\n\n"]);
        assert_eq!(blocks, ["use ```x``` here"]);
        assert!(!md.in_fence());
    }

    #[test]
    fn long_block_commits_progressively() {
        let mut md = MdStream::new();
        let mut blocks = Vec::new();
        for i in 0..45 {
            blocks.extend(md.push(&format!("- item {i}\n")));
        }
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lines().count(), PROGRESSIVE_LINES);
        assert!(blocks[0].starts_with("- item 0\n"));
        assert!(blocks[1].starts_with("- item 20\n"));
        assert_eq!(md.pending().lines().count(), 5);
        // Inside a fence, nothing commits until it closes.
        let mut md = MdStream::new();
        let mut n = md.push("```\n").len();
        for i in 0..50 {
            n += md.push(&format!("line {i}\n")).len();
        }
        assert_eq!(n, 0);
        assert_eq!(md.push("```\n").len(), 1);
    }

    #[test]
    fn render_heading() {
        let ls = render_block("## Big **deal**", 80);
        assert_eq!(plain(&ls), ["## Big deal"]);
        assert_eq!(ls[0].spans[0], crate::style::Span::new("## ", muted()));
        assert!(ls[0].spans[1..].iter().all(|s| s.style.bold));
    }

    #[test]
    fn render_inline_styles() {
        let ls = render_block("a **b** *c* _d_ `e` f", 80);
        assert_eq!(plain(&ls), ["a b c d e f"]);
        let s = &ls[0].spans;
        let find = |t: &str| s.iter().find(|x| x.text == t).map(|x| x.style);
        assert_eq!(find("b"), Some(Style::new().bold()));
        assert_eq!(find("c"), Some(Style::new().italic()));
        assert_eq!(find("d"), Some(Style::new().italic()));
        assert_eq!(find("e"), Some(code_style()));
    }

    #[test]
    fn render_literal_when_unmatched() {
        let ls = render_block("a **b and snake_case_name and 2 * 3 * 4 and `x", 80);
        assert_eq!(
            plain(&ls),
            ["a **b and snake_case_name and 2 * 3 * 4 and `x"]
        );
        assert!(ls[0].spans.iter().all(|s| s.style == Style::new()));
        assert_eq!(
            plain(&render_block(r"\*not\* ``a ` b``", 80)),
            ["*not* a ` b"]
        );
    }

    #[test]
    fn render_code_block_verbatim_and_unwrapped() {
        let long = "let very_long_identifier_name = some_function(argument_one, argument_two);";
        let block = format!("```rust\n{long}\n    indented\n```");
        let ls = render_block(&block, 20);
        assert_eq!(plain(&ls), ["```rust", long, "    indented", "```"]);
        assert!(ls[0].spans[0].style.dim && ls[3].spans[0].style.dim);
        // Tagged code is colored (`let` is a keyword), untagged code isn't.
        assert_eq!(ls[1].spans[0].style, theme::style(Token::SyntaxKeyword));
        let untagged = render_block("```\n**not bold**\n```", 80);
        assert_eq!(plain(&untagged)[1], "**not bold**");
        assert_eq!(untagged[1].spans[0].style, Style::new());
        // An unknown tag shows the code as is; an unclosed fence is still code.
        assert_eq!(
            render_block("```nope\nx\n```", 80)[1].spans[0].style,
            Style::new()
        );
        assert_eq!(render_block("```py\nx = 1", 80)[1].plain(), "x = 1");
    }

    #[test]
    fn render_prose_wraps() {
        let ls = render_block("one two three four five six", 10);
        assert_eq!(plain(&ls), ["one two", "three four", "five six"]);
    }

    #[test]
    fn render_lists_with_hanging_indent() {
        let ls = render_block("- alpha beta gamma\n  1. delta epsilon", 13);
        assert_eq!(
            plain(&ls),
            ["- alpha beta", "  gamma", "  1. delta", "     epsilon"]
        );
        // Markers as written: the bullet character, an ordered list's start
        // and delimiter.
        assert_eq!(plain(&render_block("* a\n* b", 80)), ["* a", "* b"]);
        assert_eq!(plain(&render_block("3) x\n3) y", 80)), ["3) x", "4) y"]);
    }

    #[test]
    fn render_quote_dim_and_rule() {
        let ls = render_block("> quoted words here\n---", 10);
        assert_eq!(plain(&ls), ["> quoted", "> words", "> here", "---"]);
        assert!(ls.iter().all(|l| l.spans.iter().all(|s| s.style.dim)));
    }

    #[test]
    fn render_table_unwrapped() {
        let row = "| a column | another column |";
        assert_eq!(plain(&render_block(row, 10)), [row]);
    }

    #[test]
    fn tables_line_up_and_fall_back_when_wide() {
        let src = "| name | n |\n|------|--:|\n| **a** | 1 |\n| bb | 22 |";
        let ls = render_block(src, 80);
        assert_eq!(
            plain(&ls),
            ["name │  n", "─────┼───", "a    │  1", "bb   │ 22"]
        );
        assert!(
            ls[0]
                .spans
                .iter()
                .filter(|s| s.text.trim() != "│")
                .all(|s| s.style.bold || s.text.trim().is_empty())
        );
        // Too wide: the rows as written, unwrapped.
        assert_eq!(
            plain(&render_block(src, 8)),
            ["| name | n |", "| a | 1 |", "| bb | 22 |"]
        );
    }

    #[test]
    fn a_long_table_streams_whole() {
        let mut src = "| a |\n|---|\n".to_owned();
        for i in 0..30 {
            src.push_str(&format!("| {i} |\n"));
        }
        let (blocks, _) = feed(&[&src, "\n"]);
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn links_strikethrough_tasks_and_soft_breaks() {
        let ls = render_block("see [docs](https://x.dev) and <https://y.dev>", 80);
        assert_eq!(plain(&ls), ["see docs (https://x.dev) and https://y.dev"]);
        assert!(
            ls[0]
                .spans
                .iter()
                .any(|s| s.text == "docs" && s.style.underline)
        );
        let ls = render_block("~~gone~~ kept", 80);
        assert!(ls[0].spans[0].style.strike);
        assert_eq!(
            plain(&render_block("- [x] done\n- [ ] todo", 80)),
            ["- [x] done", "- [ ] todo"]
        );
        // Lines of one paragraph join and wrap together, as in markdown.
        assert_eq!(plain(&render_block("one\ntwo", 80)), ["one two"]);
        assert_eq!(plain(&render_block("Title\n=====", 80)), ["# Title"]);
    }

    #[test]
    fn quotes_hold_lists_and_indented_code_stays() {
        let ls = render_block("> - a\n> - b", 80);
        assert_eq!(plain(&ls), ["> - a", "> - b"]);
        let ls = render_block("text\n\n    code  here", 80);
        assert_eq!(plain(&ls), ["text", "code  here"]);
    }
}
