//! What the interactive mode draws (psi-plan.md §11.2): tool calls
//! committed once as a header, a summary, a few output lines and a short
//! diff; the status line; the approval card; dropdowns.

use psi_agent::approval::ApprovalRequest;
use psi_tui::theme::{self, Token};
use psi_tui::{Line, Style};
use serde_json::Value;
use std::time::Duration;

/// Output lines a committed tool call shows (`/show <n>` has the rest).
const OUTPUT_LINES: usize = 4;
/// Diff lines a committed edit shows.
const DIFF_LINES: usize = 20;

pub fn dim() -> Style {
    theme::style(Token::Muted)
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
        "agent" => format!("{}: {}", s("agent"), s("task").replace('\n', " ")),
        "grep" => format!("{} {}", s("pattern"), s("path")).trim().to_owned(),
        "find" | "ls" => format!("{} {}", s("pattern"), s("path")).trim().to_owned(),
        "todo" => {
            let id = args.get("id").map(|v| {
                format!(
                    "#{}",
                    v.as_str().map(str::to_owned).unwrap_or(v.to_string())
                )
            });
            format!("{} {}{}", s("action"), s("text"), id.unwrap_or_default())
                .trim()
                .to_owned()
        }
        _ => serde_json::to_string(args).unwrap_or_default(),
    }
}

/// Where a call is: running for so many seconds, or finished.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallState {
    Running(f64),
    Done,
    Failed,
}

/// The header shown while a call runs, and when it is committed. The mark
/// says how it went: dim while running, green when done, red when it failed.
pub fn call_header(n: usize, name: &str, args: &Value, width: usize, state: CallState) -> Line {
    let mut l = Line::default();
    let (mark, style) = match state {
        CallState::Running(_) => ("◌ ", dim()),
        CallState::Done => ("● ", theme::style(Token::Success)),
        CallState::Failed => ("● ", theme::style(Token::Error)),
    };
    l.push(mark, style);
    l.push(name, Style::default().bold());
    l.push("  ", Style::default());
    let summary = call_summary(name, args);
    let room = width.saturating_sub(name.len() + 14);
    // A file the call touched is a link (OSC 8) the terminal can open.
    let file = matches!(name, "read" | "write" | "edit")
        .then(|| args.get("path").and_then(Value::as_str))
        .flatten()
        .and_then(|p| psi_tui::style::file_url(std::path::Path::new(p)));
    match file {
        Some(url) => l.push_link(cut(&summary, room), Style::default(), &url),
        None => l.push(cut(&summary, room), Style::default()),
    };
    if let CallState::Running(secs) = state {
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
    let state = if is_error {
        CallState::Failed
    } else {
        CallState::Done
    };
    let mut out = vec![call_header(n, name, args, width, state)];
    let lines: Vec<&str> = result.lines().collect();
    let tail = if is_error {
        theme::style(Token::Error)
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
        let (old, new) = edit_texts(args);
        out.extend(diff_lines(&old, &new, DIFF_LINES, width));
    }
    out
}

/// An edit's old and new text; several edits one after the other, so their
/// diff shows each change in order.
fn edit_texts(args: &Value) -> (String, String) {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let Some(list) = args.get("edits").and_then(Value::as_array) else {
        return (text(args, "old_text"), text(args, "new_text"));
    };
    let joined = |k: &str| {
        list.iter()
            .map(|e| format!("{}\n", text(e, k).trim_end_matches('\n')))
            .collect::<String>()
    };
    (joined("old_text"), joined("new_text"))
}

/// A colored unified-style diff of two texts, cut to `max` lines. Within a
/// changed line, the words that changed are shown in reverse video, unless
/// most of the line changed (then the color alone says it).
pub fn diff_lines(old: &str, new: &str, max: usize, width: usize) -> Vec<Line> {
    let diff = similar::TextDiff::from_lines(old, new);
    // Word diffs run on the UI thread; a pathological edit gives up early.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    let mut out = Vec::new();
    let mut more = 0;
    for op in diff.ops() {
        for change in diff.iter_inline_changes_deadline(op, Some(deadline)) {
            let (sign, style) = match change.tag() {
                similar::ChangeTag::Delete => ("-", theme::style(Token::ToolDiffRemoved)),
                similar::ChangeTag::Insert => ("+", theme::style(Token::ToolDiffAdded)),
                similar::ChangeTag::Equal => continue,
            };
            if out.len() >= max {
                more += 1;
                continue;
            }
            let parts: Vec<(bool, String)> = change
                .iter_strings_lossy()
                .map(|(emph, s)| (emph, s.trim_end_matches('\n').to_owned()))
                .collect();
            let total: usize = parts.iter().map(|(_, s)| s.chars().count()).sum();
            let changed: usize = parts
                .iter()
                .filter(|(e, _)| *e)
                .map(|(_, s)| s.chars().count())
                .sum();
            let words = changed * 10 <= total * 6;
            let mut line = Line::styled(format!("    {sign} "), style);
            for (emph, s) in &parts {
                let st = if *emph && words {
                    style.reverse()
                } else {
                    style
                };
                line.push(s, st);
            }
            out.push(line.truncate(width));
        }
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
                theme::style(Token::ToolDiffAdded)
            } else if l.starts_with('-') {
                theme::style(Token::ToolDiffRemoved)
            } else if l.starts_with("@@") {
                theme::style(Token::Accent)
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
    /// Prompt tokens read per second, last response.
    pub prompt_tok_s: Option<f64>,
    /// The share of the last prompt reused from the server's cache.
    pub cache_pct: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub mode: String,
    pub network: String,
    pub checkpoints: usize,
    pub unisolated: bool,
    pub busy: Option<String>,
    /// "plan", or the steps done while a plan is carried out.
    pub plan: Option<String>,
    /// The model server's address while it isn't up.
    pub offline: Option<String>,
}

pub fn status_line(s: &Status, width: usize) -> Line {
    let mut l = Line::default();
    if let Some(b) = &s.busy {
        l.push(b, theme::style(Token::Warning));
        l.push("  ", Style::default());
    }
    if let Some(p) = &s.plan {
        l.push(p, theme::style(Token::Warning).bold());
        l.push("  ", Style::default());
    }
    l.push(&s.model, dim());
    if s.offline.is_some() {
        l.push(" · ", dim());
        l.push("no model server", theme::style(Token::Warning));
    } else if let Some(pct) = (s.prompt_tokens * 100).checked_div(s.n_ctx) {
        // Like pi-powerline-footer: a warning from 70%, an error from 90%.
        let style = match pct {
            90.. => theme::style(Token::Error),
            70.. => theme::style(Token::Warning),
            _ => dim(),
        };
        l.push(" · ", dim());
        l.push(format!("ctx {pct}% of {}", short_tokens(s.n_ctx)), style);
    }
    // Joined with " · ", after the model and context.
    let mut parts = vec![String::new()];
    if let Some(t) = s.tok_s {
        parts.push(format!("{t:.0} tok/s"));
    }
    if let Some(t) = s.prompt_tok_s {
        parts.push(format!("read {t:.0} tok/s"));
    }
    if let Some(c) = s.cache_pct {
        parts.push(format!("cache {c}%"));
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
        l.push("  UNISOLATED", theme::style(Token::Error).bold());
    }
    l.truncate(width)
}

/// 65536 reads "64K", 900 "900".
pub fn short_tokens(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..10_000 => format!("{:.1}K", n as f64 / 1024.0).replace(".0K", "K"),
        _ => format!("{}K", (n + 512) / 1024),
    }
}

/// What the model is writing that the screen doesn't show as it comes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Generating {
    /// Characters of reasoning so far.
    Thinking(u64),
    /// A tool call: its name, its path once known, characters of arguments.
    Call {
        name: String,
        path: Option<String>,
        chars: u64,
    },
}

impl Generating {
    fn text(&self) -> String {
        let size = |n: u64| format!(": {} characters", short_tokens(n));
        match self {
            Generating::Thinking(n) => format!("thinking{}", size(*n)),
            Generating::Call { name, path, chars } => {
                let mut s = format!("writing a {name} call");
                if let Some(p) = path {
                    s.push_str(&format!(" for {p}"));
                }
                if *chars > 0 {
                    s.push_str(&size(*chars));
                }
                s
            }
        }
    }
}

/// The spinner's text and timer: what the run waits for, and for how long.
/// A summary counts from its own start and says Esc stops it (a summary at
/// 100K tokens took 47 s in the cog trial, with nothing on screen moving).
/// While the model thinks or writes a tool call it says so, with a size
/// that grows: at a local model's speed either can take minutes.
pub fn spinner_text(
    progress: Option<(u64, u64, u64)>,
    compacting: Option<(Duration, u64)>,
    since_start: Duration,
    generating: Option<&Generating>,
) -> String {
    let reading = progress
        .map(prompt_progress)
        .or_else(|| generating.map(Generating::text));
    match compacting {
        Some((took, tokens)) => {
            let mut s = format!("summarizing about {} tokens", short_tokens(tokens));
            if let Some(r) = reading {
                s.push_str(&format!(" · {r}"));
            }
            format!("{s} ({:.0}s) · Esc stops", took.as_secs_f64())
        }
        None => format!(
            "{} ({:.0}s)",
            reading.unwrap_or_else(|| "waiting for the model…".into()),
            since_start.as_secs_f64()
        ),
    }
}

/// The spinner's text while the server reads the prompt: the share of
/// the new tokens done (cached ones take no time).
pub fn prompt_progress((total, cache, processed): (u64, u64, u64)) -> String {
    let new = total.saturating_sub(cache);
    let done = processed.saturating_sub(cache);
    let pct = (done * 100).checked_div(new).unwrap_or(100);
    let mut s = format!("reading the prompt: {pct}% of {} tokens", short_tokens(new));
    if cache > 0 {
        s.push_str(&format!(", {} from the cache", short_tokens(cache)));
    }
    s
}

/// The approval card's answers; "No" is the safe one.
pub const APPROVAL_CHOICES: [&str; 4] = [
    "Yes, once",
    "Yes, for this session",
    "No",
    "No, and tell the model why",
];
pub const APPROVAL_SAFE: usize = 2;

/// A card's answers as a list, the selected one marked, and the keys
/// (cards used to take single letters, which typing could answer).
pub fn choices(items: &[String], sel: usize, safe: usize, border: Style) -> Vec<Line> {
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let mut l = Line::default();
        l.push("│ ", border);
        if i == sel {
            l.push(format!("▸ {it}"), theme::style(Token::Accent).bold());
        } else {
            l.push(format!("  {it}"), dim());
        }
        out.push(l);
    }
    let esc = items
        .get(safe)
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let mut keys = Line::default();
    keys.push("└ ", border);
    keys.push(format!("↑↓ choose · Enter confirm · Esc {esc}"), dim());
    out.push(keys);
    out
}

/// The approval card under the input box (§8.7).
pub fn approval_card(
    req: &ApprovalRequest,
    typing: Option<&str>,
    sel: usize,
    width: usize,
) -> Vec<Line> {
    let (tool, args, reason) = (req.tool.as_str(), &req.args, req.reason.as_str());
    let (intent, preceding) = (req.intent.as_deref(), req.preceding_text.as_str());
    let w = width.saturating_sub(4);
    let border = theme::style(Token::Warning);
    let mut out = Vec::new();
    let mut top = Line::default();
    top.push("┌ ", border);
    let title = match &req.agent {
        Some(a) => format!("approve {tool} for {a}"),
        None => format!("approve {tool}"),
    };
    top.push(title, theme::style(Token::Warning).bold());
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
    match typing {
        Some(t) => {
            let mut keys = Line::default();
            keys.push("└ ", border);
            keys.push("message to the model: ", dim());
            keys.push(t, Style::default());
            out.push(keys);
        }
        None => {
            let items: Vec<String> = APPROVAL_CHOICES.iter().map(|s| (*s).to_owned()).collect();
            out.extend(choices(&items, sel, APPROVAL_SAFE, border));
        }
    }
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
                Line::styled(format!("  ▸ {text}"), theme::style(Token::Accent).bold())
            } else {
                Line::styled(format!("    {text}"), dim())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn the_status_line_shows_speeds_cache_and_a_colored_context() {
        let mut s = Status {
            model: "qwen".into(),
            n_ctx: 65536,
            prompt_tokens: 32768,
            tok_s: Some(41.6),
            prompt_tok_s: Some(812.0),
            cache_pct: Some(93),
            ttft_ms: Some(240),
            mode: "auto".into(),
            ..Status::default()
        };
        let l = status_line(&s, 200);
        assert_eq!(
            text(&l),
            "qwen · ctx 50% of 64K · 42 tok/s · read 812 tok/s · cache 93% · ttft 240ms · auto · 0 checkpoints"
        );
        let ctx = |s: &Status| {
            let l = status_line(s, 200);
            l.spans
                .iter()
                .find(|x| x.text.contains("ctx"))
                .map(|x| x.style)
                .unwrap()
        };
        assert_eq!(ctx(&s), dim());
        s.prompt_tokens = 48000;
        assert_eq!(ctx(&s), theme::style(Token::Warning));
        s.prompt_tokens = 60000;
        assert_eq!(ctx(&s), theme::style(Token::Error));
    }

    #[test]
    fn prompt_progress_counts_only_the_new_tokens() {
        assert_eq!(
            prompt_progress((30000, 20000, 25000)),
            "reading the prompt: 50% of 10K tokens, 20K from the cache"
        );
        assert_eq!(
            prompt_progress((800, 0, 200)),
            "reading the prompt: 25% of 800 tokens"
        );
        assert_eq!(short_tokens(65536), "64K");
        assert_eq!(short_tokens(1024), "1K");
    }

    #[test]
    fn the_spinner_says_when_it_is_summarizing() {
        let s = Duration::from_secs;
        assert_eq!(
            spinner_text(None, None, s(3), None),
            "waiting for the model… (3s)"
        );
        assert_eq!(
            spinner_text(Some((800, 0, 200)), None, s(3), None),
            "reading the prompt: 25% of 800 tokens (3s)"
        );
        // A summary has its own timer, and says how to stop it.
        assert_eq!(
            spinner_text(None, Some((s(12), 100_352)), s(90), None),
            "summarizing about 98K tokens (12s) · Esc stops"
        );
        assert_eq!(
            spinner_text(Some((6000, 0, 3000)), Some((s(4), 100_352)), s(90), None),
            "summarizing about 98K tokens · reading the prompt: 50% of 5.9K tokens (4s) · Esc stops"
        );
    }

    #[test]
    fn the_spinner_says_when_the_model_thinks_or_writes_a_call() {
        let s = Duration::from_secs;
        let say = |g: Generating| spinner_text(None, None, s(41), Some(&g));
        assert_eq!(
            say(Generating::Thinking(1300)),
            "thinking: 1.3K characters (41s)"
        );
        let call = |path: Option<&str>, chars| Generating::Call {
            name: "write".into(),
            path: path.map(str::to_owned),
            chars,
        };
        assert_eq!(say(call(None, 0)), "writing a write call (41s)");
        assert_eq!(
            say(call(Some("src/big.rs"), 0)),
            "writing a write call for src/big.rs (41s)"
        );
        assert_eq!(
            say(call(Some("src/big.rs"), 9000)),
            "writing a write call for src/big.rs: 8.8K characters (41s)"
        );
    }

    #[test]
    fn several_edits_show_each_change_in_order() {
        let args = serde_json::json!({"path": "a.py", "edits": [
            {"old_text": "a = 1", "new_text": "a = 2"},
            {"old_text": "c = 3\n", "new_text": "c = 4\n"},
        ]});
        let (old, new) = super::edit_texts(&args);
        assert_eq!(
            (old.as_str(), new.as_str()),
            ("a = 1\nc = 3\n", "a = 2\nc = 4\n")
        );
        let one = serde_json::json!({"old_text": "x", "new_text": "y"});
        assert_eq!(super::edit_texts(&one), ("x".into(), "y".into()));
    }

    /// The words shown in reverse video on each line.
    fn emphasized(lines: &[Line]) -> Vec<Vec<String>> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .filter(|s| s.style.reverse)
                    .map(|s| s.text.clone())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn changed_words_stand_out_in_a_changed_line() {
        let lines = diff_lines(
            "let total = price * 3;\nkeep\n",
            "let total = price * 4;\nkeep\n",
            20,
            80,
        );
        let plain: Vec<String> = lines.iter().map(Line::plain).collect();
        assert_eq!(
            plain,
            [
                "    - let total = price * 3;",
                "    + let total = price * 4;"
            ]
        );
        assert_eq!(emphasized(&lines), [vec!["3;"], vec!["4;"]]);
    }

    #[test]
    fn a_rewritten_line_and_new_lines_are_color_only() {
        let lines = diff_lines("alpha beta\n", "gamma delta\nextra line\n", 20, 80);
        assert_eq!(lines.len(), 3);
        assert!(emphasized(&lines).iter().all(Vec::is_empty));
    }

    #[test]
    fn long_diffs_are_cut_with_a_count() {
        let old: String = (0..30).map(|i| format!("a{i}\n")).collect();
        let lines = diff_lines(&old, "", 20, 80);
        assert_eq!(lines.len(), 21);
        assert_eq!(lines[20].plain(), "    … 10 more changed lines");
    }
}
