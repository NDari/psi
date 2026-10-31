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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardAction {
    /// Add this line to the call's result.
    Nudge { guard: &'static str, text: String },
    /// Stop and ask the user whether to go on (headless runs stop).
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
        }
    }

    /// After the user chose to go on: the counts start over.
    pub fn reset(&mut self) {
        self.last_failed = None;
        self.repeats = 0;
        self.streak = 0;
        self.idle_turns = 0;
    }

    /// One call's outcome; `sig` is the tool and its arguments.
    pub fn on_call(&mut self, sig: &str, failed: bool) -> Vec<GuardAction> {
        if self.seen.insert(sig.to_owned()) {
            self.turn_progress = true;
        }
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
        if progress {
            self.idle_turns = 0;
            return None;
        }
        self.idle_turns += 1;
        if self.idle_turns < NO_PROGRESS_TURNS {
            return None;
        }
        self.idle_turns = 0;
        Some(GuardAction::Nudge {
            guard: "no_progress",
            text: format!(
                "The last {NO_PROGRESS_TURNS} turns repeated earlier calls and changed no file. Try a different approach, or answer if you are done."
            ),
        })
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
}
