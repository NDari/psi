//! The look: a named color for each part of the screen. It uses the
//! terminal's 16 colors, so the terminal's own theme decides the shades
//! (a gruvbox dark terminal gives gruvbox colors).

use crate::style::{Color, Style};

/// What the screen colors.
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

/// The style for a token: the 16 colors, and dim for muted text.
pub fn style(t: Token) -> Style {
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

#[cfg(test)]
mod tests {
    use super::*;

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
