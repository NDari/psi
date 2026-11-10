//! Guardrails (psi-plan.md §8.6): the harness notices loops and error
//! streaks and says one line, instead of hoping the model notices.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

/// Identical failing calls in a row before the nudge, and before a pause (H-12).
pub const REPEAT_NUDGE: u32 = 3;
pub const REPEAT_PAUSE: u32 = 5;
/// Turns that only repeat earlier calls and change no file, before a nudge (G-12).
pub const NO_PROGRESS_TURNS: u32 = 6;
/// Such turns in a row before a pause: the nudge went unheeded. Replayed
/// over the recorded sessions and eval trials (2026-11-09, about 800 Psi
/// runs): 18 reach this count, none of them changed a file afterwards, and
/// the two that were eval trials failed. The 2026-11-06 TUI rerun got 103
/// nudges and went on for 106 minutes.
pub const NO_PROGRESS_PAUSE: u32 = 2 * NO_PROGRESS_TURNS;
/// Calls a loop note lists; the rest are counted.
const NOTE_CALLS: usize = 10;
/// Characters of each call a loop note shows.
const NOTE_CALL_CHARS: usize = 200;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardAction {
    /// Add this line to the call's result.
    Nudge { guard: &'static str, text: String },
    /// Stop the run, with the reason.
    Pause { guard: &'static str, reason: String },
}

#[derive(Debug)]
pub struct Guards {
    /// Nudge after this many failed calls in a row; pause after twice that.
    max_errors: u32,
    last_failed: Option<String>,
    repeats: u32,
    streak: u32,
    seen: HashSet<String>,
    idle_turns: u32,
    turn_progress: bool,
    announced: bool,
    truncated: u32,
    /// This turn's calls, and the distinct calls of the turns that made
    /// no progress so far, in order: what a loop note names.
    turn_calls: Vec<String>,
    idle_calls: Vec<String>,
    /// The no-progress pause's reason and the note to go on with.
    loop_note: Option<(String, String)>,
}

/// What the model is told when a run goes on after a no-progress pause.
/// Going on as things were repeated in 40 of 42 samples of 14 recorded
/// loops; with this note as a user message, in 14, and 17 samples started
/// on the file the task asked for (experiments.md, 2026-11-09). Cutting
/// the repeats out of the context did little without it.
fn loop_note(calls: &[String]) -> String {
    let mut lines: Vec<String> = calls.iter().take(NOTE_CALLS).map(|c| shown(c)).collect();
    if calls.len() > NOTE_CALLS {
        lines.push(format!("- and {} more", calls.len() - NOTE_CALLS));
    }
    format!(
        "[Note from psi] You made the same calls again and again, the last {NO_PROGRESS_PAUSE} turns in a row, and no file changed. The calls:\n{}\nDon't make them again: use what you have, do something different, or answer.",
        lines.join("\n")
    )
}

/// One call of a loop note: the tool, and its command or path (all its
/// arguments when it has neither), on one line.
fn shown(sig: &str) -> String {
    let (name, args) = sig.split_once(' ').unwrap_or((sig, ""));
    let parsed: Option<serde_json::Value> = serde_json::from_str(args).ok();
    let main = parsed.as_ref().and_then(|v| {
        ["command", "path"]
            .iter()
            .find_map(|k| v.get(k).and_then(serde_json::Value::as_str))
    });
    let text = main.unwrap_or(args);
    let mut line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > NOTE_CALL_CHARS {
        line = line.chars().take(NOTE_CALL_CHARS - 1).collect();
        line.push('…');
    }
    format!("- {name}: {line}")
}

/// Answers cut off by the output limit that get another turn, per run.
pub const TRUNCATED_RETRIES: u32 = 2;

/// "Now I will examine...", "Let me run the tests." at the end of an answer.
#[allow(clippy::expect_used)] // A constant pattern; tests cover it.
static ANNOUNCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(i will|i'll|let me|let's|i am going to|i'm going to|next,? i)\b")
        .expect("valid regex")
});

impl Guards {
    pub fn new(max_consecutive_errors: u32) -> Self {
        Self {
            max_errors: max_consecutive_errors.max(1),
            last_failed: None,
            repeats: 0,
            streak: 0,
            seen: HashSet::new(),
            idle_turns: 0,
            turn_progress: false,
            announced: false,
            truncated: 0,
            turn_calls: Vec::new(),
            idle_calls: Vec::new(),
            loop_note: None,
        }
    }

    /// After the user chose to go on: the counts start over.
    pub fn reset(&mut self) {
        self.last_failed = None;
        self.repeats = 0;
        self.streak = 0;
        self.idle_turns = 0;
        self.idle_calls.clear();
    }

    /// After a no-progress pause: its reason, and the note that names the
    /// repeated calls, for the model if the run goes on. Once per pause.
    pub fn take_loop_note(&mut self) -> Option<(String, String)> {
        self.loop_note.take()
    }

    /// One call's outcome; `sig` is the tool and its arguments.
    pub fn on_call(&mut self, sig: &str, failed: bool) -> Vec<GuardAction> {
        if self.seen.insert(sig.to_owned()) {
            self.turn_progress = true;
        }
        self.turn_calls.push(sig.to_owned());
        let mut out = Vec::new();
        if !failed {
            self.last_failed = None;
            self.repeats = 0;
            self.streak = 0;
            return out;
        }
        self.streak += 1;
        if self.last_failed.as_deref() == Some(sig) {
            self.repeats += 1;
        } else {
            self.last_failed = Some(sig.to_owned());
            self.repeats = 1;
        }
        if self.repeats == REPEAT_PAUSE {
            out.push(GuardAction::Pause {
                guard: "repeated_failure",
                reason: format!("the same call failed {REPEAT_PAUSE} times in a row"),
            });
        } else if self.repeats == REPEAT_NUDGE {
            out.push(GuardAction::Nudge {
                guard: "repeated_failure",
                text: format!("This call failed the same way {REPEAT_NUDGE} times; change the call or try another approach."),
            });
        }
        if self.streak == 2 * self.max_errors {
            out.push(GuardAction::Pause {
                guard: "error_streak",
                reason: format!("{} tool calls in a row failed", self.streak),
            });
        } else if self.streak == self.max_errors {
            out.push(GuardAction::Nudge {
                guard: "error_streak",
                text: format!("{} tool calls in a row failed. Re-read the relevant files and the last error, and re-plan before the next call.", self.streak),
            });
        }
        out
    }

    /// The model answered without a tool call. If the answer ends by
    /// announcing a next step ("Now I will read tax.py.") rather than a
    /// result or a question, it most likely meant to make the call: say so,
    /// once per run.
    pub fn on_answer(&mut self, text: &str) -> Option<GuardAction> {
        if self.announced {
            return None;
        }
        let last = text.trim().rsplit("\n\n").next().unwrap_or("").trim();
        let ends_open = last.ends_with(':') || last.ends_with('.') || last.ends_with("...");
        if last.is_empty() || last.contains('?') || last.len() > 400 || !ends_open {
            return None;
        }
        // Only the last sentence or two count: "I'll explain below" deep in
        // an answer is not an announcement.
        let tail: String = last
            .chars()
            .rev()
            .take(200)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !ANNOUNCE.is_match(&tail) {
            return None;
        }
        self.announced = true;
        Some(GuardAction::Nudge {
            guard: "announced_stop",
            text: "You described a next step but made no tool call, so nothing ran. Make the call now, or answer if you are done.".into(),
        })
    }

    /// The answer hit the output limit before any tool call: it was cut
    /// off, not finished. Say so, at most twice per run.
    pub fn on_truncated(&mut self, max_output: u64) -> Option<GuardAction> {
        if self.truncated >= TRUNCATED_RETRIES {
            return None;
        }
        self.truncated += 1;
        Some(GuardAction::Nudge {
            guard: "truncated_answer",
            text: format!(
                "Your reply hit the output limit ({max_output} tokens) and was cut off before any tool call. Don't restate your plan: make the next tool call now, or give a short answer."
            ),
        })
    }

    /// The end of a turn with tool calls; `files_changed` from the checkpoint.
    pub fn on_turn_end(&mut self, files_changed: bool) -> Option<GuardAction> {
        let progress = std::mem::take(&mut self.turn_progress) || files_changed;
        let calls = std::mem::take(&mut self.turn_calls);
        if progress {
            self.idle_turns = 0;
            self.idle_calls.clear();
            return None;
        }
        self.idle_turns += 1;
        for c in calls {
            if !self.idle_calls.contains(&c) {
                self.idle_calls.push(c);
            }
        }
        if self.idle_turns == NO_PROGRESS_PAUSE {
            // Going on resets the count (`reset`), so the nudge comes again
            // before the next pause.
            let reason = format!(
                "the last {NO_PROGRESS_PAUSE} turns repeated earlier calls and changed no file"
            );
            self.loop_note = Some((reason.clone(), loop_note(&self.idle_calls)));
            return Some(GuardAction::Pause {
                guard: "no_progress",
                reason,
            });
        }
        (self.idle_turns == NO_PROGRESS_TURNS).then(|| GuardAction::Nudge {
            guard: "no_progress",
            text: format!(
                "The last {NO_PROGRESS_TURNS} turns repeated earlier calls and changed no file. Try a different approach, or answer if you are done."
            ),
        })
    }

    /// The context was summarized before a request. The results of
    /// earlier calls are gone from the context, so making one again is not
    /// a repeat.
    pub fn on_compaction(&mut self) {
        self.seen.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guards(a: &[GuardAction]) -> Vec<&'static str> {
        a.iter()
            .map(|g| match g {
                GuardAction::Nudge { guard, .. } => *guard,
                GuardAction::Pause { guard, .. } => *guard,
            })
            .collect()
    }

    #[test]
    fn repeats_nudge_at_three_and_pause_at_five() {
        let mut g = Guards::new(10);
        assert!(g.on_call("a", true).is_empty());
        assert!(g.on_call("a", true).is_empty());
        let n = g.on_call("a", true);
        assert!(matches!(
            &n[..],
            [GuardAction::Nudge {
                guard: "repeated_failure",
                ..
            }]
        ));
        assert!(g.on_call("a", true).is_empty());
        let p = g.on_call("a", true);
        assert!(matches!(
            &p[..],
            [GuardAction::Pause {
                guard: "repeated_failure",
                ..
            }]
        ));
        // A different call starts the count over; a success clears it.
        let mut g = Guards::new(10);
        g.on_call("a", true);
        g.on_call("a", true);
        assert!(g.on_call("b", true).is_empty());
        g.on_call("a", true);
        g.on_call("a", false);
        assert!(g.on_call("a", true).is_empty());
    }

    #[test]
    fn error_streaks_nudge_then_pause() {
        let mut g = Guards::new(3);
        let mut fired = Vec::new();
        for i in 0..6 {
            fired.extend(guards(&g.on_call(&i.to_string(), true)));
        }
        assert_eq!(fired, ["error_streak", "error_streak"]);
    }

    #[test]
    fn an_announced_step_without_a_call_is_nudged_once() {
        let mut g = Guards::new(10);
        // Gemma 4 12B, psi eval fix_invoice_rounding (2026-10-31): the run
        // ended here.
        let gemma = "Total: 26.37 + 4.73 + 0.11 = 31.21\n\nNow I will examine the `billing` package to see how these are calculated. I'll start with `billing/money.py` and `billing/tax.py`.";
        assert!(matches!(
            g.on_answer(gemma),
            Some(GuardAction::Nudge {
                guard: "announced_stop",
                ..
            })
        ));
        assert_eq!(g.on_answer(gemma), None, "once per run");
        let mut g = Guards::new(10);
        for done in [
            "Fixed: parse_amount now parses the decimal string exactly. All 12 tests pass.",
            "Should I also update the README?",
            "I'll leave the formatting as it is, since the tests pin it",
            "",
        ] {
            assert_eq!(g.on_answer(done), None, "{done}");
        }
    }

    #[test]
    fn no_progress_after_repeated_turns() {
        let mut g = Guards::new(10);
        g.on_call("read a", false);
        assert_eq!(g.on_turn_end(false), None);
        for _ in 0..NO_PROGRESS_TURNS - 1 {
            g.on_call("read a", false);
            assert_eq!(g.on_turn_end(false), None);
        }
        g.on_call("read a", false);
        assert!(g.on_turn_end(false).is_some());
        // A file change is progress.
        for _ in 0..NO_PROGRESS_TURNS + 1 {
            g.on_call("read a", false);
            assert_eq!(g.on_turn_end(true), None);
        }
    }

    /// One turn that makes `sig`, with no file changed.
    fn idle_turn(g: &mut Guards, sig: &str) -> Option<GuardAction> {
        g.on_call(sig, false);
        g.on_turn_end(false)
    }

    #[test]
    fn an_unheeded_no_progress_nudge_becomes_a_pause() {
        // The 2026-11-06 TUI rerun: two exact greps in turn, 623 calls.
        let mut g = Guards::new(10);
        idle_turn(&mut g, "grep a");
        idle_turn(&mut g, "grep b");
        let mut fired = Vec::new();
        for i in 0..NO_PROGRESS_PAUSE {
            let sig = if i % 2 == 0 { "grep a" } else { "grep b" };
            fired.push(idle_turn(&mut g, sig));
        }
        let at = |n: u32| fired[n as usize - 1].clone();
        assert!(matches!(
            at(NO_PROGRESS_TURNS),
            Some(GuardAction::Nudge {
                guard: "no_progress",
                ..
            })
        ));
        assert_eq!(
            at(NO_PROGRESS_PAUSE),
            Some(GuardAction::Pause {
                guard: "no_progress",
                reason: "the last 12 turns repeated earlier calls and changed no file".into(),
            })
        );
        assert_eq!(fired.iter().flatten().count(), 2, "{fired:?}");
        // The user went on: the nudge comes again first, then the pause.
        g.reset();
        let again: Vec<_> = (0..NO_PROGRESS_PAUSE)
            .filter_map(|_| idle_turn(&mut g, "grep a"))
            .collect();
        assert!(
            matches!(
                &again[..],
                [GuardAction::Nudge { .. }, GuardAction::Pause { .. }]
            ),
            "{again:?}"
        );
    }

    #[test]
    fn the_pause_comes_with_a_note_that_names_the_repeated_calls() {
        // The 2026-11-06 TUI rerun's two commands, as the agent signs them.
        let a = r#"bash {"command":"grep -rn \"func.*Text()\" embedding.go;\n  grep -n -B3 -A8 \") Text()\" embedding.go | head -15"}"#;
        let b = r#"read {"limit":40,"path":"embedding.go"}"#;
        let mut g = Guards::new(10);
        idle_turn(&mut g, a);
        idle_turn(&mut g, b);
        assert_eq!(g.take_loop_note(), None);
        for i in 0..NO_PROGRESS_PAUSE {
            idle_turn(&mut g, if i % 2 == 0 { a } else { b });
        }
        let (reason, note) = g.take_loop_note().unwrap();
        assert_eq!(
            reason,
            "the last 12 turns repeated earlier calls and changed no file"
        );
        assert_eq!(
            note,
            "[Note from psi] You made the same calls again and again, the last 12 turns in a row, and no file changed. The calls:\n\
             - bash: grep -rn \"func.*Text()\" embedding.go; grep -n -B3 -A8 \") Text()\" embedding.go | head -15\n\
             - read: embedding.go\n\
             Don't make them again: use what you have, do something different, or answer."
        );
        // Once per pause.
        assert_eq!(g.take_loop_note(), None);
    }

    #[test]
    fn a_loop_note_lists_ten_calls_cuts_long_ones_and_counts_the_rest() {
        let mut g = Guards::new(10);
        let sigs: Vec<String> = (0..12)
            .map(|i| format!(r#"bash {{"command":"echo {i} {}"}}"#, "x".repeat(300)))
            .collect();
        // Twelve calls in each turn; the first turn makes them new.
        for _ in 0..=NO_PROGRESS_PAUSE {
            for s in &sigs {
                g.on_call(s, false);
            }
            g.on_turn_end(false);
        }
        let (_, note) = g.take_loop_note().unwrap();
        let lines: Vec<&str> = note.lines().collect();
        assert_eq!(lines.len(), 13, "{note}");
        assert!(lines[1].starts_with("- bash: echo 0 xxx") && lines[1].ends_with('…'));
        assert_eq!(lines[1].chars().count(), "- bash: ".len() + 200);
        assert_eq!(lines[11], "- and 2 more");
        // A call that isn't a tool and JSON arguments is shown as it is.
        assert_eq!(shown("strange"), "- strange: ");
        assert_eq!(
            shown(r#"todo {"action":"list"}"#),
            r#"- todo: {"action":"list"}"#
        );
    }

    #[test]
    fn a_new_call_or_a_changed_file_between_repeats_starts_the_count_over() {
        let mut g = Guards::new(10);
        idle_turn(&mut g, "read a");
        for _ in 0..NO_PROGRESS_PAUSE - 1 {
            idle_turn(&mut g, "read a");
        }
        // One turn short of the pause: a call not made before.
        assert_eq!(idle_turn(&mut g, "read b"), None);
        for _ in 0..NO_PROGRESS_TURNS - 1 {
            assert_eq!(idle_turn(&mut g, "read a"), None);
        }
        // A changed file does the same.
        g.on_call("read a", false);
        assert_eq!(g.on_turn_end(true), None);
        for _ in 0..NO_PROGRESS_TURNS - 1 {
            assert_eq!(idle_turn(&mut g, "read a"), None);
        }
        assert!(matches!(
            idle_turn(&mut g, "read a"),
            Some(GuardAction::Nudge { .. })
        ));
    }

    #[test]
    fn reading_again_after_a_summary_is_not_a_repeat() {
        // multi_split_package at 16K (2026-11-01): the file was read in
        // four pieces again after each summary had replaced them.
        let mut g = Guards::new(10);
        let pieces = ["sed 1,200p", "sed 200,400p", "sed 400,600p", "sed 600,837p"];
        for _ in 0..4 {
            for p in pieces {
                assert_eq!(idle_turn(&mut g, p), None);
            }
            g.on_compaction();
        }
    }
}
