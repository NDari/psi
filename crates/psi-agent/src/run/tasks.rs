//! Carrying out a plan, one item at a time (design-going-forward.md, "The
//! shape"). Each open item is one task: a fresh context that starts with
//! the plan, the item and the harness's record of the items before it; the
//! project's check at the end; a report from the harness's own records;
//! and a tick in the plan when the task ended by itself and the check
//! passed. The first item left open stops the plan.

use std::path::Path;
use std::time::Instant;

use psi_session::EntryBody;
use serde_json::{Value, json};

use super::{Agent, Stop};
use crate::events::AgentEvent;
use crate::plan::{
    CheckResult, TASK_REPORT, TASK_START, TaskReport, items, task_prompt, tick, untick,
};
use crate::post_edit;

/// Tasks one `run_plan` carries out at most: a plan that grows as it runs
/// (the model adds items) still ends.
const MAX_TASKS: usize = 100;

/// How a plan run went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutcome {
    /// The tasks carried out, in order.
    pub reports: Vec<TaskReport>,
    /// Items still open in the plan.
    pub open: usize,
    /// Every item is done.
    pub complete: bool,
    /// Why it ended, for the user.
    pub end: String,
}

impl Agent {
    /// Carries out the open items of the plan file at `plan`, in order,
    /// until one is left open or none is left.
    pub async fn run_plan(&mut self, plan: &Path) -> PlanOutcome {
        let chat_budgets = self.cfg.budgets.clone();
        self.cfg.budgets = self.cfg.task_budgets.clone();
        let out = self.run_plan_inner(plan).await;
        self.cfg.budgets = chat_budgets;
        self.emit(AgentEvent::PlanEnd {
            done: out.reports.iter().filter(|r| r.ticked).count(),
            open: out.open,
            reason: out.end.clone(),
        });
        out
    }

    async fn run_plan_inner(&mut self, plan: &Path) -> PlanOutcome {
        let shown = plan.display().to_string();
        let path = self.ctx.cwd.join(plan);
        let mut reports: Vec<TaskReport> = Vec::new();
        let mut before = self.earlier_reports(&shown);
        let end = |reports: Vec<TaskReport>, open: usize, end: String| PlanOutcome {
            reports,
            open,
            complete: false,
            end,
        };
        loop {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => return end(reports, 0, format!("cannot read {shown}: {e}")),
            };
            let all = items(&text);
            let open = all.iter().filter(|i| !i.done).count();
            if all.is_empty() {
                let why =
                    format!("{shown} has no checklist items (lines like `- [ ] add the parser`)");
                return end(reports, 0, why);
            }
            let Some(at) = all.iter().position(|i| !i.done) else {
                return PlanOutcome {
                    reports,
                    open: 0,
                    complete: true,
                    end: format!("all {} items of {shown} are done", all.len()),
                };
            };
            if reports.len() >= MAX_TASKS {
                let why = format!("stopped after {MAX_TASKS} tasks; {open} items are open");
                return end(reports, open, why);
            }
            let (n, total, item) = (at + 1, all.len(), all[at].text.clone());
            self.emit(AgentEvent::TaskStart {
                n,
                total,
                item: item.clone(),
            });
            let mut report = match self.run_task(&shown, &text, &item, n, total, &before).await {
                Ok(r) => r,
                Err(e) => return end(reports, open, format!("item {n} failed: {e}")),
            };
            // The model may have changed the plan: tick the file as it is now.
            let now = std::fs::read_to_string(&path).unwrap_or(text);
            let written = if report.open_because.is_none() {
                match tick(&now, &item) {
                    Some(t) => std::fs::write(&path, t).map_err(|e| e.to_string()),
                    // Ticked by the model already.
                    None if items(&now).iter().any(|i| i.done && i.text == item) => Ok(()),
                    None => Err("its line is no longer in the plan".to_owned()),
                }
            } else {
                let open_again = untick(&now, &item);
                if open_again != now {
                    let _ = std::fs::write(&path, open_again);
                }
                Ok(())
            };
            match written {
                Ok(()) => report.ticked = report.open_because.is_none(),
                Err(e) => report.open_because = Some(format!("could not tick it: {e}")),
            }
            let recorded = self.record(EntryBody::Custom {
                custom_type: TASK_REPORT.into(),
                data: serde_json::to_value(&report).unwrap_or(Value::Null),
            });
            self.emit(AgentEvent::TaskEnd {
                report: report.clone(),
            });
            let stopped = report.open_because.clone();
            before.push(report.clone());
            reports.push(report);
            if let Err(e) = recorded {
                return end(reports, open, format!("item {n} failed: {e}"));
            }
            if let Some(why) = stopped {
                return end(
                    reports,
                    open,
                    format!("stopped at item {n} of {total}: {why}"),
                );
            }
        }
    }

    /// The reports of this plan's tasks that ended well earlier in this
    /// session: the record a later task starts with.
    fn earlier_reports(&self, plan: &str) -> Vec<TaskReport> {
        self.session
            .branch()
            .iter()
            .filter_map(|e| match &e.body {
                EntryBody::Custom { custom_type, data } if custom_type == TASK_REPORT => {
                    serde_json::from_value::<TaskReport>(data.clone()).ok()
                }
                _ => None,
            })
            .filter(|r| r.plan == plan && r.ticked)
            .collect()
    }

    /// One item, in a fresh context. The report is not yet ticked.
    async fn run_task(
        &mut self,
        plan: &str,
        plan_text: &str,
        item: &str,
        n: usize,
        total: usize,
        before: &[TaskReport],
    ) -> Result<TaskReport, String> {
        let started = Instant::now();
        // The boundary: no request after it sends what came before.
        self.record(EntryBody::Custom {
            custom_type: TASK_START.into(),
            data: json!({"plan": plan, "item": item, "n": n, "total": total}),
        })?;
        let first = self.session.entries().len();
        let out = self.run(&task_prompt(plan, plan_text, item, before)).await;

        let mut check = None;
        let open_because = match &out.stop {
            Stop::Done => match self.final_check().await {
                Some(c) if !c.passed => {
                    check = Some(c);
                    Some("the check failed".to_owned())
                }
                c => {
                    check = c;
                    None
                }
            },
            Stop::Aborted => Some("you stopped the task".to_owned()),
            Stop::Failed(_) => Some("the task failed".to_owned()),
            Stop::Budget(_) | Stop::Guard(_) => Some("the task did not end by itself".to_owned()),
        };
        self.capture("task", None).await?;

        let entries = &self.session.entries()[first..];
        let from = entries.iter().find_map(|e| match &e.body {
            EntryBody::Checkpoint {
                commit,
                trigger: Some(t),
                ..
            } if t == "submit" => Some(commit.clone()),
            _ => None,
        });
        let refused = entries
            .iter()
            .filter(|e| matches!(e.body, EntryBody::PolicyDecision { .. }))
            .count() as u32;
        // Without checkpoints: the files the write and edit calls named.
        let mut changed: Vec<String> = Vec::new();
        for e in entries {
            for (_, name, args) in e.tool_calls() {
                if let ("write" | "edit", Some(p)) = (name, args["path"].as_str()) {
                    let line = format!("M {p}");
                    if !changed.contains(&line) {
                        changed.push(line);
                    }
                }
            }
        }
        let (mut to, mut refs_moved) = (None, false);
        if let (Some(cp), Some(from)) = (self.safety.checkpoints.clone(), from.clone()) {
            to = cp.head();
            if let Some(to) = &to {
                changed = cp
                    .changes(&from, to)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .map(|c| format!("{} {}", c.status, c.path.display()))
                    .collect();
                refs_moved = cp.refs_differ(&from, to);
            }
        }
        Ok(TaskReport {
            plan: plan.to_owned(),
            item: item.to_owned(),
            n,
            total,
            stop: out.stop.name().to_owned(),
            reason: out.stop.reason().map(str::to_owned),
            changed,
            refs_moved,
            check,
            requests: out.turns,
            tool_calls: out.tool_calls,
            refused,
            secs: started.elapsed().as_secs(),
            ticked: false,
            open_because,
            from,
            to,
        })
    }

    /// The project's check (the `post_edit_check` setting), run by the
    /// harness when a task ended by itself: the model's word that it works
    /// is not evidence. `None` when no check is set.
    async fn final_check(&mut self) -> Option<CheckResult> {
        let check = self.post_edit.clone()?;
        let args = json!({"command": check.command, "timeout": check.timeout_s});
        let Value::Object(args) = args else {
            return None;
        };
        let out = psi_tools::bash::run(args, &self.ctx).await;
        let summary = post_edit::summary(&check.command, &out);
        Some(CheckResult {
            command: check.command,
            passed: !out.is_error,
            output: summary.lines().skip(1).collect::<Vec<_>>().join("\n"),
        })
    }
}
