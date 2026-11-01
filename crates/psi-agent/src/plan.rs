//! Plan mode (psi-plan.md §10, pi parity, D-34): the user turns it on; the
//! model explores with read-only tools and answers with a numbered plan; the
//! user can then have it carried out, with `[DONE:n]` markers tracking the
//! steps.

use serde_json::{Value, json};

/// Added after each prompt while plan mode is on.
pub const PLAN_CONTEXT: &str = "[Plan mode] Explore and plan only: write and edit are off, and bash runs only read-only commands. Don't try to change files. When you are ready, answer with a numbered plan under a \"Plan:\" heading:

Plan:
1. First step
2. Second step";

/// Added to the next prompt after plan mode is turned off by hand.
pub const PLAN_OFF: &str = "[Plan mode is off: all tools are available again.]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanState {
    /// Read-only tools only, and the plan context on each prompt.
    pub enabled: bool,
    /// The last plan proposed, or the one being carried out.
    pub steps: Vec<Step>,
    /// The plan is being carried out.
    pub executing: bool,
}

impl PlanState {
    /// The snapshot kept in the session's `plan` entries.
    pub fn to_json(&self) -> Value {
        let steps: Vec<Value> = self
            .steps
            .iter()
            .map(|s| json!({"text": s.text, "done": s.done}))
            .collect();
        json!({"enabled": self.enabled, "executing": self.executing, "steps": steps})
    }

    pub fn from_json(v: &Value) -> Self {
        Self {
            enabled: v["enabled"].as_bool().unwrap_or(false),
            executing: v["executing"].as_bool().unwrap_or(false),
            steps: v["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| Step {
                    text: s["text"].as_str().unwrap_or("").to_owned(),
                    done: s["done"].as_bool().unwrap_or(false),
                })
                .collect(),
        }
    }

    pub fn done_count(&self) -> usize {
        self.steps.iter().filter(|s| s.done).count()
    }

    /// Marks the steps named by `[DONE:n]` in `text`; true when one changed.
    pub fn mark_done(&mut self, text: &str) -> bool {
        let mut changed = false;
        for n in done_markers(text) {
            if let Some(s) = n.checked_sub(1).and_then(|i| self.steps.get_mut(i))
                && !s.done
            {
                s.done = true;
                changed = true;
            }
        }
        changed
    }
}

/// The prompt that starts carrying out a plan.
pub fn execute_prompt(steps: &[Step]) -> String {
    let list: Vec<String> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}", i + 1, s.text))
        .collect();
    format!(
        "Carry out the plan. Plan mode is off: all tools are available.\n\nSteps:\n{}\n\nStart with step 1. After you finish a step, write [DONE:n] in your answer (for example [DONE:1]).",
        list.join("\n")
    )
}

/// The numbered items after the first "Plan:" heading (pi parity): `1.` or
/// `1)`, markdown emphasis and code quotes removed, renumbered in order.
/// Unlike pi, the text is kept whole: the model reads it back.
pub fn extract_steps(message: &str) -> Vec<String> {
    let lower = message.to_ascii_lowercase();
    let Some(at) = header_end(&lower) else {
        return Vec::new();
    };
    let mut steps = Vec::new();
    for line in message[at..].lines() {
        let l = line.trim_start();
        let digits = l.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let rest = &l[digits..];
        let Some(rest) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let text = clean(rest.trim());
        if text.chars().count() <= 3 || text.starts_with(['/', '-']) {
            continue;
        }
        steps.push(text);
    }
    steps
}

/// Where the text after a "Plan:" heading starts (`**Plan:**` too).
fn header_end(lower: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = lower[from..].find("plan:") {
        let start = from + i;
        let line_start = lower[..start].rfind('\n').map_or(0, |n| n + 1);
        let before = lower[line_start..start].trim_start_matches(['#', ' ']);
        if before.trim_matches('*').is_empty() {
            let after = &lower[start + 5..];
            let eol = after.find('\n')?;
            if after[..eol].trim_matches(['*', ' ']).is_empty() {
                return Some(start + 5 + eol + 1);
            }
        }
        from = start + 5;
    }
    None
}

fn clean(s: &str) -> String {
    let s = s.replace("**", "").replace('`', "");
    let s = s.trim_matches('*');
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The step numbers in `[DONE:n]` markers, any case.
pub fn done_markers(text: &str) -> Vec<usize> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("[done:") {
        rest = &rest[i + 6..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && rest[digits..].starts_with(']')
            && let Ok(n) = rest[..digits].parse()
        {
            out.push(n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_after_the_plan_heading() {
        let m = "I looked around.\n\n**Plan:**\n1. Read `config.rs` first\n2) **Add** the flag\n3. OK\n   4. Run the tests\nNot a step\n5.no space\n";
        assert_eq!(
            extract_steps(m),
            ["Read config.rs first", "Add the flag", "Run the tests"]
        );
        assert_eq!(extract_steps("## Plan:\n1. One thing\n"), ["One thing"]);
        assert!(extract_steps("1. no heading\n").is_empty());
        assert!(extract_steps("The plan: is to x\n1. not this").is_empty());
    }

    #[test]
    fn done_markers_and_marking() {
        assert_eq!(done_markers("[DONE:1] x [done:3] [Done:] [DONE:a]"), [1, 3]);
        let mut p = PlanState {
            enabled: false,
            executing: true,
            steps: vec![
                Step {
                    text: "a".into(),
                    done: false,
                },
                Step {
                    text: "b".into(),
                    done: false,
                },
            ],
        };
        assert!(p.mark_done("finished [DONE:2] and [DONE:9]"));
        assert!(!p.mark_done("[DONE:2]"), "already done");
        assert_eq!(p.done_count(), 1);
        assert_eq!(PlanState::from_json(&p.to_json()), p);
    }

    #[test]
    fn execute_prompt_lists_the_steps() {
        let steps = [
            Step {
                text: "Read it".into(),
                done: false,
            },
            Step {
                text: "Fix it".into(),
                done: false,
            },
        ];
        let p = execute_prompt(&steps);
        assert!(p.contains("\nSteps:\n1. Read it\n2. Fix it\n\nStart with step 1."));
    }
}
