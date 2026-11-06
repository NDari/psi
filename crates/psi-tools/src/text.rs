//! Output hygiene shared by the tools.

use std::sync::LazyLock;

use regex::Regex;

/// Longest line a tool shows before cutting it.
pub(crate) const MAX_LINE_CHARS: usize = 2000;

#[allow(clippy::expect_used)] // A constant pattern; tests cover it.
static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[@-Z\\-_]")
        .expect("valid regex")
});

/// Removes terminal escapes and keeps only what a carriage return left
/// visible on each line (progress bars).
pub(crate) fn clean(s: &str) -> String {
    let s = ANSI.replace_all(s, "");
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        out.push_str(line.rsplit('\r').next().unwrap_or(line));
    }
    out
}

/// "1 line", "3 lines".
pub(crate) fn lines(n: usize) -> String {
    if n == 1 {
        "1 line".into()
    } else {
        format!("{n} lines")
    }
}

pub(crate) fn cut_line(line: &str) -> String {
    cut_at(line, MAX_LINE_CHARS)
}

fn cut_at(line: &str, limit: usize) -> String {
    if line.chars().count() <= limit {
        return line.to_owned();
    }
    let kept: String = line.chars().take(limit).collect();
    format!("{kept} [line cut at {limit} characters]")
}

/// Keeps the first quarter and the last three quarters of `max` characters,
/// on line boundaries, with a notice in between: errors tend to be at the end.
/// Long lines are cut first, so one long line (minified JSON) still shows.
pub(crate) fn head_tail(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let limit = MAX_LINE_CHARS.min(max / 2);
    let cut: Vec<String> = s.lines().map(|l| cut_at(l, limit)).collect();
    let joined = cut.join("\n");
    if joined.chars().count() <= max {
        return joined;
    }
    let lines: Vec<&str> = cut.iter().map(String::as_str).collect();
    let (head_budget, tail_budget) = (max / 4, max - max / 4);
    let mut head = Vec::new();
    let mut used = 0;
    for l in &lines {
        let n = l.chars().count() + 1;
        if used + n > head_budget {
            break;
        }
        used += n;
        head.push(*l);
    }
    let mut tail = Vec::new();
    used = 0;
    for l in lines[head.len()..].iter().rev() {
        let n = l.chars().count() + 1;
        if used + n > tail_budget {
            break;
        }
        used += n;
        tail.push(*l);
    }
    tail.reverse();
    let omitted = lines.len() - head.len() - tail.len();
    let mut out = head.join("\n");
    out.push_str(&format!("\n[... {omitted} lines omitted ...]\n"));
    out.push_str(&tail.join("\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_colors_and_progress() {
        assert_eq!(clean("\x1b[31merror\x1b[0m: x"), "error: x");
        assert_eq!(clean("10%\r50%\r100%\ndone\r\n"), "100%\ndone\n");
    }

    #[test]
    fn head_tail_keeps_both_ends() {
        let s: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        let out = head_tail(&s, 200);
        assert!(out.starts_with("line 1\n"));
        assert!(out.ends_with("line 100"));
        assert!(out.contains("lines omitted"));
        assert!(out.chars().count() < 260);
    }

    #[test]
    fn head_tail_keeps_one_long_line() {
        let out = head_tail(&"a".repeat(40_000), 30_000);
        assert!(out.starts_with("aaaa"));
        assert!(out.ends_with("[line cut at 2000 characters]"));
        // A budget smaller than a cut line: the end still shows.
        let s = format!("first {}\nlast {}", "b".repeat(500), "c".repeat(500));
        let out = head_tail(&s, 200);
        assert!(out.contains("last ccc"), "{out}");
    }

    #[test]
    fn long_lines_are_cut() {
        let l = "x".repeat(MAX_LINE_CHARS + 5);
        assert!(cut_line(&l).ends_with("[line cut at 2000 characters]"));
    }
}
