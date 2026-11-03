//! Pastes into the input: invisible characters are removed, and a long
//! paste goes into the box as a placeholder (`[Pasted text #1 +120 lines]`)
//! that is swapped back for the text when the prompt is sent.

/// A paste longer than this many lines or characters becomes a placeholder.
const MAX_LINES: usize = 10;
const MAX_CHARS: usize = 1000;

#[derive(Default)]
pub struct Pastes {
    n: usize,
    /// (placeholder, text), kept for the session: a stashed or recalled
    /// draft may still hold a placeholder.
    items: Vec<(String, String)>,
}

impl Pastes {
    /// What goes into the box for a paste, and how many invisible
    /// characters were removed from it.
    pub fn take(&mut self, pasted: &str) -> (String, usize) {
        let text = pasted.replace("\r\n", "\n").replace('\r', "\n");
        let (text, removed) = strip_invisible(&text);
        let lines = text.lines().count();
        if lines <= MAX_LINES && text.chars().count() <= MAX_CHARS {
            return (text, removed);
        }
        self.n += 1;
        let what = if lines > 1 {
            format!("+{lines} lines")
        } else {
            format!("{} characters", text.chars().count())
        };
        let placeholder = format!("[Pasted text #{} {what}]", self.n);
        self.items.push((placeholder.clone(), text));
        (placeholder, removed)
    }

    /// `text` with each placeholder replaced by its paste.
    pub fn expand(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (placeholder, pasted) in &self.items {
            if out.contains(placeholder.as_str()) {
                out = out.replace(placeholder.as_str(), pasted);
            }
        }
        out
    }
}

/// Removes characters that are invisible on screen but read by a model:
/// Unicode tag characters (which can spell out hidden instructions) and
/// bidirectional controls (which reorder what is shown).
pub fn strip_invisible(s: &str) -> (String, usize) {
    let mut removed = 0;
    let out = s
        .chars()
        .filter(|&c| {
            let hidden = matches!(c,
                '\u{E0000}'..='\u{E007F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{200E}' | '\u{200F}' | '\u{061C}');
            removed += usize::from(hidden);
            !hidden
        })
        .collect();
    (out, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_pastes_go_in_as_they_are() {
        let mut p = Pastes::default();
        assert_eq!(p.take("a\r\nb"), ("a\nb".to_owned(), 0));
    }

    #[test]
    fn long_pastes_become_placeholders_that_expand() {
        let mut p = Pastes::default();
        let long: String = (0..120).map(|i| format!("line {i}\n")).collect();
        let (ph, _) = p.take(&long);
        assert_eq!(ph, "[Pasted text #1 +120 lines]");
        let (ph2, _) = p.take(&"x".repeat(1500));
        assert_eq!(ph2, "[Pasted text #2 1500 characters]");
        let sent = p.expand(&format!("fix this:\n{ph}\nthanks"));
        assert_eq!(sent, format!("fix this:\n{long}\nthanks"));
        assert_eq!(p.expand("no placeholder"), "no placeholder");
    }

    #[test]
    fn invisible_characters_are_counted_and_removed() {
        // "hi" followed by tag characters spelling "ok", and a bidi override.
        let s = "hi\u{E006F}\u{E006B} \u{202E}abc\u{202C}";
        assert_eq!(strip_invisible(s), ("hi abc".to_owned(), 4));
        // Joiners in emoji stay.
        assert_eq!(strip_invisible("👩\u{200D}💻").1, 0);
    }
}
