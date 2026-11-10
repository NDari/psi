//! The plan file and the task report (design-going-forward.md, "The
//! shape"). A plan is a markdown file with a checklist. The harness gives
//! the model the plan and one open item at a time, each in a fresh context,
//! writes a report of what the task did from its own records, and ticks
//! the item when the task ended by itself and the check passed.

use serde::{Deserialize, Serialize};

/// The session entry (`custom_type`) that starts a task. The context of
/// every later request starts after the last one: nothing before it is
/// sent, so nothing is carried from task to task in the model's words.
pub const TASK_START: &str = "task_start";
/// The session entry that holds a task's [`TaskReport`].
pub const TASK_REPORT: &str = "task_report";

/// The plan's text in a task prompt, at most (both ends are kept).
const MAX_PLAN_CHARS: usize = 12_000;
/// The files a report line names before "and N more".
const SHOWN_FILES: usize = 8;

/// One checklist line of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The line's index in the file.
    pub line: usize,
    pub text: String,
    pub done: bool,
}

/// A checklist line: a list marker, `[ ]` or `[x]`, and the text.
fn parse_line(line: &str) -> Option<(bool, &str)> {
    let rest = line.trim_start();
    let rest = match rest.as_bytes().first()? {
        b'-' | b'*' | b'+' => &rest[1..],
        b if b.is_ascii_digit() => {
            let digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            digits.strip_prefix(['.', ')'])?
        }
        _ => return None,
    };
    let rest = rest.strip_prefix(' ')?.trim_start();
    let (done, rest) = match rest.strip_prefix("[ ]") {
        Some(r) => (false, r),
        None => {
            let r = rest
                .strip_prefix("[x]")
                .or_else(|| rest.strip_prefix("[X]"))?;
            (true, r)
        }
    };
    let text = rest.trim();
    (!text.is_empty()).then_some((done, text))
}

/// The plan's checklist items, in order. Lines inside a fenced code block
/// are not items.
pub fn items(plan: &str) -> Vec<Item> {
    let mut fenced = false;
    let mut out = Vec::new();
    for (i, line) in plan.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some((done, text)) = parse_line(line) {
            out.push(Item {
                line: i,
                text: text.to_owned(),
                done,
            });
        }
    }
    out
}

/// The plan with the first open item whose text is `text` ticked, or
/// `None` when there is no such line (the model rewrote or removed it).
pub fn tick(plan: &str, text: &str) -> Option<String> {
    let at = items(plan)
        .into_iter()
        .find(|i| !i.done && i.text == text)?
        .line;
    let mut out: Vec<String> = plan.lines().map(str::to_owned).collect();
    out[at] = out[at].replacen("[ ]", "[x]", 1);
    let mut s = out.join("\n");
    if plan.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

/// The plan with every ticked item whose text is `text` open again: a task
/// that did not end well leaves its item open, whatever the model ticked.
pub fn untick(plan: &str, text: &str) -> String {
    let lines: Vec<usize> = items(plan)
        .into_iter()
        .filter(|i| i.done && i.text == text)
        .map(|i| i.line)
        .collect();
    let mut out: Vec<String> = plan.lines().map(str::to_owned).collect();
    for at in lines {
        out[at] = out[at].replacen("[x]", "[ ]", 1).replacen("[X]", "[ ]", 1);
    }
    let mut s = out.join("\n");
    if plan.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// How the project's check went at the end of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub command: String,
    pub passed: bool,
    /// The first lines of a failed check's output.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
}

/// What one task did, from the harness's own records: the checkpoints,
/// the check it ran, and how the run ended. Nothing here is the model's
/// account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReport {
    pub plan: String,
    pub item: String,
    /// The item's place among the plan's items, from 1, and how many there are.
    pub n: usize,
    pub total: usize,
    /// How the run ended: `done`, `budget`, `guard`, `aborted` or `failed`.
    pub stop: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Files that differ between the task's start and end, as `M path`.
    pub changed: Vec<String>,
    /// The project's branch tips or HEAD moved (the model committed).
    #[serde(default)]
    pub refs_moved: bool,
    /// `None` when no check is set, or the task did not end by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckResult>,
    pub requests: u32,
    pub tool_calls: u32,
    /// Calls the policy refused or the boundary blocked.
    pub refused: u32,
    pub secs: u64,
    /// The item was ticked in the plan.
    pub ticked: bool,
    /// Why it was left open, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_because: Option<String>,
    /// The checkpoints at the task's start and end, for undo per task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

fn files(changed: &[String]) -> String {
    if changed.is_empty() {
        return "nothing".into();
    }
    let mut s = changed
        .iter()
        .take(SHOWN_FILES)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if changed.len() > SHOWN_FILES {
        s.push_str(&format!(" and {} more", changed.len() - SHOWN_FILES));
    }
    s
}

fn duration(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

impl TaskReport {
    /// The report for the screen: one screen says what changed, which
    /// check ran and how the run ended.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!("task {} of {}: {}", self.n, self.total, self.item)];
        out.push(match (self.stop.as_str(), &self.reason) {
            ("done", _) => "  ended: by itself".into(),
            ("aborted", _) => "  ended: stopped by you".into(),
            ("failed", Some(r)) => format!("  ended: failed: {r}"),
            (_, Some(r)) => format!("  ended: stopped: {r}"),
            (s, None) => format!("  ended: {s}"),
        });
        let n = self.changed.len();
        out.push(match n {
            0 => "  changed: nothing".into(),
            1 => format!("  changed: 1 file ({})", files(&self.changed)),
            _ => format!("  changed: {n} files ({})", files(&self.changed)),
        });
        if self.refs_moved {
            out.push("  git: the branch tips or HEAD moved".into());
        }
        match &self.check {
            Some(c) if c.passed => out.push(format!("  check: `{}` passed", c.command)),
            Some(c) => {
                out.push(format!("  check: `{}` failed", c.command));
                out.extend(c.output.lines().map(|l| format!("    {l}")));
            }
            None if self.stop == "done" => out.push("  check: none is set".into()),
            None => out.push("  check: not run".into()),
        }
        let mut counts = format!(
            "  {} requests, {} tool calls",
            self.requests, self.tool_calls
        );
        if self.refused > 0 {
            counts.push_str(&format!(", {} refused", self.refused));
        }
        counts.push_str(&format!(", {}", duration(self.secs)));
        out.push(counts);
        out.push(match &self.open_because {
            None => format!("  ticked in {}", self.plan),
            Some(why) => format!("  left open: {why}"),
        });
        out
    }

    /// One line for the tasks that come after: what changed and whether
    /// the check passed.
    pub fn record_line(&self) -> String {
        let check = match &self.check {
            Some(c) if c.passed => format!("check `{}` passed", c.command),
            Some(c) => format!("check `{}` failed", c.command),
            None => "no check".into(),
        };
        format!("- {}: changed {}; {check}", self.item, files(&self.changed))
    }
}

/// Keeps both ends of `s`, about `max` characters in all.
fn cut_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max / 2).collect();
    let tail: String = s.chars().skip(n - max / 2).collect();
    format!("{head}\n[... {} characters cut ...]\n{tail}", n - max)
}

/// What a task's fresh context starts with: the plan, the harness's record
/// of the items done before, and the item.
pub fn task_prompt(plan_path: &str, plan: &str, item: &str, before: &[TaskReport]) -> String {
    let record = if before.is_empty() {
        "None in this session.".to_owned()
    } else {
        before
            .iter()
            .map(TaskReport::record_line)
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "[Task from psi] Carry out one item of the plan below, and only that item.\n\n\
         The plan ({plan_path}):\n{}\n\n\
         Items done before this one, as psi recorded them:\n{record}\n\n\
         Your item: {item}\n\n\
         Do this item and stop: don't start the next one, and don't tick the plan's checkboxes (psi ticks an item when its work ended and the check passed). End with a short note of what you did.",
        cut_middle(plan.trim_end(), MAX_PLAN_CHARS)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Plan\n\nSome words.\n\n- [x] Add the parser\n- [ ] Wire it into main\n  * [ ] nested: add a flag\n1. [ ] Write the docs\n\n```\n- [ ] not an item\n```\n- not a checkbox\n- [ ]\n";

    #[test]
    fn items_are_the_checklist_lines() {
        let all = items(PLAN);
        let got: Vec<(usize, &str, bool)> = all
            .iter()
            .map(|i| (i.line, i.text.as_str(), i.done))
            .collect();
        assert_eq!(
            got,
            [
                (4, "Add the parser", true),
                (5, "Wire it into main", false),
                (6, "nested: add a flag", false),
                (7, "Write the docs", false),
            ]
        );
        assert!(items("no list here\n").is_empty());
    }

    #[test]
    fn ticking_changes_one_line_and_keeps_the_rest() {
        let t = tick(PLAN, "Wire it into main").unwrap();
        assert_eq!(
            t,
            PLAN.replace("- [ ] Wire it into main", "- [x] Wire it into main")
        );
        let t = tick(&t, "nested: add a flag").unwrap();
        assert!(t.contains("  * [x] nested: add a flag\n"));
        // Done already, or not there: nothing to tick.
        assert_eq!(tick(PLAN, "Add the parser"), None);
        assert_eq!(tick(PLAN, "Something else"), None);
        // A task that failed leaves its item open, whatever the model ticked.
        assert_eq!(
            untick(&t, "nested: add a flag"),
            tick(PLAN, "Wire it into main").unwrap()
        );
        assert_eq!(untick(PLAN, "Write the docs"), PLAN);
        // No newline at the end stays that way.
        assert_eq!(tick("- [ ] a", "a").unwrap(), "- [x] a");
    }

    fn report() -> TaskReport {
        TaskReport {
            plan: "plan.md".into(),
            item: "Wire it into main".into(),
            n: 2,
            total: 4,
            stop: "done".into(),
            reason: None,
            changed: vec!["M src/main.rs".into(), "A src/wire.rs".into()],
            refs_moved: true,
            check: Some(CheckResult {
                command: "cargo test -q".into(),
                passed: true,
                output: String::new(),
            }),
            requests: 12,
            tool_calls: 19,
            refused: 1,
            secs: 252,
            ticked: true,
            open_because: None,
            from: Some("a".into()),
            to: Some("b".into()),
        }
    }

    #[test]
    fn the_report_says_what_changed_which_check_ran_and_how_it_ended() {
        assert_eq!(
            report().lines(),
            [
                "task 2 of 4: Wire it into main",
                "  ended: by itself",
                "  changed: 2 files (M src/main.rs, A src/wire.rs)",
                "  git: the branch tips or HEAD moved",
                "  check: `cargo test -q` passed",
                "  12 requests, 19 tool calls, 1 refused, 4m12s",
                "  ticked in plan.md",
            ]
        );
        let failed = TaskReport {
            stop: "budget".into(),
            reason: Some("reached the limit of 40 model requests in one run".into()),
            changed: vec![],
            refs_moved: false,
            check: None,
            refused: 0,
            secs: 9,
            ticked: false,
            open_because: Some("the task did not end by itself".into()),
            ..report()
        };
        assert_eq!(
            failed.lines()[1..],
            [
                "  ended: stopped: reached the limit of 40 model requests in one run",
                "  changed: nothing",
                "  check: not run",
                "  12 requests, 19 tool calls, 9s",
                "  left open: the task did not end by itself",
            ]
        );
        let red = TaskReport {
            check: Some(CheckResult {
                command: "make".into(),
                passed: false,
                output: "error: x\nat y".into(),
            }),
            ..report()
        };
        assert_eq!(
            red.lines()[4..7],
            ["  check: `make` failed", "    error: x", "    at y"]
        );
        // It round-trips through the session.
        let v = serde_json::to_value(report()).unwrap();
        assert_eq!(serde_json::from_value::<TaskReport>(v).unwrap(), report());
    }

    #[test]
    fn a_task_prompt_has_the_plan_the_record_and_the_item() {
        let p = task_prompt("plan.md", PLAN, "Write the docs", &[report()]);
        assert!(p.starts_with(
            "[Task from psi] Carry out one item of the plan below, and only that item.\n\nThe plan (plan.md):\n# Plan\n"
        ));
        assert!(p.contains(
            "\n\nItems done before this one, as psi recorded them:\n- Wire it into main: changed M src/main.rs, A src/wire.rs; check `cargo test -q` passed\n\nYour item: Write the docs\n\n"
        ));
        assert!(p.ends_with("End with a short note of what you did."));
        let first = task_prompt("plan.md", PLAN, "Add the parser", &[]);
        assert!(first.contains("as psi recorded them:\nNone in this session.\n"));
        // A long plan keeps both ends.
        let long = format!("{PLAN}{}", "filler line\n".repeat(3000));
        let p = task_prompt("plan.md", &long, "x", &[]);
        assert!(p.contains("characters cut") && p.len() < 14_000);
    }
}
