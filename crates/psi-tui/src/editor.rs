//! A multi-line input editor with history (psi-plan.md §11.4).
//!
//! The cursor column is a grapheme index into the line; display columns come from
//! unicode-width, so CJK and emoji count as two.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::keys::{Action, Keymap};
use crate::style::{Line, Style, TAB_WIDTH, sanitize, str_width};

/// What a key did.
#[derive(Debug, PartialEq, Eq)]
pub enum EditorEvent {
    /// The key is not handled here; the caller may use it.
    None,
    /// The text or cursor changed; redraw.
    Changed,
    /// Enter: the whole text (possibly empty). The editor is cleared and the text
    /// pushed to history.
    Submit(String),
}

/// A multi-line text input with history browsing.
#[derive(Clone, Debug)]
pub struct Editor {
    lines: Vec<String>,
    row: usize,
    /// Grapheme index into `lines[row]`.
    col: usize,
    history: Vec<String>,
    /// Index into `history` while browsing it.
    hist_pos: Option<usize>,
    /// The unsent text, saved when history browsing starts.
    draft: String,
    keymap: Keymap,
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

fn is_ws(g: &str) -> bool {
    g.chars().all(char::is_whitespace)
}

fn grapheme_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Byte offset of grapheme `g` in `s` (the end of `s` when past it).
fn byte_at(s: &str, g: usize) -> usize {
    s.grapheme_indices(true).nth(g).map_or(s.len(), |(i, _)| i)
}

impl Editor {
    pub fn new() -> Self {
        Editor {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            history: Vec::new(),
            hist_pos: None,
            draft: String::new(),
            keymap: Keymap::default(),
        }
    }

    /// The keys the editor's actions use.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
    }

    /// Handles one key. Alt+Enter, Tab, Esc and other unbound keys return
    /// [`EditorEvent::None`] so the caller can bind them.
    pub fn handle(&mut self, key: KeyEvent) -> EditorEvent {
        if key.kind == KeyEventKind::Release {
            return EditorEvent::None;
        }
        let m = key.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        let alt = m.contains(KeyModifiers::ALT);
        let km = &self.keymap;
        let action = [
            Action::NewLine,
            Action::Submit,
            Action::CursorUp,
            Action::CursorDown,
            Action::CursorLeft,
            Action::CursorRight,
            Action::CursorWordLeft,
            Action::CursorWordRight,
            Action::CursorLineStart,
            Action::CursorLineEnd,
            Action::DeleteCharBackward,
            Action::DeleteCharForward,
            Action::DeleteWordBackward,
            Action::DeleteToLineStart,
            Action::DeleteToLineEnd,
        ]
        .into_iter()
        .find(|a| km.is(*a, &key));
        let changed = match action {
            Some(Action::NewLine) => self.edit(|e| e.insert_newline()),
            Some(Action::Submit) => {
                if self.col > 0 && self.grapheme_before() == Some("\\") {
                    self.edit(|e| {
                        e.backspace();
                        e.insert_newline();
                    })
                } else {
                    let text = self.text();
                    self.push_history(&text);
                    self.clear();
                    return EditorEvent::Submit(text);
                }
            }
            Some(Action::CursorUp) => self.up(),
            Some(Action::CursorDown) => self.down(),
            Some(Action::CursorLeft) => self.left(),
            Some(Action::CursorRight) => self.right(),
            Some(Action::CursorWordLeft) => self.word_left(),
            Some(Action::CursorWordRight) => self.word_right(),
            Some(Action::CursorLineStart) => self.move_to(self.row, 0),
            Some(Action::CursorLineEnd) => self.move_to(self.row, self.line_len()),
            Some(Action::DeleteCharBackward) => self.edit(|e| e.backspace()),
            Some(Action::DeleteCharForward) => self.edit(|e| e.delete_forward()),
            Some(Action::DeleteWordBackward) => self.edit(|e| e.delete_word_back()),
            Some(Action::DeleteToLineStart) => {
                self.col > 0 && self.edit(|e| e.delete_range(0, e.col))
            }
            Some(Action::DeleteToLineEnd) => {
                self.col < self.line_len() && self.edit(|e| e.delete_range(e.col, e.line_len()))
            }
            _ => match key.code {
                // Plain, shifted, or Ctrl+Alt (AltGr on some platforms).
                KeyCode::Char(c) if c.is_control() || ctrl != alt => false,
                KeyCode::Char(c) => self.edit(|e| e.insert_clean(c.encode_utf8(&mut [0; 4]))),
                _ => false,
            },
        };
        if changed {
            EditorEvent::Changed
        } else {
            EditorEvent::None
        }
    }

    /// Inserts pasted text: `\r\n` and lone `\r` become line breaks, tabs become
    /// spaces, other control characters are dropped.
    pub fn insert_str(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        self.edit(|e| {
            for (i, part) in s.split('\n').enumerate() {
                if i > 0 {
                    e.insert_newline();
                }
                e.insert_clean(&sanitize(part));
            }
        });
    }

    /// The whole text, lines joined with `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Replaces the text and puts the cursor at the end.
    pub fn set_text(&mut self, s: &str) {
        self.hist_pos = None;
        self.load(s);
    }

    pub fn clear(&mut self) {
        self.set_text("");
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// (row, grapheme index in that row).
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Adds an entry, skipping blank ones and a repeat of the newest.
    pub fn push_history(&mut self, s: &str) {
        if s.trim().is_empty() || self.history.last().is_some_and(|l| l == s) {
            return;
        }
        self.history.push(s.to_string());
    }

    /// Replaces the newest entry, when the text sent differs from the text
    /// typed (a paste placeholder swapped back for the paste).
    pub fn replace_last_history(&mut self, s: &str) {
        if let Some(last) = self.history.last_mut() {
            *last = s.to_owned();
        }
    }

    /// Replaces the history (oldest first).
    pub fn set_history(&mut self, h: Vec<String>) {
        self.history = h;
        self.hist_pos = None;
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Soft-wrapped rows for a box `width` columns wide. The first row starts with
    /// `prompt` (in `style`), the others with as many spaces. Returns the rows and
    /// the cursor (row, display column) within them.
    pub fn render(&self, width: usize, prompt: &str, style: Style) -> (Vec<Line>, (usize, usize)) {
        let prompt = sanitize(prompt);
        let pw = str_width(&prompt);
        let indent = " ".repeat(pw);
        let cw = width.saturating_sub(pw).max(1);
        let mut rows: Vec<Line> = Vec::new();
        let mut cursor = (0, pw);
        for (li, line) in self.lines.iter().enumerate() {
            let gs: Vec<(&str, usize)> = line.graphemes(true).map(|g| (g, g.width())).collect();
            let ranges = wrap_ranges(&gs, cw);
            for (k, &(s, e)) in ranges.iter().enumerate() {
                let text: String = gs[s..e].iter().map(|g| g.0).collect();
                let mut row = if rows.is_empty() {
                    Line::styled(&prompt, style)
                } else {
                    Line::raw(&indent)
                };
                row.push(text, Style::new());
                rows.push(row);
                let last = k + 1 == ranges.len();
                if li == self.row && (self.col < e || (last && self.col == e)) && self.col >= s {
                    let col: usize = gs[s..self.col].iter().map(|g| g.1).sum();
                    if col >= cw {
                        // Cursor after a full last row: give it a row of its own.
                        rows.push(Line::raw(&indent));
                        cursor = (rows.len() - 1, pw);
                    } else {
                        cursor = (rows.len() - 1, pw + col);
                    }
                }
            }
        }
        (rows, cursor)
    }

    /// The word before the cursor on its line (back to whitespace) and the grapheme
    /// column where it starts, for `/cmd` and `@path` completion.
    pub fn current_word(&self) -> (usize, String) {
        let line = &self.lines[self.row];
        let gs: Vec<&str> = line.graphemes(true).take(self.col).collect();
        let start = gs.iter().rposition(|g| is_ws(g)).map_or(0, |i| i + 1);
        (start, gs[start..].concat())
    }

    /// Replaces the word before the cursor with `with`, leaving the cursor after it.
    pub fn replace_current_word(&mut self, with: &str) {
        let (start, _) = self.current_word();
        let with = sanitize(with);
        self.edit(|e| {
            e.delete_range(start, e.col);
            e.insert_clean(&with);
        });
    }

    // Internal edits. Each returns whether something changed.

    /// Runs a text edit; editing a recalled history entry stops browsing.
    fn edit(&mut self, f: impl FnOnce(&mut Self)) -> bool {
        let before = (self.lines.clone(), self.row, self.col);
        f(self);
        let changed = before != (self.lines.clone(), self.row, self.col);
        if changed {
            self.hist_pos = None;
        }
        changed
    }

    fn load(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n");
        self.lines = s
            .split('\n')
            .map(|l| sanitize(&l.replace('\t', &" ".repeat(TAB_WIDTH))))
            .collect();
        self.row = self.lines.len() - 1;
        self.col = self.line_len();
    }

    fn line_len(&self) -> usize {
        grapheme_count(&self.lines[self.row])
    }

    fn grapheme_before(&self) -> Option<&str> {
        let col = self.col.checked_sub(1)?;
        self.lines[self.row].graphemes(true).nth(col)
    }

    fn move_to(&mut self, row: usize, col: usize) -> bool {
        let changed = (row, col) != (self.row, self.col);
        self.row = row;
        self.col = col;
        changed
    }

    /// Inserts text that has no newlines or control characters.
    fn insert_clean(&mut self, s: &str) {
        let line = &mut self.lines[self.row];
        let at = byte_at(line, self.col);
        line.insert_str(at, s);
        // Count graphemes up to the end of the insert, so a combining mark that
        // joins the previous grapheme does not push the cursor too far.
        self.col = grapheme_count(&line[..at + s.len()]);
    }

    fn insert_newline(&mut self) {
        let line = &mut self.lines[self.row];
        let at = byte_at(line, self.col);
        let rest = line.split_off(at);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
    }

    /// Deletes graphemes `from..to` of the current line.
    fn delete_range(&mut self, from: usize, to: usize) {
        let line = &mut self.lines[self.row];
        let (a, b) = (byte_at(line, from), byte_at(line, to));
        line.replace_range(a..b, "");
        self.col = from;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            self.delete_range(self.col - 1, self.col);
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len();
            self.lines[self.row].push_str(&line);
        }
    }

    fn delete_forward(&mut self) {
        if self.col < self.line_len() {
            self.delete_range(self.col, self.col + 1);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    /// Start of the word before `col`: skip whitespace, then non-whitespace.
    fn word_start_before(&self) -> usize {
        let gs: Vec<&str> = self.lines[self.row].graphemes(true).collect();
        let mut i = self.col;
        while i > 0 && is_ws(gs[i - 1]) {
            i -= 1;
        }
        while i > 0 && !is_ws(gs[i - 1]) {
            i -= 1;
        }
        i
    }

    fn delete_word_back(&mut self) {
        if self.col == 0 {
            self.backspace();
        } else {
            let start = self.word_start_before();
            self.delete_range(start, self.col);
        }
    }

    fn word_left(&mut self) -> bool {
        if self.col == 0 {
            return self.left();
        }
        let start = self.word_start_before();
        self.move_to(self.row, start)
    }

    fn word_right(&mut self) -> bool {
        let len = self.line_len();
        if self.col == len {
            return self.right();
        }
        let gs: Vec<&str> = self.lines[self.row].graphemes(true).collect();
        let mut i = self.col;
        while i < len && is_ws(gs[i]) {
            i += 1;
        }
        while i < len && !is_ws(gs[i]) {
            i += 1;
        }
        self.move_to(self.row, i)
    }

    fn left(&mut self) -> bool {
        if self.col > 0 {
            self.move_to(self.row, self.col - 1)
        } else if self.row > 0 {
            let len = grapheme_count(&self.lines[self.row - 1]);
            self.move_to(self.row - 1, len)
        } else {
            false
        }
    }

    fn right(&mut self) -> bool {
        if self.col < self.line_len() {
            self.move_to(self.row, self.col + 1)
        } else if self.row + 1 < self.lines.len() {
            self.move_to(self.row + 1, 0)
        } else {
            false
        }
    }

    /// Grapheme index in `row` closest to (not past) display column `x`.
    fn col_for_x(&self, row: usize, x: usize) -> usize {
        let mut w = 0;
        for (i, g) in self.lines[row].graphemes(true).enumerate() {
            w += g.width();
            if w > x {
                return i;
            }
        }
        grapheme_count(&self.lines[row])
    }

    fn cursor_x(&self) -> usize {
        self.lines[self.row]
            .graphemes(true)
            .take(self.col)
            .map(UnicodeWidthStr::width)
            .sum()
    }

    fn up(&mut self) -> bool {
        if self.row > 0 {
            let col = self.col_for_x(self.row - 1, self.cursor_x());
            return self.move_to(self.row - 1, col);
        }
        let pos = match self.hist_pos {
            None if self.history.is_empty() => return false,
            None => {
                self.draft = self.text();
                self.history.len() - 1
            }
            Some(0) => return false,
            Some(p) => p - 1,
        };
        self.hist_pos = Some(pos);
        let entry = self.history[pos].clone();
        self.load(&entry);
        true
    }

    fn down(&mut self) -> bool {
        if self.row + 1 < self.lines.len() {
            let col = self.col_for_x(self.row + 1, self.cursor_x());
            return self.move_to(self.row + 1, col);
        }
        let Some(pos) = self.hist_pos else {
            return false;
        };
        if pos + 1 < self.history.len() {
            self.hist_pos = Some(pos + 1);
            let entry = self.history[pos + 1].clone();
            self.load(&entry);
        } else {
            self.hist_pos = None;
            let draft = std::mem::take(&mut self.draft);
            self.load(&draft);
        }
        true
    }
}

/// Splits graphemes (with widths) into rows of at most `width` columns, breaking
/// after the last space when possible. Returns grapheme ranges; at least one.
fn wrap_ranges(gs: &[(&str, usize)], width: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut start, mut w, mut last_space) = (0, 0, None);
    let mut i = 0;
    while i < gs.len() {
        let (g, gw) = gs[i];
        if w + gw > width && i > start {
            let brk = match last_space {
                _ if is_ws(g) => i,
                Some(s) if s >= start => s + 1,
                _ => i,
            };
            out.push((start, brk));
            start = brk;
            w = gs[start..i].iter().map(|x| x.1).sum();
            last_space = None;
            continue;
        }
        if is_ws(g) {
            last_space = Some(i);
        }
        w += gw;
        i += 1;
    }
    out.push((start, gs.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    fn typed(s: &str) -> Editor {
        let mut e = Editor::new();
        for c in s.chars() {
            e.handle(key(KeyCode::Char(c)));
        }
        e
    }

    #[test]
    fn typing_and_submit() {
        let mut e = typed("hello");
        assert_eq!(e.text(), "hello");
        assert_eq!(e.cursor(), (0, 5));
        assert_eq!(
            e.handle(key(KeyCode::Enter)),
            EditorEvent::Submit("hello".into())
        );
        assert!(e.is_empty());
        assert_eq!(e.history(), ["hello"]);
    }

    #[test]
    fn shifted_chars_insert() {
        let mut e = Editor::new();
        e.handle(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        assert_eq!(e.text(), "A");
    }

    #[test]
    fn release_events_ignored() {
        let mut e = Editor::new();
        let mut k = key(KeyCode::Char('x'));
        k.kind = KeyEventKind::Release;
        assert_eq!(e.handle(k), EditorEvent::None);
        assert!(e.is_empty());
    }

    #[test]
    fn newline_keys() {
        let mut e = typed("a");
        e.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        e.handle(key(KeyCode::Char('b')));
        e.handle(ctrl('j'));
        e.handle(key(KeyCode::Char('c')));
        assert_eq!(e.text(), "a\nb\nc");
        assert_eq!(e.cursor(), (2, 1));
        assert_eq!(e.handle(alt(KeyCode::Enter)), EditorEvent::None);
        assert_eq!(e.text(), "a\nb\nc");
    }

    #[test]
    fn backslash_enter_inserts_newline() {
        let mut e = typed("one\\");
        assert_eq!(e.handle(key(KeyCode::Enter)), EditorEvent::Changed);
        assert_eq!(e.text(), "one\n");
        e.insert_str("two");
        assert_eq!(
            e.handle(key(KeyCode::Enter)),
            EditorEvent::Submit("one\ntwo".into())
        );
    }

    #[test]
    fn backspace_and_delete_join_lines() {
        let mut e = Editor::new();
        e.insert_str("ab\ncd");
        e.handle(key(KeyCode::Home));
        e.handle(key(KeyCode::Backspace));
        assert_eq!(e.text(), "abcd");
        assert_eq!(e.cursor(), (0, 2));
        e.handle(key(KeyCode::Backspace));
        assert_eq!(e.text(), "acd");
        e.handle(key(KeyCode::Delete));
        assert_eq!(e.text(), "ad");
        e.set_text("x\ny");
        e.handle(key(KeyCode::Up));
        e.handle(key(KeyCode::End));
        e.handle(key(KeyCode::Delete));
        assert_eq!(e.text(), "xy");
        assert_eq!(e.handle(key(KeyCode::End)), EditorEvent::Changed);
        assert_eq!(e.handle(key(KeyCode::Delete)), EditorEvent::None);
    }

    #[test]
    fn left_right_cross_lines() {
        let mut e = Editor::new();
        e.insert_str("ab\ncd");
        e.handle(key(KeyCode::Home));
        e.handle(key(KeyCode::Left));
        assert_eq!(e.cursor(), (0, 2));
        e.handle(key(KeyCode::Right));
        assert_eq!(e.cursor(), (1, 0));
        e.handle(key(KeyCode::End));
        assert_eq!(e.handle(key(KeyCode::Right)), EditorEvent::None);
    }

    #[test]
    fn up_down_between_lines_keep_column() {
        let mut e = Editor::new();
        e.insert_str("abcdef\nxy\n123456");
        assert_eq!(e.cursor(), (2, 6));
        e.handle(key(KeyCode::Up));
        assert_eq!(e.cursor(), (1, 2));
        e.handle(key(KeyCode::Up));
        assert_eq!(e.cursor(), (0, 2));
        e.handle(key(KeyCode::Down));
        e.handle(key(KeyCode::Down));
        assert_eq!(e.cursor(), (2, 2));
    }

    #[test]
    fn up_down_by_display_column_with_wide_chars() {
        let mut e = Editor::new();
        e.insert_str("你好世界\nabcd");
        e.handle(key(KeyCode::Left));
        // Display column 3 on "abcd" lands on the second wide char (columns 2-3).
        e.handle(key(KeyCode::Up));
        assert_eq!(e.cursor(), (0, 1));
    }

    #[test]
    fn home_end_and_ctrl_a_e() {
        let mut e = typed("hello");
        e.handle(key(KeyCode::Home));
        assert_eq!(e.cursor(), (0, 0));
        e.handle(key(KeyCode::End));
        assert_eq!(e.cursor(), (0, 5));
        e.handle(ctrl('a'));
        assert_eq!(e.cursor(), (0, 0));
        e.handle(ctrl('e'));
        assert_eq!(e.cursor(), (0, 5));
    }

    #[test]
    fn word_moves() {
        let mut e = typed("foo bar  baz");
        e.handle(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL));
        assert_eq!(e.cursor(), (0, 9));
        e.handle(alt(KeyCode::Char('b')));
        assert_eq!(e.cursor(), (0, 4));
        e.handle(alt(KeyCode::Char('b')));
        assert_eq!(e.cursor(), (0, 0));
        e.handle(alt(KeyCode::Char('f')));
        assert_eq!(e.cursor(), (0, 3));
        e.handle(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(e.cursor(), (0, 7));
        e.handle(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(e.cursor(), (0, 12));
    }

    #[test]
    fn word_deletes() {
        let mut e = typed("git commit  -m");
        e.handle(ctrl('w'));
        assert_eq!(e.text(), "git commit  ");
        e.handle(ctrl('w'));
        assert_eq!(e.text(), "git ");
        e.handle(alt(KeyCode::Backspace));
        assert_eq!(e.text(), "");
        let mut e = Editor::new();
        e.insert_str("a\nb");
        e.handle(key(KeyCode::Home));
        e.handle(ctrl('w'));
        assert_eq!(e.text(), "ab");
    }

    #[test]
    fn kill_to_start_and_end() {
        let mut e = typed("hello world");
        for _ in 0..5 {
            e.handle(key(KeyCode::Left));
        }
        e.handle(ctrl('k'));
        assert_eq!(e.text(), "hello ");
        assert_eq!(e.handle(ctrl('k')), EditorEvent::None);
        e.handle(key(KeyCode::Left));
        e.handle(ctrl('u'));
        assert_eq!(e.text(), " ");
        assert_eq!(e.cursor(), (0, 0));
        assert_eq!(e.handle(ctrl('u')), EditorEvent::None);
    }

    #[test]
    fn unbound_keys_return_none() {
        let mut e = typed("x");
        assert_eq!(e.handle(key(KeyCode::Tab)), EditorEvent::None);
        assert_eq!(e.handle(key(KeyCode::Esc)), EditorEvent::None);
        assert_eq!(e.handle(ctrl('z')), EditorEvent::None);
        assert_eq!(e.text(), "x");
    }

    #[test]
    fn history_browsing_restores_draft() {
        let mut e = Editor::new();
        e.set_history(vec!["first".into(), "second".into()]);
        e.insert_str("draft");
        assert_eq!(e.handle(key(KeyCode::Up)), EditorEvent::Changed);
        assert_eq!(e.text(), "second");
        e.handle(key(KeyCode::Up));
        assert_eq!(e.text(), "first");
        assert_eq!(e.handle(key(KeyCode::Up)), EditorEvent::None);
        assert_eq!(e.text(), "first");
        e.handle(key(KeyCode::Down));
        assert_eq!(e.text(), "second");
        e.handle(key(KeyCode::Down));
        assert_eq!(e.text(), "draft");
        assert_eq!(e.handle(key(KeyCode::Down)), EditorEvent::None);
    }

    #[test]
    fn history_multiline_entry_moves_within_lines() {
        let mut e = Editor::new();
        e.set_history(vec!["old".into(), "l1\nl2".into()]);
        e.handle(key(KeyCode::Up));
        assert_eq!(e.cursor(), (1, 2));
        e.handle(key(KeyCode::Up));
        assert_eq!((e.text().as_str(), e.cursor()), ("l1\nl2", (0, 2)));
        e.handle(key(KeyCode::Up));
        assert_eq!(e.text(), "old");
    }

    #[test]
    fn push_history_skips_blank_and_repeats() {
        let mut e = Editor::new();
        e.push_history("a");
        e.push_history("a");
        e.push_history("  ");
        e.push_history("b");
        e.push_history("a");
        assert_eq!(e.history(), ["a", "b", "a"]);
        assert_eq!(
            e.handle(key(KeyCode::Enter)),
            EditorEvent::Submit(String::new())
        );
        assert_eq!(e.history().len(), 3);
    }

    #[test]
    fn paste_normalizes() {
        let mut e = typed("x");
        e.insert_str("a\r\nb\rc\td\x1b[31m");
        assert_eq!(e.text(), "xa\nb\nc    d");
        assert_eq!(e.cursor(), (2, 6));
    }

    #[test]
    fn wide_char_cursor_columns() {
        let e = typed("a你🎉");
        assert_eq!(e.cursor(), (0, 3));
        let (rows, cur) = e.render(20, "> ", Style::new());
        assert_eq!(rows[0].plain(), "> a你🎉");
        assert_eq!(cur, (0, 2 + 5));
    }

    #[test]
    fn combining_mark_is_one_grapheme() {
        let mut e = typed("e\u{301}x");
        assert_eq!(e.cursor(), (0, 2));
        e.handle(key(KeyCode::Left));
        e.handle(key(KeyCode::Backspace));
        assert_eq!(e.text(), "x");
    }

    #[test]
    fn render_wraps_with_prompt_indent() {
        let mut e = Editor::new();
        e.insert_str("hello there world\nnext");
        let (rows, cur) = e.render(10, "> ", Style::new().bold());
        let plain: Vec<String> = rows.iter().map(Line::plain).collect();
        assert_eq!(plain, ["> hello ", "  there ", "  world", "  next"]);
        assert!(rows[0].spans[0].style.bold);
        assert_eq!(cur, (3, 6));
        assert!(rows.iter().all(|r| r.width() <= 10));
    }

    #[test]
    fn render_cursor_positions() {
        let mut e = Editor::new();
        e.insert_str("abcdefghij");
        // Content width 8: "abcdefgh" / "ij".
        let (rows, cur) = e.render(10, "> ", Style::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(cur, (1, 4));
        for _ in 0..2 {
            e.handle(key(KeyCode::Left));
        }
        assert_eq!(e.render(10, "> ", Style::new()).1, (1, 2));
        e.handle(key(KeyCode::Left));
        assert_eq!(e.render(10, "> ", Style::new()).1, (0, 9));
        // A full last row puts the cursor on a new row.
        let e = typed("abcdefgh");
        let (rows, cur) = e.render(10, "> ", Style::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(cur, (1, 2));
        // Empty editor.
        let (rows, cur) = Editor::new().render(10, "> ", Style::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(cur, (0, 2));
    }

    #[test]
    fn render_wide_chars_wrap() {
        let e = typed("你好世界");
        let (rows, cur) = e.render(7, "> ", Style::new());
        let plain: Vec<String> = rows.iter().map(Line::plain).collect();
        assert_eq!(plain, ["> 你好", "  世界"]);
        assert_eq!(cur, (1, 6));
    }

    #[test]
    fn current_word_and_replace() {
        let mut e = typed("look at @src/ma");
        assert_eq!(e.current_word(), (8, "@src/ma".into()));
        e.replace_current_word("@src/main.rs");
        assert_eq!(e.text(), "look at @src/main.rs");
        assert_eq!(e.cursor(), (0, 20));
        let mut e = typed("/mo");
        e.replace_current_word("/model ");
        assert_eq!(e.text(), "/model ");
        let e = typed("a ");
        assert_eq!(e.current_word(), (2, String::new()));
    }
}
