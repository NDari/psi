//! The inline differential renderer (psi-plan.md §11.1).
//!
//! The screen has two parts: committed lines, printed once and left to scroll into
//! the terminal's native scrollback, and the live region at the bottom, redrawn in
//! place. The renderer only emits bytes to its writer and tracks the cursor itself,
//! relative to the live region's first row; it never queries the terminal.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::style::Line;

const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const SHOW_CURSOR: &[u8] = b"\x1b[?25h";
const CLEAR_LINE: &[u8] = b"\x1b[2K";
const CLEAR_BELOW: &[u8] = b"\x1b[J";

/// Draws committed lines and a live region inline, without the alternate screen.
pub struct Renderer<W: Write> {
    out: W,
    width: u16,
    height: u16,
    /// The live rows on screen now, already truncated to the width.
    prev: Vec<Line>,
    /// Where the cursor was put at the end of the last frame (row in `prev`, column).
    cursor: Option<(usize, usize)>,
    /// Terminal cursor row relative to the live region's first row.
    row: usize,
    /// Terminal cursor column (only used to estimate reflow on resize).
    col: usize,
    /// Redraw the whole live region on the next draw.
    full: bool,
    buf: Vec<u8>,
    last_draw: Duration,
}

impl<W: Write> Renderer<W> {
    pub fn new(out: W, width: u16, height: u16) -> Self {
        Renderer {
            out,
            width: width.max(1),
            height: height.max(1),
            prev: Vec::new(),
            cursor: None,
            row: 0,
            col: 0,
            full: false,
            buf: Vec::with_capacity(4096),
            last_draw: Duration::ZERO,
        }
    }

    /// Starts rendering at the cursor. With `clear_screen`, first prints one newline
    /// per row so earlier content scrolls into scrollback (instead of being erased),
    /// then starts at the top of the now-empty screen.
    pub fn start(&mut self, clear_screen: bool) -> io::Result<()> {
        self.buf.clear();
        if clear_screen {
            for _ in 0..self.height {
                self.buf.extend_from_slice(b"\r\n");
            }
            self.buf.extend_from_slice(b"\x1b[H");
        } else {
            self.buf.push(b'\r');
        }
        self.prev.clear();
        self.cursor = None;
        self.row = 0;
        self.col = 0;
        self.full = false;
        self.flush_buf()
    }

    /// Prints `lines` permanently above the live region, then redraws the live
    /// region below them. Lines wider than the terminal are left for it to
    /// soft-wrap, so copied text stays exact.
    pub fn commit(&mut self, lines: &[Line]) -> io::Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        self.begin_frame();
        self.move_to(0);
        self.buf.extend_from_slice(CLEAR_BELOW);
        for line in lines {
            line.write_ansi(&mut self.buf)?;
            self.buf.extend_from_slice(b"\r\n");
        }
        // The cursor is now at column 0 of the row where the live region starts.
        self.row = 0;
        self.col = 0;
        // Re-truncate: the width may have shrunk since the last draw.
        let width = usize::from(self.width);
        let prev: Vec<Line> = self.prev.iter().map(|l| l.truncate(width)).collect();
        for (i, line) in prev.iter().enumerate() {
            self.move_to(i);
            line.write_ansi(&mut self.buf)?;
        }
        self.prev = prev;
        self.full = false;
        let cursor = self.cursor;
        self.place_cursor(cursor);
        self.end_frame()
    }

    /// Draws the live region. Only rows that changed since the last draw are
    /// rewritten. Each line is truncated to one row; if there are more than
    /// `height - 1` lines, only the last ones are kept. `cursor` is (index into
    /// `live`, display column) for a visible cursor, or None to hide it.
    pub fn draw(&mut self, live: &[Line], cursor: Option<(usize, usize)>) -> io::Result<()> {
        let started = Instant::now();
        let max_rows = usize::from(self.height).saturating_sub(1).max(1);
        let skip = live.len().saturating_sub(max_rows);
        let width = usize::from(self.width);
        let new: Vec<Line> = live[skip..].iter().map(|l| l.truncate(width)).collect();
        let cursor = cursor
            .and_then(|(r, c)| Some((r.checked_sub(skip)?, c.min(width - 1))))
            .filter(|&(r, _)| r < new.len());

        if !self.full && new == self.prev && cursor == self.cursor {
            self.last_draw = started.elapsed();
            return Ok(());
        }

        self.begin_frame();
        if self.full {
            self.move_to(0);
            self.buf.extend_from_slice(CLEAR_BELOW);
            for (i, line) in new.iter().enumerate() {
                self.move_to(i);
                line.write_ansi(&mut self.buf)?;
            }
        } else {
            for (i, line) in new.iter().enumerate() {
                if self.prev.get(i) != Some(line) {
                    self.move_to(i);
                    self.buf.extend_from_slice(CLEAR_LINE);
                    line.write_ansi(&mut self.buf)?;
                }
            }
            if new.len() < self.prev.len() {
                self.move_to(new.len());
                self.buf.extend_from_slice(CLEAR_BELOW);
            }
        }
        self.prev = new;
        self.full = false;
        self.place_cursor(cursor);
        let result = self.end_frame();
        self.last_draw = started.elapsed();
        result
    }

    /// The terminal was resized. Committed lines are left alone; the live region is
    /// cleared and fully redrawn on the next draw.
    pub fn resize(&mut self, width: u16, height: u16) {
        let (width, height) = (width.max(1), height.max(1));
        if width < self.width {
            // Most terminals reflow on a narrower width, so a row wider than the new
            // width now takes several rows. Estimate where the region start went.
            let w = usize::from(width);
            let above: usize = self.prev[..self.row.min(self.prev.len())]
                .iter()
                .map(|l| l.width().div_ceil(w).max(1))
                .sum();
            self.row = above + self.col / w;
        }
        self.row = self.row.min(usize::from(height) - 1);
        self.width = width;
        self.height = height;
        self.full = true;
    }

    /// Ends rendering: keeps the last live frame on screen with the cursor below it
    /// (`keep_live`), or clears it and leaves the cursor where it started.
    pub fn finish(&mut self, keep_live: bool) -> io::Result<()> {
        self.begin_frame();
        if keep_live {
            if self.prev.is_empty() {
                self.move_to(0);
            } else {
                self.move_to(self.prev.len() - 1);
                self.buf.extend_from_slice(b"\r\n");
            }
        } else {
            self.move_to(0);
            self.buf.extend_from_slice(CLEAR_BELOW);
        }
        self.buf.extend_from_slice(SHOW_CURSOR);
        self.prev.clear();
        self.cursor = None;
        self.row = 0;
        self.col = 0;
        self.full = false;
        self.end_frame()
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// How long the last [`Renderer::draw`] took, for the debug overlay (§11.3).
    pub fn last_draw(&self) -> Duration {
        self.last_draw
    }

    /// Rows the live region occupies now.
    pub fn live_rows(&self) -> usize {
        self.prev.len()
    }

    pub fn get_ref(&self) -> &W {
        &self.out
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.out
    }

    pub fn into_inner(self) -> W {
        self.out
    }

    fn begin_frame(&mut self) {
        self.buf.clear();
        self.buf.extend_from_slice(SYNC_BEGIN);
        self.buf.extend_from_slice(HIDE_CURSOR);
    }

    fn end_frame(&mut self) -> io::Result<()> {
        self.buf.extend_from_slice(SYNC_END);
        self.flush_buf()
    }

    fn flush_buf(&mut self) -> io::Result<()> {
        self.out.write_all(&self.buf)?;
        self.buf.clear();
        self.out.flush()
    }

    /// Moves to column 0 of `target` (relative to the region start). Moving down
    /// uses `\r\n`, which scrolls the screen when the region grows past the bottom.
    fn move_to(&mut self, target: usize) {
        if target < self.row {
            self.csi(self.row - target, b'A');
            self.buf.push(b'\r');
        } else {
            self.buf.push(b'\r');
            for _ in self.row..target {
                self.buf.extend_from_slice(b"\r\n");
            }
        }
        self.row = target;
        self.col = 0;
    }

    fn csi(&mut self, n: usize, cmd: u8) {
        // Writing to a Vec cannot fail.
        let _ = write!(self.buf, "\x1b[{n}");
        self.buf.push(cmd);
    }

    /// Puts the cursor at `cursor` and shows it, or leaves it hidden on the last row.
    fn place_cursor(&mut self, cursor: Option<(usize, usize)>) {
        match cursor {
            Some((r, c)) => {
                let c = c.min(usize::from(self.width) - 1);
                self.move_to(r);
                if c > 0 {
                    self.csi(c + 1, b'G');
                }
                self.col = c;
                self.buf.extend_from_slice(SHOW_CURSOR);
            }
            None => self.move_to(self.prev.len().saturating_sub(1)),
        }
        self.cursor = cursor;
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;
    use std::time::Instant;

    use super::*;
    use crate::style::Style;

    /// A renderer wired to a vt100 virtual terminal.
    struct Term {
        r: Renderer<Vec<u8>>,
        p: vt100::Parser,
        w: u16,
    }

    fn nz(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    fn lines(rows: &[&str]) -> Vec<Line> {
        rows.iter().map(|s| Line::raw(*s)).collect()
    }

    impl Term {
        fn new(w: u16, h: u16) -> Self {
            Term {
                r: Renderer::new(Vec::new(), w, h),
                p: vt100::Parser::new(nz(h), nz(w), 100_000),
                w,
            }
        }

        /// Feeds the renderer's new bytes to the terminal and returns them.
        fn sync(&mut self) -> String {
            let bytes = std::mem::take(self.r.get_mut());
            self.p.process(&bytes);
            String::from_utf8(bytes).unwrap()
        }

        fn draw(&mut self, rows: &[&str], cursor: Option<(usize, usize)>) -> String {
            self.r.draw(&lines(rows), cursor).unwrap();
            self.sync()
        }

        fn commit(&mut self, rows: &[&str]) -> String {
            self.r.commit(&lines(rows)).unwrap();
            self.sync()
        }

        fn screen(&self) -> Vec<String> {
            self.p.screen().rows(0, self.w).collect()
        }

        /// Scrollback then screen, oldest first.
        fn history(&mut self) -> Vec<String> {
            self.p.screen_mut().set_scrollback(usize::MAX);
            let len = self.p.screen().scrollback();
            let mut out = Vec::new();
            for off in (1..=len).rev() {
                self.p.screen_mut().set_scrollback(off);
                out.extend(self.p.screen().rows(0, self.w).next());
            }
            self.p.screen_mut().set_scrollback(0);
            out.extend(self.screen());
            out
        }

        fn cursor(&self) -> (u16, u16) {
            self.p.screen().cursor_position()
        }
    }

    #[test]
    fn start_scrolls_old_content_into_scrollback() {
        let mut t = Term::new(20, 5);
        t.p.process(b"$ psi\r\nold output\r\n");
        t.r.start(true).unwrap();
        t.sync();
        assert!(t.screen().iter().all(String::is_empty));
        assert_eq!(t.cursor(), (0, 0));
        let h = t.history();
        assert_eq!(&h[..2], ["$ psi", "old output"]);
    }

    #[test]
    fn commit_goes_above_live_region() {
        let mut t = Term::new(20, 8);
        t.r.start(true).unwrap();
        t.draw(&["status", "> hi"], Some((1, 4)));
        let bytes = t.commit(&["hello", "world"]);
        assert!(bytes.starts_with("\x1b[?2026h") && bytes.ends_with("\x1b[?2026l"));
        assert_eq!(&t.screen()[..5], ["hello", "world", "status", "> hi", ""]);
        assert_eq!(t.cursor(), (3, 4));
        assert!(!t.p.screen().hide_cursor());
        t.commit(&["third"]);
        t.draw(&["status", "> hi!"], Some((1, 5)));
        assert_eq!(
            &t.screen()[..6],
            ["hello", "world", "third", "status", "> hi!", ""]
        );
        assert_eq!(t.cursor(), (4, 5));
    }

    #[test]
    fn redraw_rewrites_only_changed_rows() {
        let mut t = Term::new(30, 10);
        t.draw(&["alpha row", "beta row", "gamma row"], None);
        let bytes = t.draw(&["alpha row", "BETA CHANGED", "gamma row"], None);
        assert!(bytes.contains("BETA CHANGED"));
        assert!(!bytes.contains("alpha") && !bytes.contains("gamma"));
        assert_eq!(&t.screen()[..3], ["alpha row", "BETA CHANGED", "gamma row"]);
        assert!(t.p.screen().hide_cursor());
        // An identical frame writes nothing.
        assert_eq!(
            t.draw(&["alpha row", "BETA CHANGED", "gamma row"], None),
            ""
        );
        // Only moving the cursor writes no row text.
        let bytes = t.draw(&["alpha row", "BETA CHANGED", "gamma row"], Some((0, 3)));
        assert!(!bytes.contains("row"));
        assert_eq!(t.cursor(), (0, 3));
    }

    #[test]
    fn shrinking_clears_old_rows() {
        let mut t = Term::new(20, 10);
        t.draw(&["one", "two", "three", "four"], None);
        t.draw(&["one", "2"], Some((1, 1)));
        assert_eq!(&t.screen()[..5], ["one", "2", "", "", ""]);
        assert_eq!(t.cursor(), (1, 1));
        t.draw(&[], None);
        assert!(t.screen().iter().all(String::is_empty));
        t.draw(&["back"], None);
        assert_eq!(t.screen()[0], "back");
    }

    #[test]
    fn growing_past_bottom_scrolls_without_losing_commits() {
        let mut t = Term::new(20, 6);
        t.r.start(true).unwrap();
        t.commit(&["c0", "c1", "c2"]);
        t.draw(&["l0"], None);
        t.draw(&["l0", "l1", "l2", "l3", "l4"], Some((4, 2)));
        assert_eq!(t.screen(), ["c1", "c2", "l0", "l1", "l2", "l3", "l4"][1..]);
        assert_eq!(t.cursor(), (5, 2));
        let h = t.history();
        let at = h.iter().position(|l| l == "c0").unwrap();
        assert_eq!(&h[at..], ["c0", "c1", "c2", "l0", "l1", "l2", "l3", "l4"]);
        // Committing at the bottom keeps everything in order too.
        t.commit(&["c3", "c4"]);
        t.draw(&["l0", "x1", "l2", "l3", "l4"], None);
        let h = t.history();
        let at = h.iter().position(|l| l == "c0").unwrap();
        assert_eq!(
            &h[at..],
            ["c0", "c1", "c2", "c3", "c4", "l0", "x1", "l2", "l3", "l4"]
        );
    }

    #[test]
    fn too_many_live_lines_keeps_the_last() {
        let mut t = Term::new(20, 5);
        let rows: Vec<String> = (0..10).map(|i| format!("l{i}")).collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        t.draw(&refs, Some((9, 1)));
        assert_eq!(t.screen(), ["l6", "l7", "l8", "l9", ""]);
        assert_eq!(t.cursor(), (3, 1));
        assert_eq!(t.r.live_rows(), 4);
        // A cursor on a dropped row is hidden.
        t.draw(&refs, Some((0, 0)));
        assert!(t.p.screen().hide_cursor());
    }

    #[test]
    fn resize_redraws_a_clean_live_region() {
        let mut t = Term::new(20, 8);
        t.r.start(true).unwrap();
        t.commit(&["committed"]);
        t.draw(&["status line", "> abc"], Some((1, 5)));
        // Wider and taller. Like vte, vt100 pulls rows back from scrollback when
        // the height grows, so check the layout relative to the committed line.
        t.p.screen_mut().set_size(nz(10), nz(30));
        t.r.resize(30, 10);
        t.w = 30;
        t.draw(&["new status", "> abc"], Some((1, 5)));
        let after = |t: &Term| {
            let s = t.screen();
            let at = s.iter().position(|l| l == "committed").unwrap();
            (at, s[at..].to_vec())
        };
        let (at, rows) = after(&t);
        assert_eq!(rows[..4], ["committed", "new status", "> abc", ""]);
        assert!(rows[3..].iter().all(String::is_empty));
        assert_eq!(t.cursor(), (at as u16 + 2, 5));
        // Narrower, with rows that still fit.
        t.p.screen_mut().set_size(nz(10), nz(12));
        t.r.resize(12, 10);
        t.w = 12;
        t.draw(&["st", "> abc", "dropdown"], Some((1, 5)));
        let (at, rows) = after(&t);
        assert_eq!(rows[..4], ["committed", "st", "> abc", "dropdown"]);
        assert!(rows[4..].iter().all(String::is_empty));
        assert_eq!(t.cursor(), (at as u16 + 2, 5));
        // Shorter: the top scrolls away, the region stays intact at the bottom.
        t.p.screen_mut().set_size(nz(4), nz(12));
        t.r.resize(12, 4);
        t.draw(&["st", "> abcd", "dd"], Some((1, 6)));
        assert_eq!(t.screen(), ["committed", "st", "> abcd", "dd"]);
        assert_eq!(t.cursor(), (2, 6));
    }

    #[test]
    fn commit_right_after_narrowing_keeps_one_row_per_live_line() {
        let mut t = Term::new(20, 8);
        t.draw(&["status line here", "> abcdefgh"], Some((1, 10)));
        t.p.screen_mut().set_size(nz(8), nz(8));
        t.r.resize(8, 8);
        t.w = 8;
        t.commit(&["c1"]);
        assert_eq!(&t.screen()[..4], ["c1", "status l", "> abcdef", ""]);
        assert_eq!(t.cursor(), (2, 7));
        t.draw(&["st", "> ab"], Some((1, 4)));
        assert_eq!(&t.screen()[..4], ["c1", "st", "> ab", ""]);
    }

    #[test]
    fn resize_estimates_reflow_of_wide_rows() {
        // A terminal that reflows turns one 20-column row into two at width 10.
        let mut r = Renderer::new(Vec::new(), 20, 10);
        r.draw(&lines(&["12345678901234567890", "> x"]), Some((1, 3)))
            .unwrap();
        r.resize(10, 10);
        assert_eq!(r.row, 2);
    }

    #[test]
    fn wide_chars_truncate_to_width() {
        let mut t = Term::new(5, 4);
        t.draw(&["你好世界", "ab🎉cd", "abcdefgh"], None);
        assert_eq!(&t.screen()[..3], ["你好", "ab🎉c", "abcde"]);
        let mut t = Term::new(3, 4);
        t.draw(&["ab🎉cd"], None);
        assert_eq!(t.screen()[0], "ab");
    }

    #[test]
    fn finish_keeps_frame_and_moves_below() {
        let mut t = Term::new(20, 6);
        t.draw(&["x", "y"], Some((0, 0)));
        t.r.finish(true).unwrap();
        t.sync();
        assert_eq!(&t.screen()[..3], ["x", "y", ""]);
        assert_eq!(t.cursor(), (2, 0));
        assert!(!t.p.screen().hide_cursor());

        let mut t = Term::new(20, 6);
        t.draw(&["x", "y"], Some((1, 1)));
        t.r.finish(false).unwrap();
        t.sync();
        assert!(t.screen().iter().all(String::is_empty));
        assert_eq!(t.cursor(), (0, 0));
        assert!(!t.p.screen().hide_cursor());
    }

    #[test]
    fn long_commits_soft_wrap_and_live_follows() {
        let mut t = Term::new(10, 8);
        t.r.start(true).unwrap();
        t.draw(&["live"], Some((0, 4)));
        t.commit(&["0123456789abcdef", "0123456789", "end"]);
        assert_eq!(
            &t.screen()[..6],
            ["0123456789", "abcdef", "0123456789", "end", "live", ""]
        );
        assert!(t.p.screen().row_wrapped(0));
        assert_eq!(t.cursor(), (4, 4));
    }

    #[test]
    fn styles_reach_the_terminal() {
        let mut t = Term::new(20, 4);
        let line = Line::raw("a").with("B", Style::new().bold());
        t.r.draw(&[line], None).unwrap();
        t.sync();
        let cell = t.p.screen().cell(0, 1).unwrap();
        assert!(cell.bold());
        assert!(!t.p.screen().cell(0, 0).unwrap().bold());
    }

    #[test]
    fn draw_is_fast_after_many_commits() {
        let mut r = Renderer::new(Vec::new(), 120, 40);
        r.start(true).unwrap();
        for chunk in 0..100 {
            let rows: Vec<Line> = (0..100)
                .map(|i| Line::raw(format!("committed line {chunk}-{i} with some text")))
                .collect();
            r.commit(&rows).unwrap();
        }
        let frame = |n: usize| -> Vec<Line> {
            (0..30)
                .map(|i| {
                    Line::raw(format!("live row {i} frame {n} "))
                        .with("x".repeat(80), Style::fg(crate::Color::Green))
                })
                .collect()
        };
        let limit = if cfg!(debug_assertions) {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(16)
        };
        r.draw(&frame(0), Some((29, 5))).unwrap();
        for n in 1..20 {
            r.get_mut().clear();
            let f = frame(n);
            let started = Instant::now();
            r.draw(&f, Some((29, 5))).unwrap();
            let took = started.elapsed();
            assert!(took < limit, "draw took {took:?}");
            assert!(r.last_draw() <= took);
        }
        assert!(r.last_draw() > Duration::ZERO);
    }
}
