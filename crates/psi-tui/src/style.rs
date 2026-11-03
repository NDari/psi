//! Styled text: the 16 ANSI colors, spans and lines, with width-aware truncate and wrap.

use std::io::{self, Write};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Columns a tab expands to.
pub const TAB_WIDTH: usize = 4;

/// One of the terminal's 16 ANSI colors, so the user's theme decides the actual shade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    /// A 256-color palette index, from a theme.
    Indexed(u8),
    /// A true color, from a theme.
    Rgb(u8, u8, u8),
}

impl Color {
    /// The SGR parameters for this color as foreground, or as background.
    fn sgr(self, bg: bool) -> String {
        let base = if bg { 48 } else { 38 };
        match self {
            Color::Indexed(n) => format!("{base};5;{n}"),
            Color::Rgb(r, g, b) => format!("{base};2;{r};{g};{b}"),
            c => (c.fg_code() + if bg { 10 } else { 0 }).to_string(),
        }
    }

    /// SGR foreground code (30-37, 90-97); background is this plus 10.
    fn fg_code(self) -> u8 {
        match self {
            Color::Black => 30,
            Color::Red => 31,
            Color::Green => 32,
            Color::Yellow => 33,
            Color::Blue => 34,
            Color::Magenta => 35,
            Color::Cyan => 36,
            Color::White => 37,
            Color::BrightBlack => 90,
            Color::BrightRed => 91,
            Color::BrightGreen => 92,
            Color::BrightYellow => 93,
            Color::BrightBlue => 94,
            Color::BrightMagenta => 95,
            Color::BrightCyan => 96,
            Color::BrightWhite => 97,
            // Written by `sgr`; never reached for these.
            Color::Indexed(_) | Color::Rgb(..) => 39,
        }
    }
}

/// Colors and attributes for a span. The default is the terminal's own style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub strike: bool,
}

impl Style {
    /// The terminal's default style.
    pub const fn new() -> Self {
        Style {
            fg: None,
            bg: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            reverse: false,
            strike: false,
        }
    }

    /// A style with only a foreground color.
    pub const fn fg(c: Color) -> Self {
        Style::new().with_fg(c)
    }

    /// A style with only a background color.
    pub const fn bg(c: Color) -> Self {
        Style::new().with_bg(c)
    }

    pub const fn with_fg(mut self, c: Color) -> Self {
        self.fg = Some(c);
        self
    }

    pub const fn with_bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub const fn reverse(mut self) -> Self {
        self.reverse = true;
        self
    }

    pub const fn strike(mut self) -> Self {
        self.strike = true;
        self
    }

    /// `other` layered on top of `self`: its colors win when set, attributes add up.
    pub const fn patch(self, other: Style) -> Self {
        Style {
            fg: match other.fg {
                Some(c) => Some(c),
                None => self.fg,
            },
            bg: match other.bg {
                Some(c) => Some(c),
                None => self.bg,
            },
            bold: self.bold || other.bold,
            dim: self.dim || other.dim,
            italic: self.italic || other.italic,
            underline: self.underline || other.underline,
            reverse: self.reverse || other.reverse,
            strike: self.strike || other.strike,
        }
    }

    fn is_plain(&self) -> bool {
        *self == Style::new()
    }

    /// Writes `ESC[0;...m` for this style (callers only call it for a non-plain style).
    fn write_sgr(&self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(b"\x1b[0")?;
        let attrs = [
            (self.bold, 1),
            (self.dim, 2),
            (self.italic, 3),
            (self.underline, 4),
            (self.reverse, 7),
            (self.strike, 9),
        ];
        for (on, code) in attrs {
            if on {
                write!(out, ";{code}")?;
            }
        }
        if let Some(c) = self.fg {
            write!(out, ";{}", c.sgr(false))?;
        }
        if let Some(c) = self.bg {
            write!(out, ";{}", c.sgr(true))?;
        }
        out.write_all(b"m")
    }
}

/// Makes text safe to put on one terminal row: a tab becomes spaces, a newline a
/// space, escape sequences and other control or bidi-override characters are removed.
fn apply_sgr(mut st: Style, params: &str) -> Style {
    const COLORS: [Color; 8] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::White,
    ];
    const BRIGHT: [Color; 8] = [
        Color::BrightBlack,
        Color::BrightRed,
        Color::BrightGreen,
        Color::BrightYellow,
        Color::BrightBlue,
        Color::BrightMagenta,
        Color::BrightCyan,
        Color::BrightWhite,
    ];
    for p in params.split(';') {
        match p.parse::<u8>().unwrap_or(0) {
            0 => st = Style::default(),
            1 => st.bold = true,
            2 => st.dim = true,
            22 => {
                st.bold = false;
                st.dim = false;
            }
            39 => st.fg = None,
            n @ 30..=37 => st.fg = Some(COLORS[(n - 30) as usize]),
            n @ 90..=97 => st.fg = Some(BRIGHT[(n - 90) as usize]),
            _ => {}
        }
    }
    st
}

pub fn sanitize(s: &str) -> String {
    if !s.chars().any(needs_sanitizing) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\t' => out.extend(std::iter::repeat_n(' ', TAB_WIDTH)),
            '\n' => out.push(' '),
            '\x1b' => skip_escape(&mut chars),
            c if needs_sanitizing(c) => {}
            c => out.push(c),
        }
    }
    out
}

fn needs_sanitizing(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Consumes the rest of an escape sequence whose ESC was already read.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        // CSI: parameters and intermediates, then one final byte.
        Some('[') => {
            for c in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&c) {
                    break;
                }
            }
        }
        // OSC, DCS, SOS, PM, APC: up to BEL or ST (ESC \).
        Some(']' | 'P' | 'X' | '^' | '_') => {
            while let Some(c) = chars.next() {
                if c == '\x07' {
                    break;
                }
                if c == '\x1b' {
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
            }
        }
        // Other sequences: intermediates, then one final byte.
        Some(c) if ('\x20'..='\x2f').contains(&c) => {
            while let Some(&n) = chars.peek() {
                chars.next();
                if !('\x20'..='\x2f').contains(&n) {
                    break;
                }
            }
        }
        _ => {}
    }
}

/// Display width of a string, summed by grapheme.
pub fn str_width(s: &str) -> usize {
    s.graphemes(true).map(UnicodeWidthStr::width).sum()
}

/// One grapheme of a line, with its style, display width and link.
type Cell<'a> = (&'a str, Style, usize, Option<&'a String>);

/// A run of text in one style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// A hyperlink (OSC 8) around the text. Only Psi sets one; text from
    /// tools and models has its escapes stripped.
    pub link: Option<String>,
}

impl Span {
    /// A span with sanitized text.
    pub fn new(text: impl AsRef<str>, style: Style) -> Self {
        Span {
            text: sanitize(text.as_ref()),
            style,
            link: None,
        }
    }
}

/// One terminal row of styled text (committed lines may be wider and soft-wrap).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl From<&str> for Line {
    fn from(s: &str) -> Self {
        Line::raw(s)
    }
}

impl From<String> for Line {
    fn from(s: String) -> Self {
        Line::raw(s)
    }
}

impl Line {
    /// A line from text with SGR escapes (as `psi.style` makes them): the
    /// 16 colors, bold and dim become styles; any other escape is dropped.
    pub fn from_ansi(s: &str) -> Self {
        let mut line = Line::new();
        let mut style = Style::default();
        let mut rest = s;
        while let Some(i) = rest.find('\x1b') {
            if i > 0 {
                line.push(&rest[..i], style);
            }
            rest = &rest[i + 1..];
            if let Some(osc) = rest.strip_prefix(']') {
                // OSC: up to BEL or ESC \.
                let end = osc.find(['\x07', '\x1b']).unwrap_or(osc.len());
                let skip = if osc[end..].starts_with('\x1b') { 2 } else { 1 };
                rest = osc.get(end + skip..).unwrap_or("");
                continue;
            }
            let Some(body) = rest.strip_prefix('[') else {
                // Another escape: drop the escape and its one character.
                rest = rest
                    .get(rest.chars().next().map_or(0, char::len_utf8)..)
                    .unwrap_or("");
                continue;
            };
            let end = body
                .find(|c: char| c.is_ascii_alphabetic())
                .unwrap_or(body.len());
            if body[end..].starts_with('m') {
                style = apply_sgr(style, &body[..end]);
            }
            rest = body.get(end + 1..).unwrap_or("");
        }
        if !rest.is_empty() {
            line.push(rest, style);
        }
        line
    }

    pub fn new() -> Self {
        Line::default()
    }

    /// Unstyled text.
    pub fn raw(s: impl AsRef<str>) -> Self {
        Line::styled(s, Style::new())
    }

    /// Text in one style.
    pub fn styled(s: impl AsRef<str>, style: Style) -> Self {
        let mut line = Line::new();
        line.push(s, style);
        line
    }

    /// One line per `\n`-separated line of `s` (a `\r\n` counts as one break).
    pub fn split_lines(s: &str, style: Style) -> Vec<Line> {
        s.split('\n')
            .map(|l| Line::styled(l.strip_suffix('\r').unwrap_or(l), style))
            .collect()
    }

    /// Appends sanitized text, merging with the last span when the style matches.
    pub fn push(&mut self, s: impl AsRef<str>, style: Style) -> &mut Self {
        let text = sanitize(s.as_ref());
        self.push_clean(text, style);
        self
    }

    /// Builder form of [`Line::push`].
    pub fn with(mut self, s: impl AsRef<str>, style: Style) -> Self {
        self.push(s, style);
        self
    }

    /// Appends text that is a hyperlink to `url` (OSC 8). A URL that isn't
    /// http, https, mailto or file, or holds control characters, is left out
    /// and the text is plain.
    pub fn push_link(&mut self, s: impl AsRef<str>, style: Style, url: &str) -> &mut Self {
        let text = sanitize(s.as_ref());
        self.push_span(text, style, link_target(url));
        self
    }

    /// Makes the text from byte `at` of [`Line::plain`] to the end a link.
    pub fn link_from(&mut self, at: usize, url: &str) {
        let Some(url) = link_target(url) else { return };
        let mut start = 0;
        let mut out = Vec::with_capacity(self.spans.len() + 1);
        for mut span in std::mem::take(&mut self.spans) {
            let end = start + span.text.len();
            if end > at && start < at && span.text.is_char_boundary(at - start) {
                let tail = span.text.split_off(at - start);
                out.push(span.clone());
                span.text = tail;
            }
            if end > at {
                span.link = Some(url.clone());
            }
            start = end;
            out.push(span);
        }
        for s in out {
            self.push_span(s.text, s.style, s.link);
        }
    }

    /// Appends all spans of `other`.
    pub fn append(&mut self, other: Line) -> &mut Self {
        for span in other.spans {
            self.push_span(span.text, span.style, span.link);
        }
        self
    }

    fn push_clean(&mut self, text: String, style: Style) {
        self.push_span(text, style, None);
    }

    fn push_span(&mut self, text: String, style: Style, link: Option<String>) {
        if text.is_empty() {
            return;
        }
        match self.spans.last_mut() {
            Some(last) if last.style == style && last.link == link => last.text.push_str(&text),
            _ => self.spans.push(Span { text, style, link }),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|s| s.text.is_empty())
    }

    /// Display width in columns.
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| str_width(&s.text)).sum()
    }

    /// The text without styles.
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    /// At most `width` columns, never splitting a grapheme (a wide char that would
    /// straddle the edge is dropped).
    pub fn truncate(&self, width: usize) -> Line {
        let mut out = Line::new();
        let mut used = 0;
        for span in &self.spans {
            let w = str_width(&span.text);
            if used + w <= width {
                used += w;
                out.push_span(span.text.clone(), span.style, span.link.clone());
                continue;
            }
            let mut text = String::new();
            for g in span.text.graphemes(true) {
                let gw = g.width();
                if used + gw > width {
                    break;
                }
                used += gw;
                text.push_str(g);
            }
            out.push_span(text, span.style, span.link.clone());
            break;
        }
        out
    }

    /// Word wrap to `width` columns, keeping styles. Spaces at a break are dropped;
    /// a word longer than `width` is split. Always returns at least one line.
    pub fn wrap(&self, width: usize) -> Vec<Line> {
        let width = width.max(1);
        if self.width() <= width {
            return vec![self.clone()];
        }
        // Flatten into graphemes, then group into alternating space / word tokens.
        let cells: Vec<Cell<'_>> = self
            .spans
            .iter()
            .flat_map(|s| {
                s.text
                    .graphemes(true)
                    .map(move |g| (g, s.style, g.width(), s.link.as_ref()))
            })
            .collect();
        let mut tokens: Vec<(bool, &[Cell<'_>])> = Vec::new();
        let mut start = 0;
        for i in 1..=cells.len() {
            let space = |j: usize| cells[j].0 == " ";
            if i == cells.len() || space(i) != space(start) {
                tokens.push((space(start), &cells[start..i]));
                start = i;
            }
        }

        let mut lines = Vec::new();
        let mut cur = Line::new();
        let mut cur_w = 0;
        let mut pending_space: &[Cell<'_>] = &[];
        let put = |line: &mut Line, cells: &[Cell<'_>]| {
            for (g, st, _, link) in cells {
                line.push_span((*g).to_string(), *st, link.cloned());
            }
        };
        for (is_space, toks) in tokens {
            if is_space {
                // Leading spaces on the very first line are indentation: keep them.
                if lines.is_empty() && cur_w == 0 {
                    pending_space = toks;
                } else {
                    pending_space = if cur_w == 0 { &[] } else { toks };
                }
                continue;
            }
            let sw: usize = pending_space.iter().map(|c| c.2).sum();
            let ww: usize = toks.iter().map(|c| c.2).sum();
            if cur_w + sw + ww <= width {
                put(&mut cur, pending_space);
                put(&mut cur, toks);
                cur_w += sw + ww;
            } else if ww <= width {
                if cur_w > 0 {
                    lines.push(std::mem::take(&mut cur));
                }
                put(&mut cur, toks);
                cur_w = ww;
            } else {
                // Split a long word: fill the current line, then whole lines.
                if cur_w > 0 && cur_w + sw < width {
                    put(&mut cur, pending_space);
                    cur_w += sw;
                } else if cur_w > 0 {
                    lines.push(std::mem::take(&mut cur));
                    cur_w = 0;
                }
                for cell in toks {
                    if cur_w + cell.2 > width && cur_w > 0 {
                        lines.push(std::mem::take(&mut cur));
                        cur_w = 0;
                    }
                    put(&mut cur, std::slice::from_ref(cell));
                    cur_w += cell.2;
                }
            }
            pending_space = &[];
        }
        lines.push(cur);
        lines
    }

    /// Writes the line with SGR codes, resetting at the end if any style was used.
    /// Control characters are skipped even if a span was built by hand.
    pub fn write_ansi(&self, out: &mut impl Write) -> io::Result<()> {
        let mut current = Style::new();
        let mut link: Option<&String> = None;
        for span in &self.spans {
            if span.text.is_empty() {
                continue;
            }
            if span.link.as_ref() != link {
                if link.is_some() {
                    out.write_all(b"\x1b]8;;\x1b\\")?;
                }
                if let Some(url) = &span.link {
                    write!(out, "\x1b]8;;{url}\x1b\\")?;
                }
                link = span.link.as_ref();
            }
            if span.style != current {
                if span.style.is_plain() {
                    out.write_all(b"\x1b[0m")?;
                } else {
                    span.style.write_sgr(out)?;
                }
                current = span.style;
            }
            if span.text.chars().any(needs_sanitizing) {
                out.write_all(sanitize(&span.text).as_bytes())?;
            } else {
                out.write_all(span.text.as_bytes())?;
            }
        }
        if link.is_some() {
            out.write_all(b"\x1b]8;;\x1b\\")?;
        }
        if !current.is_plain() {
            out.write_all(b"\x1b[0m")?;
        }
        Ok(())
    }
}

/// A URL fit for an OSC 8 link: a known scheme, no control characters or
/// spaces, not absurdly long.
fn link_target(url: &str) -> Option<String> {
    let known = ["http://", "https://", "mailto:", "file://"]
        .iter()
        .any(|p| {
            url.len() > p.len()
                && url
                    .get(..p.len())
                    .is_some_and(|s| s.eq_ignore_ascii_case(p))
        });
    let clean = !url.chars().any(|c| c.is_control() || c.is_whitespace());
    (known && clean && url.len() <= 2048).then(|| url.to_owned())
}

/// A `file://` URL for a local path (OSC 8), with the bytes a URL can't hold
/// percent-encoded.
pub fn file_url(path: &std::path::Path) -> Option<String> {
    let abs = std::path::absolute(path).ok()?;
    let mut url = String::from("file://");
    for b in abs.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    Some(url)
}

#[cfg(test)]
mod ansi_tests {
    use super::*;

    #[test]
    fn links_open_and_close_around_their_text() {
        let mut l = Line::raw("see ");
        l.push_link("docs", Style::new().underline(), "https://example.com/a");
        l.push(" now", Style::new());
        let mut out = Vec::new();
        l.write_ansi(&mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(
            s,
            "see \x1b]8;;https://example.com/a\x1b\\\x1b[0;4mdocs\x1b]8;;\x1b\\\x1b[0m now"
        );
        // Truncating and wrapping keep the link.
        assert_eq!(
            l.truncate(6).spans[1].link.as_deref(),
            Some("https://example.com/a")
        );
        let wrapped = l.wrap(5);
        assert!(
            wrapped
                .iter()
                .any(|w| w.spans.iter().any(|s| s.link.is_some()))
        );
    }

    #[test]
    fn only_known_schemes_become_links() {
        let mut l = Line::new();
        l.push_link("x", Style::new(), "javascript:alert(1)");
        l.push_link("y", Style::new(), "https://a b");
        assert!(l.spans.iter().all(|s| s.link.is_none()));
        let mut m = Line::raw("text link");
        m.link_from(5, "https://example.com");
        assert_eq!(m.spans.len(), 2);
        assert_eq!(m.spans[1].text, "link");
        assert_eq!(m.spans[1].link.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn file_urls_are_absolute_and_encoded() {
        let u = file_url(std::path::Path::new("/tmp/a b/ü.rs")).unwrap();
        assert_eq!(u, "file:///tmp/a%20b/%C3%BC.rs");
    }

    #[test]
    fn sgr_becomes_styles_and_other_escapes_drop() {
        let l = Line::from_ansi("a\x1b[31mred\x1b[39m \x1b[1mbold\x1b[22m\x1b]0;title\x07!");
        assert_eq!(l.plain(), "ared bold!");
        assert_eq!(l.spans[1].style.fg, Some(Color::Red));
        assert!(l.spans.iter().any(|sp| sp.text == "bold" && sp.style.bold));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ansi(line: &Line) -> String {
        let mut v = Vec::new();
        line.write_ansi(&mut v).unwrap();
        String::from_utf8(v).unwrap()
    }

    #[test]
    fn sanitize_controls() {
        assert_eq!(sanitize("a\tb"), "a    b");
        assert_eq!(sanitize("x\x1b[31mred\x1b[0m"), "xred");
        assert_eq!(sanitize("t\x1b]0;title\x07end"), "tend");
        assert_eq!(sanitize("t\x1b]8;;http://x\x1b\\link"), "tlink");
        assert_eq!(sanitize("a\rb\x00c\x7f\u{9b}d"), "abcd");
        assert_eq!(sanitize("line\nnext"), "line next");
        assert_eq!(sanitize("\x1b(Bok"), "ok");
        assert_eq!(sanitize("rtl\u{202e}x"), "rtlx");
        assert_eq!(sanitize("plain ü 你"), "plain ü 你");
    }

    #[test]
    fn width_counts_wide_chars() {
        assert_eq!(Line::raw("abc").width(), 3);
        assert_eq!(Line::raw("你好").width(), 4);
        assert_eq!(Line::raw("a🎉b").width(), 4);
        assert_eq!(Line::raw("\tx").width(), 5);
        assert_eq!(Line::raw("e\u{301}").width(), 1);
    }

    #[test]
    fn truncate_never_splits_wide() {
        let l = Line::raw("你好世界");
        assert_eq!(l.truncate(5).plain(), "你好");
        assert_eq!(l.truncate(4).plain(), "你好");
        assert_eq!(Line::raw("ab🎉cd").truncate(3).plain(), "ab");
        assert_eq!(Line::raw("ab🎉cd").truncate(4).plain(), "ab🎉");
        // A combining mark stays with its base.
        assert_eq!(Line::raw("ae\u{301}x").truncate(2).plain(), "ae\u{301}");
    }

    #[test]
    fn truncate_keeps_styles() {
        let l = Line::raw("ab")
            .with("cd", Style::fg(Color::Red))
            .with("ef", Style::new());
        let t = l.truncate(3);
        assert_eq!(t.spans.len(), 2);
        assert_eq!(t.spans[1], Span::new("c", Style::fg(Color::Red)));
    }

    #[test]
    fn wrap_words() {
        let l = Line::raw("the quick brown fox jumps");
        let w: Vec<String> = l.wrap(10).iter().map(Line::plain).collect();
        assert_eq!(w, ["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn wrap_splits_long_words_and_keeps_styles() {
        let l = Line::raw("ab ").with("cdefghijkl", Style::new().bold());
        let w = l.wrap(5);
        let plain: Vec<String> = w.iter().map(Line::plain).collect();
        assert_eq!(plain, ["ab cd", "efghi", "jkl"]);
        assert!(w[1].spans[0].style.bold);
        assert!(!w[0].spans[0].style.bold);
        assert!(w.iter().all(|l| l.width() <= 5));
    }

    #[test]
    fn wrap_wide_chars() {
        let w = Line::raw("你好世界你好").wrap(5);
        assert!(w.iter().all(|l| l.width() <= 5));
        assert_eq!(
            w.iter().map(Line::plain).collect::<String>(),
            "你好世界你好"
        );
    }

    #[test]
    fn wrap_keeps_indent_and_handles_empty() {
        let w: Vec<String> = Line::raw("  aaa bbb")
            .wrap(6)
            .iter()
            .map(Line::plain)
            .collect();
        assert_eq!(w, ["  aaa", "bbb"]);
        assert_eq!(Line::new().wrap(5), vec![Line::new()]);
    }

    #[test]
    fn ansi_output() {
        assert_eq!(ansi(&Line::raw("hi")), "hi");
        let l = Line::raw("a")
            .with("b", Style::fg(Color::Red).bold())
            .with("c", Style::bg(Color::BrightBlue).dim());
        assert_eq!(ansi(&l), "a\x1b[0;1;31mb\x1b[0;2;104mc\x1b[0m");
        let hand = Line {
            spans: vec![Span {
                text: "x\x1b[2Jy".into(),
                style: Style::new(),
                link: None,
            }],
        };
        assert_eq!(ansi(&hand), "xy");
    }

    #[test]
    fn split_lines_crlf() {
        let ls = Line::split_lines("a\r\nb\nc", Style::new());
        assert_eq!(
            ls.iter().map(Line::plain).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }
}
