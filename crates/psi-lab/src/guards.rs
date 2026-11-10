//! `psi guards`: today's guards run over recorded sessions
//! (design-going-forward.md, goal 4). The loop's own `Guards` are fed the
//! calls and results a recording holds, in the order the loop feeds them,
//! so a question like "would this rule have stopped that loop, and when"
//! is answered from recordings, without a copy of the rules in a script.

use std::path::{Path, PathBuf};

use psi_agent::guards::{GuardAction, Guards, call_signature};
use psi_session::{Entry, EntryBody, Role, StopReason, read_entries_lossy, text_of};
use serde::Serialize;

use crate::Result;
use crate::error::{IoContext, other};

/// Failed calls in a row before the error-streak nudge, as the loop's
/// default (`Budgets::max_consecutive_errors`).
const MAX_ERRORS: u32 = 10;
/// How the loop marks a call it held back: the guards don't see those.
const HELD_BACK: &str = "Not run: the results before this one take about";

/// One thing a guard would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    /// The model request it follows, counted from 1 over the session.
    pub step: usize,
    pub guard: String,
    /// `nudge` (a line for the model), `note` (the run goes on once with a
    /// note naming the repeated calls) or `stop`.
    pub what: &'static str,
    pub text: String,
}

/// One run (one prompt, or one task) of a session under today's guards.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct RunReplay {
    /// The session's steps this run covers: first and last.
    pub first_step: usize,
    pub last_step: usize,
    pub hits: Vec<Hit>,
    /// The step today's guards would stop the run after, if they would.
    pub stop: Option<usize>,
    /// The guard events the recording itself holds: (step, guard).
    pub recorded: Vec<(usize, String)>,
}

impl RunReplay {
    pub fn steps(&self) -> usize {
        (self.last_step + 1).saturating_sub(self.first_step)
    }

    /// Requests the recording made after the point today's guards stop it.
    pub fn saved(&self) -> usize {
        self.stop.map_or(0, |s| self.last_step.saturating_sub(s))
    }
}

struct Live {
    guards: Guards,
    run: RunReplay,
    /// The run went on from a no-progress stop once already.
    loop_noted: bool,
    /// The last model step answered without calls: the next user message
    /// starts a run.
    ended: bool,
}

impl Live {
    fn new(first_step: usize) -> Self {
        Self {
            guards: Guards::new(MAX_ERRORS),
            run: RunReplay {
                first_step,
                last_step: first_step.saturating_sub(1),
                ..RunReplay::default()
            },
            loop_noted: false,
            ended: false,
        }
    }

    fn act(&mut self, step: usize, action: GuardAction) {
        let (guard, what, text) = match action {
            GuardAction::Nudge { guard, text } => (guard, "nudge", text),
            GuardAction::Pause { guard, reason } => {
                // As the loop: a no-progress stop goes on once, with the note.
                let note = self
                    .guards
                    .take_loop_note()
                    .filter(|(r, _)| *r == reason && !self.loop_noted);
                match note {
                    Some(_) => {
                        self.loop_noted = true;
                        self.guards.reset();
                        (guard, "note", reason)
                    }
                    None => {
                        self.run.stop.get_or_insert(step);
                        (guard, "stop", reason)
                    }
                }
            }
        };
        self.run.hits.push(Hit {
            step,
            guard: guard.to_owned(),
            what,
            text,
        });
    }
}

/// Runs today's guards over a session's current branch. One result per
/// run that made a model request.
pub fn replay_guards(entries: &[Entry]) -> Vec<RunReplay> {
    let leaf = entries.last().map(|e| e.id.as_str());
    let branch = psi_session::branch(entries, leaf);
    // Sessions say whether a person was at the keyboard since step 4;
    // older ones are taken as unattended (eval trials, print runs).
    let attended = branch.iter().any(|e| {
        matches!(&e.body, EntryBody::Custom { custom_type, data }
            if custom_type == "session_info" && data["attended"] == true)
    });
    let mut out = Vec::new();
    let mut live: Option<Live> = None;
    let mut step = 0usize;
    let finish = |live: Option<Live>, out: &mut Vec<RunReplay>| {
        if let Some(l) = live
            && l.run.last_step >= l.run.first_step
        {
            out.push(l.run);
        }
    };
    for (i, e) in branch.iter().enumerate() {
        match &e.body {
            EntryBody::Checkpoint {
                trigger: Some(t), ..
            } if t == "submit" => {
                finish(live.take(), &mut out);
                live = Some(Live::new(step + 1));
            }
            EntryBody::Custom { custom_type, .. } if custom_type == "task_start" => {
                finish(live.take(), &mut out);
                live = Some(Live::new(step + 1));
            }
            // Without checkpoints: a user message after an answer.
            EntryBody::Message {
                role: Role::User, ..
            } if live.as_ref().is_none_or(|l| l.ended) => {
                finish(live.take(), &mut out);
                live = Some(Live::new(step + 1));
            }
            EntryBody::Compaction { summary, .. } if !summary.is_empty() => {
                if let Some(l) = &mut live {
                    l.guards.on_compaction();
                }
            }
            EntryBody::GuardEvent { guard, .. } => {
                if let Some(l) = &mut live {
                    l.run.recorded.push((step, guard.clone()));
                }
            }
            EntryBody::Message {
                role: Role::Assistant,
                content,
                stop_reason,
                ..
            } if e.is_model_step() => {
                step += 1;
                let l = live.get_or_insert_with(|| Live::new(step));
                l.run.last_step = step;
                // After the stop the recording is what did not happen.
                if l.run.stop.is_some() {
                    continue;
                }
                let calls: Vec<_> = e.tool_calls().collect();
                l.ended = calls.is_empty();
                if calls.is_empty() {
                    let nudge = if *stop_reason == Some(StopReason::Length) {
                        l.guards.on_truncated(0)
                    } else if !attended {
                        l.guards.on_answer(&text_of(content))
                    } else {
                        None
                    };
                    if let Some(a) = nudge {
                        l.act(step, a);
                    }
                    continue;
                }
                // This turn: up to the next model step.
                let turn_end = branch[i + 1..]
                    .iter()
                    .position(|n| n.is_model_step())
                    .map_or(branch.len(), |n| i + 1 + n);
                let turn = &branch[i + 1..turn_end];
                let result = |id: &str| {
                    turn.iter().find_map(|r| match &r.body {
                        EntryBody::ToolResult {
                            tool_call_id,
                            content,
                            is_error,
                            ..
                        } if tool_call_id == id => Some((*is_error, text_of(content))),
                        _ => None,
                    })
                };
                let mut wrote = false;
                let mut actions = Vec::new();
                for (id, name, args) in &calls {
                    let (failed, text) = result(id).unwrap_or((true, String::new()));
                    if text.starts_with(HELD_BACK) {
                        continue;
                    }
                    wrote |= !failed && matches!(*name, "write" | "edit");
                    let empty = serde_json::Map::new();
                    let sig = call_signature(name, args.as_object().unwrap_or(&empty));
                    actions.extend(l.guards.on_call(&sig, failed));
                }
                let ids: Vec<&str> = calls.iter().map(|(id, _, _)| *id).collect();
                let captured = turn.iter().any(|c| {
                    matches!(&c.body, EntryBody::Checkpoint { trigger: Some(t), tool_call_id: Some(id), .. }
                        if t == "tool" && ids.contains(&id.as_str()))
                });
                actions.extend(l.guards.on_turn_end(captured || wrote));
                for a in actions {
                    l.act(step, a);
                }
            }
            _ => {}
        }
    }
    finish(live, &mut out);
    out
}

/// A session file's runs under today's guards.
#[derive(Debug, Clone, Serialize)]
pub struct SessionReplay {
    pub path: PathBuf,
    pub runs: Vec<RunReplay>,
}

/// The session files at `path`: the file itself, an eval trial's
/// `session.jsonl`, or every session under a directory (wire logs and
/// event logs are not sessions).
pub fn session_files(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    if !path.is_dir() {
        return Err(other(format!(
            "{} is not a file or a directory",
            path.display()
        )));
    }
    let mut out = Vec::new();
    let mut dirs = vec![path.to_owned()];
    while let Some(d) = dirs.pop() {
        let entries = std::fs::read_dir(&d).ctx(|| format!("reading {}", d.display()))?;
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                // A trial's own home and work are the agent's, not records.
                if !matches!(name.as_str(), "home" | "work" | "grade") {
                    dirs.push(p);
                }
            } else if name.ends_with(".jsonl")
                && !name.ends_with("wire.jsonl")
                && name != "run.jsonl"
            {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Today's guards over every session at `paths`.
pub fn replay_paths(paths: &[PathBuf]) -> Result<Vec<SessionReplay>> {
    let mut out = Vec::new();
    for path in paths {
        for file in session_files(path)? {
            let f = std::fs::File::open(&file).ctx(|| format!("opening {}", file.display()))?;
            let (entries, _) = read_entries_lossy(std::io::BufReader::new(f))
                .map_err(|e| other(format!("{}: {e}", file.display())))?;
            out.push(SessionReplay {
                path: file,
                runs: replay_guards(&entries),
            });
        }
    }
    Ok(out)
}

/// The report: each run with what the guards would do, then the totals.
/// With `stops_only`, only runs today's guards would stop.
pub fn report(sessions: &[SessionReplay], stops_only: bool) -> String {
    let mut out = String::new();
    let (mut runs, mut stopped, mut saved) = (0, 0, 0);
    let mut by_guard: Vec<(String, usize)> = Vec::new();
    for s in sessions {
        let mut lines = Vec::new();
        for (n, r) in s.runs.iter().enumerate() {
            runs += 1;
            if let Some(at) = r.stop {
                stopped += 1;
                saved += r.saved();
                let guard = r
                    .hits
                    .iter()
                    .find(|h| h.what == "stop" && h.step == at)
                    .map_or("?", |h| h.guard.as_str());
                match by_guard.iter_mut().find(|(g, _)| g == guard) {
                    Some((_, c)) => *c += 1,
                    None => by_guard.push((guard.to_owned(), 1)),
                }
            }
            if r.hits.is_empty() || (stops_only && r.stop.is_none()) {
                continue;
            }
            // What the recording's own guards did then, by guard.
            let mut then: Vec<(&str, usize, usize)> = Vec::new();
            for (step, g) in &r.recorded {
                match then.iter_mut().find(|(name, _, _)| name == g) {
                    Some((_, count, _)) => *count += 1,
                    None => then.push((g.as_str(), 1, *step)),
                }
            }
            let mut recorded = String::new();
            if !then.is_empty() {
                let list: Vec<String> = then
                    .iter()
                    .map(|(g, count, first)| match count {
                        1 => format!("{g} at {first}"),
                        _ => format!("{g} {count} times from {first}"),
                    })
                    .collect();
                recorded = format!("; recorded then: {}", list.join(", "));
            }
            lines.push(format!(
                "  run {}: requests {} to {}{recorded}",
                n + 1,
                r.first_step,
                r.last_step
            ));
            for h in &r.hits {
                let what = match h.what {
                    "stop" => format!(
                        "stop ({} requests before the recording ended)",
                        r.last_step.saturating_sub(h.step)
                    ),
                    w => w.to_owned(),
                };
                let text: String = h.text.chars().take(100).collect();
                lines.push(format!("    after {}: {} {what}: {text}", h.step, h.guard));
            }
        }
        if !lines.is_empty() {
            out.push_str(&format!("{}\n{}\n", s.path.display(), lines.join("\n")));
        }
    }
    by_guard.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let guards: Vec<String> = by_guard.iter().map(|(g, c)| format!("{g} {c}")).collect();
    out.push_str(&format!(
        "{} sessions, {runs} runs; today's guards would stop {stopped}",
        sessions.len()
    ));
    if stopped > 0 {
        out.push_str(&format!(
            " ({}), {saved} requests before the recordings ended",
            guards.join(", ")
        ));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use psi_session::{Block, IdGen};
    use serde_json::json;

    fn user(t: &str) -> EntryBody {
        EntryBody::Message {
            role: Role::User,
            content: vec![Block::Text { text: t.into() }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }

    fn call(id: &str, name: &str, args: serde_json::Value) -> EntryBody {
        EntryBody::Message {
            role: Role::Assistant,
            content: vec![Block::ToolCall {
                id: id.into(),
                name: name.into(),
                arguments: args,
                raw: None,
            }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }

    fn answer(t: &str) -> EntryBody {
        EntryBody::Message {
            role: Role::Assistant,
            content: vec![Block::Text { text: t.into() }],
            usage: None,
            stop_reason: None,
            error: None,
            model: None,
        }
    }

    fn result(id: &str, failed: bool) -> EntryBody {
        EntryBody::ToolResult {
            tool_call_id: id.into(),
            tool_name: "bash".into(),
            content: vec![Block::Text { text: "out".into() }],
            is_error: failed,
        }
    }

    fn entries(bodies: Vec<EntryBody>) -> Vec<Entry> {
        let mut ids = IdGen::default();
        bodies.into_iter().map(|b| ids.push(0, b)).collect()
    }

    /// The 2026-11-06 loop's shape: two commands in turn, each working,
    /// nothing changing. Recorded before the stop existed, 40 requests long.
    fn looping(n: usize) -> Vec<EntryBody> {
        let mut b = vec![user("look around")];
        for i in 0..n {
            let id = format!("c{i}");
            let cmd = if i % 2 == 0 { "echo a" } else { "echo b" };
            // The model rewords the intent; the guards leave it out.
            b.push(call(
                &id,
                "bash",
                json!({"command": cmd, "intent": format!("look {i}")}),
            ));
            b.push(result(&id, false));
        }
        b
    }

    #[test]
    fn a_recorded_loop_is_nudged_noted_and_stopped_where_the_loop_would() {
        let runs = replay_guards(&entries(looping(40)));
        assert_eq!(runs.len(), 1);
        let r = &runs[0];
        assert_eq!((r.first_step, r.last_step, r.steps()), (1, 40, 40));
        let hits: Vec<(usize, &str, &str)> = r
            .hits
            .iter()
            .map(|h| (h.step, h.guard.as_str(), h.what))
            .collect();
        // As the agent's own test of the same calls: the nudge after six
        // repeats, the note in place of the first stop, then the stop.
        assert_eq!(
            hits,
            [
                (8, "no_progress", "nudge"),
                (14, "no_progress", "note"),
                (20, "no_progress", "nudge"),
                (26, "no_progress", "stop"),
            ]
        );
        assert_eq!((r.stop, r.saved()), (Some(26), 14));
    }

    #[test]
    fn progress_failures_and_run_boundaries_are_read_from_the_recording() {
        let mut b = looping(5);
        // A write that worked is progress, with or without a checkpoint.
        b.push(call("w", "write", json!({"path": "a", "text": "x"})));
        b.push(result("w", false));
        b.extend(looping(5).into_iter().skip(1));
        b.push(answer("Done."));
        // A second run: the same failing call five times.
        b.push(user("again"));
        for i in 0..6 {
            let id = format!("f{i}");
            b.push(call(&id, "bash", json!({"command": "ls nope"})));
            b.push(result(&id, true));
        }
        b.push(EntryBody::GuardEvent {
            guard: "repeated_failure".into(),
            message: "then".into(),
            tool_call_id: None,
        });
        let runs = replay_guards(&entries(b));
        assert_eq!(runs.len(), 2);
        assert!(runs[0].hits.is_empty(), "{:?}", runs[0].hits);
        assert_eq!((runs[0].first_step, runs[0].last_step), (1, 12));
        let second = &runs[1];
        assert_eq!((second.first_step, second.last_step), (13, 18));
        let hits: Vec<(usize, &str, &str)> = second
            .hits
            .iter()
            .map(|h| (h.step, h.guard.as_str(), h.what))
            .collect();
        assert_eq!(
            hits,
            [
                (15, "repeated_failure", "nudge"),
                (17, "repeated_failure", "stop")
            ]
        );
        assert_eq!(second.recorded, [(18, "repeated_failure".to_owned())]);
        let text = report(
            &[SessionReplay {
                path: "s.jsonl".into(),
                runs,
            }],
            true,
        );
        assert_eq!(
            text,
            "s.jsonl\n  run 2: requests 13 to 18; recorded then: repeated_failure at 18\n    after 15: repeated_failure nudge: This call failed the same way 3 times; change the call or try another approach.\n    after 17: repeated_failure stop (1 requests before the recording ended): the same call failed 5 times in a row\n1 sessions, 2 runs; today's guards would stop 1 (repeated_failure 1), 1 requests before the recordings ended\n"
        );
    }

    #[test]
    fn sessions_are_found_under_a_directory() {
        let d = tempfile::tempdir().unwrap();
        let trial = d.path().join("run/task/trial-0");
        std::fs::create_dir_all(trial.join("home/.psi/sessions")).unwrap();
        for f in [
            "session.jsonl",
            "wire.jsonl",
            "run.jsonl",
            "home/.psi/sessions/x.jsonl",
        ] {
            std::fs::write(trial.join(f), "").unwrap();
        }
        std::fs::write(d.path().join("a.jsonl"), "").unwrap();
        std::fs::write(d.path().join("a.wire.jsonl"), "").unwrap();
        let found = session_files(d.path()).unwrap();
        assert_eq!(
            found,
            [d.path().join("a.jsonl"), trial.join("session.jsonl")]
        );
        assert!(session_files(&d.path().join("missing")).is_err());
        assert_eq!(replay_paths(&[d.path().to_owned()]).unwrap().len(), 2);
    }
}
