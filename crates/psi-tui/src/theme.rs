//! Themes (psi-plan.md §11.4, pi parity): named colors for each part of the
//! screen. The default uses the terminal's 16 colors, so the terminal's own
//! theme decides the shades (§11.2). A theme file is pi's format: `name`,
//! optional `vars`, and `colors` from token to a value: `""` (the terminal's
//! default), a 256-color index, `#rgb` or `#rrggbb`, an ANSI color name, or
//! a var. Tokens Psi doesn't draw are ignored, so pi themes load.

use std::sync::{LazyLock, RwLock};

use serde_json::Value;

use crate::style::{Color, Style};

/// What the screen colors, by pi's token names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    /// Marks, selections, command names.
    Accent,
    Success,
    Error,
    Warning,
    /// Secondary text: hints, summaries, the status line.
    Muted,
    /// The `›` before the user's prompts.
    UserMessageText,
    /// The `!` before user bash commands.
    BashMode,
    /// The line above the input box.
    BorderMuted,
    /// Headings, and lines that are all bold (titles).
    MdHeading,
    /// A link's text; its URL after it is `MdLinkUrl`.
    MdLink,
    MdLinkUrl,
    /// Inline `code`.
    MdCode,
    /// Fenced code without a known language tag.
    MdCodeBlock,
    MdCodeBlockBorder,
    /// A quote's text and the bar before it.
    MdQuote,
    MdQuoteBorder,
    /// A horizontal rule.
    MdHr,
    /// List markers (`-`, `1.`).
    MdListBullet,
    ToolDiffAdded,
    ToolDiffRemoved,
    /// Code in fenced blocks (§11.5): pi's nine syntax colors.
    SyntaxComment,
    SyntaxKeyword,
    SyntaxFunction,
    SyntaxVariable,
    SyntaxString,
    SyntaxNumber,
    SyntaxType,
    SyntaxOperator,
    SyntaxPunctuation,
}

const TOKENS: &[(Token, &str)] = &[
    (Token::Accent, "accent"),
    (Token::Success, "success"),
    (Token::Error, "error"),
    (Token::Warning, "warning"),
    (Token::Muted, "muted"),
    (Token::UserMessageText, "userMessageText"),
    (Token::BashMode, "bashMode"),
    (Token::BorderMuted, "borderMuted"),
    (Token::MdHeading, "mdHeading"),
    (Token::MdLink, "mdLink"),
    (Token::MdLinkUrl, "mdLinkUrl"),
    (Token::MdCode, "mdCode"),
    (Token::MdCodeBlock, "mdCodeBlock"),
    (Token::MdCodeBlockBorder, "mdCodeBlockBorder"),
    (Token::MdQuote, "mdQuote"),
    (Token::MdQuoteBorder, "mdQuoteBorder"),
    (Token::MdHr, "mdHr"),
    (Token::MdListBullet, "mdListBullet"),
    (Token::ToolDiffAdded, "toolDiffAdded"),
    (Token::ToolDiffRemoved, "toolDiffRemoved"),
    (Token::SyntaxComment, "syntaxComment"),
    (Token::SyntaxKeyword, "syntaxKeyword"),
    (Token::SyntaxFunction, "syntaxFunction"),
    (Token::SyntaxVariable, "syntaxVariable"),
    (Token::SyntaxString, "syntaxString"),
    (Token::SyntaxNumber, "syntaxNumber"),
    (Token::SyntaxType, "syntaxType"),
    (Token::SyntaxOperator, "syntaxOperator"),
    (Token::SyntaxPunctuation, "syntaxPunctuation"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    /// `None`: the default look for the token.
    colors: Vec<(Token, Option<Color>)>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "default".into(),
            colors: Vec::new(),
        }
    }
}

/// The look without a theme: the 16 colors, and dim for muted text.
fn default_style(t: Token) -> Style {
    match t {
        Token::Accent => Style::fg(Color::Cyan),
        Token::Success | Token::ToolDiffAdded => Style::fg(Color::Green),
        Token::Error | Token::ToolDiffRemoved => Style::fg(Color::Red),
        Token::Warning => Style::fg(Color::Yellow),
        Token::Muted
        | Token::BorderMuted
        | Token::MdCodeBlockBorder
        | Token::MdLinkUrl
        | Token::MdQuote
        | Token::MdQuoteBorder
        | Token::MdHr => Style::new().dim(),
        Token::UserMessageText => Style::fg(Color::Blue),
        Token::BashMode => Style::fg(Color::Magenta),
        // Close to pi's dark theme in the 16 colors; inline code stays cyan.
        Token::MdHeading => Style::fg(Color::Yellow),
        Token::MdLink => Style::fg(Color::Blue),
        Token::MdCode => Style::fg(Color::Cyan),
        Token::MdCodeBlock => Style::fg(Color::Green),
        Token::MdListBullet => Style::fg(Color::Magenta),
        // Code: a few of the 16 colors; operators, punctuation and names
        // stay the terminal's own, so code isn't a wall of color.
        Token::SyntaxComment => Style::new().dim(),
        Token::SyntaxKeyword => Style::fg(Color::Magenta),
        Token::SyntaxFunction => Style::fg(Color::Blue),
        Token::SyntaxString => Style::fg(Color::Green),
        Token::SyntaxNumber => Style::fg(Color::Yellow),
        Token::SyntaxType => Style::fg(Color::Cyan),
        Token::SyntaxVariable | Token::SyntaxOperator | Token::SyntaxPunctuation => Style::new(),
    }
}

impl Theme {
    pub fn style(&self, t: Token) -> Style {
        match self.colors.iter().find(|(k, _)| *k == t) {
            Some((_, Some(c))) => Style::fg(*c),
            // "" is the terminal's own color.
            Some((_, None)) => Style::new(),
            None => default_style(t),
        }
    }

    /// Reads a theme file's JSON. Returns the problems found too; a value
    /// that can't be read keeps that token's default.
    pub fn from_json(v: &Value) -> Result<(Theme, Vec<String>), String> {
        let name = v["name"]
            .as_str()
            .filter(|n| !n.is_empty() && !n.contains('/'))
            .ok_or("a theme needs a \"name\" without slashes")?
            .to_owned();
        let colors = v["colors"]
            .as_object()
            .ok_or("a theme needs a \"colors\" object")?;
        let vars = v["vars"].as_object();
        let mut problems = Vec::new();
        let mut out = Vec::new();
        for (token, key) in TOKENS {
            let Some(val) = colors.get(*key) else {
                continue;
            };
            match resolve(val, vars, 0) {
                Ok(c) => out.push((*token, c)),
                Err(e) => problems.push(format!("theme {name}: {key}: {e}")),
            }
        }
        Ok((Theme { name, colors: out }, problems))
    }
}

fn resolve(
    v: &Value,
    vars: Option<&serde_json::Map<String, Value>>,
    depth: usize,
) -> Result<Option<Color>, String> {
    if depth > 8 {
        return Err("vars refer to each other in a loop".into());
    }
    match v {
        Value::Number(n) => match n.as_u64() {
            Some(i @ 0..=255) => Ok(Some(Color::Indexed(i as u8))),
            _ => Err(format!("{n} is not a color index from 0 to 255")),
        },
        Value::String(s) if s.is_empty() => Ok(None),
        Value::String(s) if s.starts_with('#') => hex(s).map(Some),
        Value::String(s) => {
            if let Some(c) = ansi(s) {
                return Ok(Some(c));
            }
            if let Some(var) = vars.and_then(|m| m.get(s)) {
                return resolve(var, vars, depth + 1);
            }
            if s.contains('(') {
                return Err(format!(
                    "{s}: only #hex, 0-255, ANSI names and \"\" are supported"
                ));
            }
            Err(format!("no var named {s}"))
        }
        other => Err(format!("{other} is not a color")),
    }
}

fn hex(s: &str) -> Result<Color, String> {
    let h = &s[1..];
    let digits: Option<Vec<u8>> = h.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect();
    match (h.len(), digits) {
        (3, Some(d)) => Ok(Color::Rgb(d[0] * 17, d[1] * 17, d[2] * 17)),
        (6, Some(d)) => Ok(Color::Rgb(
            d[0] * 16 + d[1],
            d[2] * 16 + d[3],
            d[4] * 16 + d[5],
        )),
        _ => Err(format!("{s} is not #rgb or #rrggbb")),
    }
}

/// The 16 colors by name, pi-style camelCase or with a hyphen.
fn ansi(s: &str) -> Option<Color> {
    let n = s.to_ascii_lowercase().replace(['-', '_'], "");
    Some(match n.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "brightblack" | "gray" | "grey" => Color::BrightBlack,
        "brightred" => Color::BrightRed,
        "brightgreen" => Color::BrightGreen,
        "brightyellow" => Color::BrightYellow,
        "brightblue" => Color::BrightBlue,
        "brightmagenta" => Color::BrightMagenta,
        "brightcyan" => Color::BrightCyan,
        "brightwhite" => Color::BrightWhite,
        _ => return None,
    })
}

static CURRENT: LazyLock<RwLock<Theme>> = LazyLock::new(|| RwLock::new(Theme::default()));

/// The style for a token in the current theme.
pub fn style(t: Token) -> Style {
    CURRENT
        .read()
        .map(|th| th.style(t))
        .unwrap_or_else(|_| default_style(t))
}

/// Switches the theme for everything drawn from now on.
pub fn set(theme: Theme) {
    if let Ok(mut c) = CURRENT.write() {
        *c = theme;
    }
}

pub fn current_name() -> String {
    CURRENT
        .read()
        .map(|t| t.name.clone())
        .unwrap_or_else(|_| "default".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_vars_and_unknown_tokens() {
        let (t, problems) = Theme::from_json(&json!({
            "name": "mine",
            "vars": {"pink": "#ff88cc", "alias": "pink"},
            "colors": {
                "accent": "alias",
                "error": 196,
                "success": "brightGreen",
                "muted": "",
                "warning": "okhsl(83 88% 67%)",
                "mdHeading": "#abc",
                "syntaxKeyword": "#123456",
            }
        }))
        .unwrap();
        assert_eq!(t.style(Token::Accent), Style::fg(Color::Rgb(255, 136, 204)));
        assert_eq!(t.style(Token::Error), Style::fg(Color::Indexed(196)));
        assert_eq!(t.style(Token::Success), Style::fg(Color::BrightGreen));
        assert_eq!(
            t.style(Token::Muted),
            Style::new(),
            "\"\" is the terminal's color"
        );
        assert_eq!(
            t.style(Token::MdHeading),
            Style::fg(Color::Rgb(170, 187, 204))
        );
        // Unreadable values keep the default, and say why.
        assert_eq!(t.style(Token::Warning), Style::fg(Color::Yellow));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("warning: okhsl"), "{problems:?}");
        assert_eq!(t.style(Token::BashMode), Style::fg(Color::Magenta));
        // pi's syntax colors apply to code.
        assert_eq!(
            t.style(Token::SyntaxKeyword),
            Style::fg(Color::Rgb(0x12, 0x34, 0x56))
        );
        assert_eq!(t.style(Token::SyntaxString), Style::fg(Color::Green));
    }

    #[test]
    fn bad_files() {
        assert!(Theme::from_json(&json!({"colors": {}})).is_err());
        assert!(Theme::from_json(&json!({"name": "a/b", "colors": {}})).is_err());
        assert!(Theme::from_json(&json!({"name": "x"})).is_err());
        let (_, p) = Theme::from_json(
            &json!({"name": "x", "vars": {"a": "b", "b": "a"}, "colors": {"accent": "a"}}),
        )
        .unwrap();
        assert!(p[0].contains("loop"), "{p:?}");
    }

    #[test]
    fn sgr_for_theme_colors() {
        let mut out = Vec::new();
        let mut l = crate::Line::default();
        l.push("x", Style::fg(Color::Rgb(1, 2, 3)));
        l.push("y", Style::fg(Color::Indexed(200)).bold());
        l.write_ansi(&mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\x1b[0;38;2;1;2;3mx\x1b[0;1;38;5;200my\x1b[0m"
        );
    }
}
