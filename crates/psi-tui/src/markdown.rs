//! Streaming markdown, committed once (psi-plan.md §11.2).
//!
//! [`MdStream`] splits streamed text into complete blocks (paragraphs, headings,
//! whole fenced code blocks) and [`render_block`] turns a block into lines. This is
//! deliberately not CommonMark: anything it does not recognize renders literally.

use crate::style::{Line, Style};
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
                    if self.lines >= PROGRESSIVE_LINES {
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

/// Lines for one block. Prose is word-wrapped to `width` with headings bold (the
/// `#`s dim), `**bold**`, `*italic*`/`_italic_`, `` `code` `` in cyan, list bullets
/// kept with a hanging indent, and block quotes dim. Fenced code keeps its fence
/// lines (dim) and its code lines verbatim and unwrapped, so a copy from the
/// terminal is the code. Table rows are not wrapped either.
pub fn render_block(block: &str, width: usize) -> Vec<Line> {
    let mut out = Vec::new();
    let mut fence = None;
    for raw in block.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(f) = fence {
            if fence_close(line, f) {
                fence = None;
                out.push(Line::styled(line, theme::style(Token::MdCodeBlockBorder)));
            } else {
                out.push(Line::raw(line));
            }
        } else if let Some(f) = fence_open(line) {
            fence = Some(f);
            out.push(Line::styled(line, theme::style(Token::MdCodeBlockBorder)));
        } else {
            prose_line(line, width, &mut out);
        }
    }
    out
}

fn prose_line(line: &str, width: usize, out: &mut Vec<Line>) {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    if trimmed.is_empty() {
        out.push(Line::new());
    } else if let Some(n) = heading_level(trimmed) {
        let marks = &trimmed[..n];
        let rest = trimmed[n..].trim_start();
        let first = Line::raw(indent).with(format!("{marks} "), muted());
        wrap_prefixed(
            first,
            false,
            inline(rest, theme::style(Token::MdHeading).bold()),
            width,
            out,
        );
    } else if let Some(rest) = trimmed.strip_prefix('>') {
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        let first = Line::raw(indent).with("> ", muted());
        wrap_prefixed(first, true, inline(rest, muted()), width, out);
    } else if is_rule(trimmed) {
        out.push(Line::styled(line, muted()));
    } else if trimmed.starts_with('|') {
        out.push(inline(line, Style::new()));
    } else if let Some(marker) = list_marker(trimmed) {
        let first = Line::raw(format!("{indent}{marker}"));
        let body = inline(&trimmed[marker.len()..], Style::new());
        wrap_prefixed(first, false, body, width, out);
    } else {
        out.extend(inline(line, Style::new()).wrap(width));
    }
}

/// Wraps `body` after `first`; later rows get `first` again (`repeat`) or spaces.
fn wrap_prefixed(first: Line, repeat: bool, body: Line, width: usize, out: &mut Vec<Line>) {
    let pw = first.width();
    let cont = if repeat {
        first.clone()
    } else {
        Line::raw(" ".repeat(pw))
    };
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

fn is_rule(t: &str) -> bool {
    let chars: Vec<char> = t.chars().filter(|c| *c != ' ').collect();
    chars.len() >= 3 && matches!(chars[0], '-' | '*' | '_') && chars.iter().all(|&c| c == chars[0])
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

/// Inline markup in `base` style. Unmatched markers render literally.
fn inline(s: &str, base: Style) -> Line {
    let mut out = Line::new();
    inline_into(&s.chars().collect::<Vec<_>>(), base, &mut out);
    out
}

fn run(cs: &[char], i: usize, c: char) -> usize {
    cs[i..].iter().take_while(|&&x| x == c).count()
}

fn inline_into(cs: &[char], base: Style, out: &mut Line) {
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Line| {
        out.push(std::mem::take(text), base);
    };
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '\\' if cs.get(i + 1).is_some_and(char::is_ascii_punctuation) => {
                text.push(cs[i + 1]);
                i += 2;
            }
            '`' => {
                let n = run(cs, i, '`');
                match find_run(cs, i + n, '`', n) {
                    Some(j) => {
                        flush(&mut text, out);
                        let code: String = cs[i + n..j].iter().collect();
                        let code = match code.strip_prefix(' ').and_then(|c| c.strip_suffix(' ')) {
                            Some(inner) if !inner.trim().is_empty() => inner.to_string(),
                            _ => code,
                        };
                        out.push(code, base.patch(code_style()));
                        i = j + n;
                    }
                    None => {
                        text.extend(&cs[i..i + n]);
                        i += n;
                    }
                }
            }
            '*' | '_' => {
                let n = run(cs, i, c);
                let open_ok = c == '*' || i == 0 || !cs[i - 1].is_alphanumeric();
                let close = match n {
                    1 | 2 if open_ok => find_close(cs, i + n, c, n),
                    _ => None,
                };
                match close {
                    Some(j) => {
                        flush(&mut text, out);
                        let style = if n == 2 { base.bold() } else { base.italic() };
                        inline_into(&cs[i + n..j], style, out);
                        i = j + n;
                    }
                    None => {
                        text.extend(&cs[i..i + n]);
                        i += n;
                    }
                }
            }
            _ => {
                text.push(c);
                i += 1;
            }
        }
    }
    flush(&mut text, out);
}

/// The next run of exactly `n` `c`s at or after `from`.
fn find_run(cs: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    let mut j = from;
    while j < cs.len() {
        if cs[j] == c {
            let r = run(cs, j, c);
            if r == n {
                return Some(j);
            }
            j += r;
        } else {
            j += 1;
        }
    }
    None
}

/// A closing emphasis run for an opener whose content starts at `from`: content is
/// non-empty and not space-padded, and `_` does not close inside a word.
fn find_close(cs: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    if cs.get(from).is_none_or(|x| x.is_whitespace()) {
        return None;
    }
    let mut j = from + 1;
    while j < cs.len() {
        if cs[j] == c {
            let r = run(cs, j, c);
            let after_ok = c == '*' || cs.get(j + r).is_none_or(|x| !x.is_alphanumeric());
            if r == n && !cs[j - 1].is_whitespace() && after_ok {
                return Some(j);
            }
            j += r;
        } else {
            j += 1;
        }
    }
    None
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
        assert_eq!(ls[1].spans[0].style, Style::new());
        assert_eq!(
            plain(&render_block("```\n**not bold**\n```", 80))[1],
            "**not bold**"
        );
    }

    #[test]
    fn render_prose_wraps() {
        let ls = render_block("one two three four five six", 10);
        assert_eq!(plain(&ls), ["one two", "three four", "five six"]);
    }

    #[test]
    fn render_lists_with_hanging_indent() {
        let ls = render_block("- alpha beta gamma\n  12. delta epsilon", 13);
        assert_eq!(
            plain(&ls),
            ["- alpha beta", "  gamma", "  12. delta", "      epsilon"]
        );
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
}
