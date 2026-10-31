//! What the interactive mode draws (psi-plan.md §11.2): tool calls
//! committed once as a header, a summary, a few output lines and a short
//! diff; the status line; the approval card; dropdowns.

use psi_tui::{Color, Line, Style};
use serde_json::Value;

/// Output lines a committed tool call shows (`/show <n>` has the rest).
const OUTPUT_LINES: usize = 4;
/// Diff lines a committed edit shows.
const DIFF_LINES: usize = 20;

pub fn dim() -> Style {
    Style::default().dim()
}

fn first_line(s: &str, max: usize) -> String {
    let l = s.lines().next().unwrap_or("");
    cut(l, max)
}

pub fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// The one-line argument summary for a call.
pub fn call_summary(name: &str, args: &Value) -> String {
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    match name {
        "bash" => s("command").replace('\n', " "),
        "read" => {
            let mut out = s("path").to_owned();
            if let Some(o) = args.get("offset").and_then(Value::as_i64) {
                out.push_str(&format!(" from line {o}"));
            }
            out
        }
        "write" | "edit" => s("path").to_owned(),
        "grep" => format!("{} {}", s("pattern"), s("path")).trim().to_owned(),
        "find" | "ls" => format!("{} {}", s("pattern"), s("path")).trim().to_owned(),
        _ => serde_json::to_string(args).unwrap_or_default(),
    }
}

/// The header shown while a call runs, and when it is committed.
pub fn call_header(n: usize, name: &str, args: &Value, width: usize, running: Option<f64>) -> Line {
    let mut l = Line::default();
    let mark = match running {
        Some(_) => "◌ ",
        None => "● ",
    };
    l.push(mark, Style::fg(Color::Cyan));
    l.push(name, Style::default().bold());
    l.push("  ", Style::default());
    let summary = call_summary(name, args);
    let room = width.saturating_sub(name.len() + 14);
    l.push(cut(&summary, room), Style::default());
    if let Some(secs) = running {
        l.push(format!("  {secs:.1}s"), dim());
    } else {
        l.push(format!("  #{n}"), dim());
    }
    l
}

/// A finished call: header, summary, up to 4 output lines, and for edits a
/// short diff.
pub fn call_block(
    n: usize,
    name: &str,
    args: &Value,
    result: &str,
    is_error: bool,
    width: usize,
) -> Vec<Line> {
    let mut out = vec![call_header(n, name, args, width, None)];
    let lines: Vec<&str> = result.lines().collect();
    let tail = if is_error {
        Style::fg(Color::Red)
    } else {
        dim()
    };
    let summary = match name {
        "read" if !is_error => format!("{} lines", lines.len()),
        "bash" => {
            let last = lines.last().copied().unwrap_or("");
            if last.starts_with("[exit code")
                || last.starts_with("[timed out")
                || last.starts_with("[killed")
                || last == "[aborted]"
            {
                last.trim_matches(['[', ']']).to_owned()
            } else {
                format!("{} lines", lines.len())
            }
        }
        _ => first_line(result, width.saturating_sub(6)),
    };
    let mut l = Line::default();
    l.push("  └ ", dim());
    l.push(&summary, tail);
    out.push(l);
    let shown: Vec<&str> = match name {
        "bash" => lines
            .iter()
            .copied()
            .filter(|l| !l.starts_with("[exit code"))
            .take(OUTPUT_LINES)
            .collect(),
        "write" | "edit" | "read" if !is_error => Vec::new(),
        _ => lines.iter().copied().skip(1).take(OUTPUT_LINES).collect(),
    };
    for s in &shown {
        out.push(Line::styled(
            format!("    {}", cut(s, width.saturating_sub(4))),
            dim(),
        ));
    }
    let hidden = lines.len().saturating_sub(shown.len() + 1);
    if name == "bash" && hidden > 0 && lines.len() > OUTPUT_LINES {
        out.push(Line::styled(
            format!("    … {hidden} more lines (/show {n})"),
            dim(),
        ));
    }
    if name == "edit" && !is_error {
        let old = args.get("old_text").and_then(Value::as_str).unwrap_or("");
        let new = args.get("new_text").and_then(Value::as_str).unwrap_or("");
        out.extend(diff_lines(old, new, DIFF_LINES, width));
    }
    out
}

/// A colored unified-style diff of two texts, cut to `max` lines.
pub fn diff_lines(old: &str, new: &str, max: usize, width: usize) -> Vec<Line> {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    let mut more = 0;
    for change in diff.iter_all_changes() {
        let (sign, style) = match change.tag() {
            similar::ChangeTag::Delete => ("-", Style::fg(Color::Red)),
            similar::ChangeTag::Insert => ("+", Style::fg(Color::Green)),
            similar::ChangeTag::Equal => continue,
        };
        if out.len() >= max {
            more += 1;
            continue;
        }
        let text = change.value().trim_end_matches('\n');
        out.push(Line::styled(
            format!("    {sign} {}", cut(text, width.saturating_sub(6))),
            style,
        ));
    }
    if more > 0 {
        out.push(Line::styled(
            format!("    … {more} more changed lines"),
            dim(),
        ));
    }
    out
}

/// Diff text from git, colored by line.
pub fn git_diff_lines(diff: &str) -> Vec<Line> {
    diff.lines()
        .map(|l| {
            let style = if l.starts_with("+++")
                || l.starts_with("---")
                || l.starts_with("diff ")
                || l.starts_with("index ")
            {
                Style::default().bold()
            } else if l.starts_with('+') {
                Style::fg(Color::Green)
            } else if l.starts_with('-') {
                Style::fg(Color::Red)
            } else if l.starts_with("@@") {
                Style::fg(Color::Cyan)
            } else {
                Style::default()
            };
            Line::styled(l, style)
        })
        .collect()
}

/// The facts the status line shows (§10).
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub model: String,
    pub n_ctx: u64,
    pub prompt_tokens: u64,
    pub tok_s: Option<f64>,
    pub ttft_ms: Option<u64>,
    pub mode: String,
    pub network: String,
    pub checkpoints: usize,
    pub unisolated: bool,
    pub busy: Option<String>,
}

pub fn status_line(s: &Status, width: usize) -> Line {
    let mut l = Line::default();
    if let Some(b) = &s.busy {
        l.push(b, Style::fg(Color::Yellow));
        l.push("  ", Style::default());
    }
    let mut parts = vec![s.model.clone()];
    if let Some(pct) = (s.prompt_tokens * 100).checked_div(s.n_ctx) {
        parts.push(format!("ctx {pct}%"));
    }
    if let Some(t) = s.tok_s {
        parts.push(format!("{t:.0} tok/s"));
    }
    if let Some(t) = s.ttft_ms {
        parts.push(format!("ttft {t}ms"));
    }
    parts.push(s.mode.clone());
    if s.network == "off" {
        parts.push("net off".into());
    }
    parts.push(format!("{} checkpoints", s.checkpoints));
    l.push(parts.join(" · "), dim());
    if s.unisolated {
        l.push("  UNISOLATED", Style::fg(Color::Red).bold());
    }
    l.truncate(width)
}

/// The approval card under the input box (§8.7).
pub fn approval_card(
    tool: &str,
    args: &Value,
    intent: Option<&str>,
    preceding: &str,
    reason: &str,
    typing: Option<&str>,
    width: usize,
) -> Vec<Line> {
    let w = width.saturating_sub(4);
    let border = Style::fg(Color::Yellow);
    let mut out = Vec::new();
    let mut top = Line::default();
    top.push("┌ ", border);
    top.push(format!("approve {tool}"), Style::fg(Color::Yellow).bold());
    out.push(top);
    let body = |out: &mut Vec<Line>, label: &str, text: &str, style: Style| {
        for (i, t) in text.lines().take(4).enumerate() {
            let mut l = Line::default();
            l.push("│ ", border);
            if i == 0 && !label.is_empty() {
                l.push(label, dim());
            }
            l.push(cut(t, w.saturating_sub(label.len())), style);
            out.push(l);
        }
    };
    body(
        &mut out,
        "",
        &call_summary(tool, args),
        Style::default().bold(),
    );
    match intent {
        Some(i) => body(&mut out, "intent: ", i, Style::default()),
        None if !preceding.trim().is_empty() => {
            let last = preceding.trim().lines().last().unwrap_or("");
            body(&mut out, "model: ", last, dim());
        }
        None => {}
    }
    body(&mut out, "why: ", reason, Style::default());
    let mut keys = Line::default();
    keys.push("└ ", border);
    match typing {
        Some(t) => {
            keys.push("message to the model: ", dim());
            keys.push(t, Style::default());
        }
        None => {
            keys.push(
                "y once · s this session · n deny · e deny with a message",
                dim(),
            );
        }
    }
    out.push(keys);
    out
}

/// A list under the input box: completions, or a picker.
pub fn list(items: &[String], sel: usize, max: usize, width: usize) -> Vec<Line> {
    let start = sel.saturating_sub(max.saturating_sub(1));
    items
        .iter()
        .enumerate()
        .skip(start)
        .take(max)
        .map(|(i, it)| {
            let text = cut(it, width.saturating_sub(4));
            if i == sel {
                Line::styled(format!("  ▸ {text}"), Style::fg(Color::Cyan).bold())
            } else {
                Line::styled(format!("    {text}"), dim())
            }
        })
        .collect()
}
