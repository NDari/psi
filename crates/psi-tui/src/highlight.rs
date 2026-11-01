//! Syntax highlighting for fenced code (psi-plan.md §11.5, D-43): bat's
//! grammars through `syntect`, with each token's scope mapped onto pi's
//! nine `syntax*` theme colors. Only color is added; the text is unchanged.

use std::sync::OnceLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::style::{Line, Style};
use crate::theme::{self, Token};

/// The grammars, loaded when the first code block needs them (about a
/// millisecond; each grammar compiles lazily).
fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// Tags the grammars don't know by these names.
const ALIASES: &[(&str, &str)] = &[
    ("jsonc", "json"),
    ("json5", "json"),
    ("zsh", "bash"),
    ("shell", "bash"),
    ("console", "bash"),
    ("py3", "python"),
    ("yml", "yaml"),
    ("golang", "go"),
];

fn syntax_for<'a>(set: &'a SyntaxSet, tag: &str) -> Option<&'a SyntaxReference> {
    let tag = tag.trim().to_ascii_lowercase();
    let tag = ALIASES
        .iter()
        .find(|(a, _)| *a == tag)
        .map_or(tag.as_str(), |(_, t)| *t);
    set.find_syntax_by_token(tag)
}

/// Scope prefixes and their colors, most specific first. Strings and
/// comments win over anything inside them (a string's quotes are
/// punctuation inside a string scope).
fn kinds() -> &'static [(Scope, Kind)] {
    static K: OnceLock<Vec<(Scope, Kind)>> = OnceLock::new();
    K.get_or_init(|| {
        [
            ("markup.inserted", Kind::Added),
            ("markup.deleted", Kind::Removed),
            ("meta.diff.range", Kind::Muted),
            ("meta.diff.header", Kind::Muted),
            (
                "constant.character.escape",
                Kind::Token(Token::SyntaxString),
            ),
            ("constant.numeric", Kind::Token(Token::SyntaxNumber)),
            ("constant.language", Kind::Token(Token::SyntaxKeyword)),
            ("keyword.operator", Kind::Token(Token::SyntaxOperator)),
            ("keyword", Kind::Token(Token::SyntaxKeyword)),
            ("storage", Kind::Token(Token::SyntaxKeyword)),
            ("entity.name.function", Kind::Token(Token::SyntaxFunction)),
            ("support.function", Kind::Token(Token::SyntaxFunction)),
            ("variable.function", Kind::Token(Token::SyntaxFunction)),
            ("entity.name.type", Kind::Token(Token::SyntaxType)),
            ("entity.name.class", Kind::Token(Token::SyntaxType)),
            ("entity.name.struct", Kind::Token(Token::SyntaxType)),
            ("entity.name.enum", Kind::Token(Token::SyntaxType)),
            ("entity.name.trait", Kind::Token(Token::SyntaxType)),
            ("support.type", Kind::Token(Token::SyntaxType)),
            ("support.class", Kind::Token(Token::SyntaxType)),
            ("punctuation", Kind::Token(Token::SyntaxPunctuation)),
            ("variable", Kind::Token(Token::SyntaxVariable)),
        ]
        .into_iter()
        .filter_map(|(s, k)| Scope::new(s).ok().map(|s| (s, k)))
        .collect()
    })
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Token(Token),
    Added,
    Removed,
    Muted,
}

fn scope(prefix: &str) -> Option<Scope> {
    Scope::new(prefix).ok()
}

/// The style for text under `stack`.
fn style_for(stack: &ScopeStack) -> Style {
    static OUTER: OnceLock<Vec<(Scope, Token)>> = OnceLock::new();
    let outer = OUTER.get_or_init(|| {
        [
            ("comment", Token::SyntaxComment),
            ("string", Token::SyntaxString),
        ]
        .into_iter()
        .filter_map(|(s, t)| scope(s).map(|s| (s, t)))
        .collect()
    });
    let scopes = stack.as_slice();
    for (prefix, t) in outer {
        if scopes.iter().any(|s| prefix.is_prefix_of(*s)) {
            return theme::style(*t);
        }
    }
    for s in scopes.iter().rev() {
        if let Some((_, k)) = kinds().iter().find(|(p, _)| p.is_prefix_of(*s)) {
            return match k {
                Kind::Token(t) => theme::style(*t),
                Kind::Added => theme::style(Token::ToolDiffAdded),
                Kind::Removed => theme::style(Token::ToolDiffRemoved),
                Kind::Muted => theme::style(Token::Muted),
            };
        }
    }
    Style::new()
}

/// `lines` of code highlighted as the language `tag` names; `None` when
/// the tag names no known language (the caller shows the code as is).
pub fn highlight(tag: &str, lines: &[&str]) -> Option<Vec<Line>> {
    let set = syntaxes();
    let syntax = syntax_for(set, tag)?;
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        // The grammars expect each line with its newline.
        let text = format!("{line}\n");
        let ops = state.parse_line(&text, set).ok()?;
        let mut l = Line::new();
        let mut at = 0;
        for (i, op) in ops {
            let end = i.min(line.len());
            if end > at {
                l.push(&line[at..end], style_for(&stack));
                at = end;
            }
            stack.apply(&op).ok()?;
        }
        if at < line.len() {
            l.push(&line[at..], style_for(&stack));
        }
        out.push(l);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    /// (text, style) runs of a highlighted line, merging nothing.
    fn runs(l: &Line) -> Vec<(String, Style)> {
        l.spans.iter().map(|s| (s.text.clone(), s.style)).collect()
    }

    fn style_of(l: &Line, text: &str) -> Style {
        runs(l)
            .into_iter()
            .find(|(t, _)| t.contains(text))
            .map(|(_, s)| s)
            .unwrap_or_else(|| panic!("no span with {text:?} in {:?}", runs(l)))
    }

    #[test]
    fn rust_gets_the_default_colors_and_keeps_its_text() {
        let code = [
            "// add one",
            "fn add(x: u32) -> u32 {",
            "    x + 1 // \"no\"",
            "}",
        ];
        let out = highlight("rust", &code).unwrap();
        let plain: Vec<String> = out.iter().map(Line::plain).collect();
        assert_eq!(plain, code);
        assert_eq!(style_of(&out[0], "add one"), Style::new().dim());
        assert_eq!(style_of(&out[1], "fn"), Style::fg(Color::Magenta));
        assert_eq!(style_of(&out[1], "add"), Style::fg(Color::Blue));
        assert_eq!(style_of(&out[2], "1"), Style::fg(Color::Yellow));
    }

    #[test]
    fn strings_keep_their_quotes_in_one_color() {
        let out = highlight("py", &["s = \"hi\""]).unwrap();
        assert_eq!(style_of(&out[0], "\"hi"), Style::fg(Color::Green));
    }

    #[test]
    fn diffs_use_the_diff_colors() {
        let out = highlight("diff", &["-old", "+new"]).unwrap();
        assert_eq!(style_of(&out[0], "old"), Style::fg(Color::Red));
        assert_eq!(style_of(&out[1], "new"), Style::fg(Color::Green));
    }

    #[test]
    fn tags_and_aliases() {
        for tag in ["toml", "ts", "Dockerfile", "jsonc", "zsh", "yml", "fish"] {
            assert!(highlight(tag, &["x"]).is_some(), "{tag}");
        }
        assert!(highlight("no-such-language", &["x"]).is_none());
    }
}
