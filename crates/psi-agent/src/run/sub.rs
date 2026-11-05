//! Running a sub-agent inside the parent's tool call (psi-plan.md §10.1,
//! D-41): a second `Agent` with its own session and context, the parent's
//! policy, approver, extensions and cancel token, and no `agent` tool.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use psi_ai::WireLog;
use psi_session::{Block, EntryBody, SessionFile};
use psi_tools::{ToolCtx, ToolOutput};
use serde_json::{Map, Value, json};

use super::{Agent, AgentConfig, Budgets, RunOutcome, Safety, Stop};
use crate::events::AgentEvent;
use crate::subagent::{MAX_TURNS, REPORT, SUB_NOTE};

impl Agent {
    /// Runs the agent the call names on its task, and returns its last
    /// answer with one line on how it went. Boxed as `Send` by hand: the
    /// sub-agent's run contains this call again, which the compiler can't
    /// prove `Send` on its own.
    pub(super) fn run_subagent<'a>(
        &'a mut self,
        id: &'a str,
        args: &'a Map<String, Value>,
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput, String>> + Send + 'a>> {
        Box::pin(self.subagent_call(id, args))
    }

    async fn subagent_call(
        &mut self,
        id: &str,
        args: &Map<String, Value>,
    ) -> Result<ToolOutput, String> {
        let arg = |k: &str| {
            args.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned()
        };
        let (name, task) = (arg("agent"), arg("task"));
        let Some(def) = self.subagents.iter().find(|d| d.name == name).cloned() else {
            let names: Vec<&str> = self.subagents.iter().map(|d| d.name.as_str()).collect();
            return Ok(ToolOutput::err(format!(
                "There is no agent named '{name}'. Agents: {}.",
                names.join(", ")
            )));
        };
        if task.is_empty() {
            return Ok(ToolOutput::err(
                "Give the agent a task: what to do, with the paths and facts it needs.",
            ));
        }

        let path = self.subagent_path(&name);
        let session = SessionFile::create_at(&path).map_err(|e| {
            format!(
                "cannot create the sub-agent's session file {}: {e}",
                path.display()
            )
        })?;
        let client = match WireLog::open(&session.wire_path()) {
            Ok(w) => self.client.clone().with_wire(Arc::new(w)),
            Err(e) => {
                tracing::warn!("no wire log of its own for the sub-agent: {e}");
                self.client.clone()
            }
        };
        let read_only = def.read_only(&self.cfg.tools);
        let cfg = AgentConfig {
            model: self.cfg.model.clone(),
            max_output: self.cfg.max_output,
            n_ctx: self.cfg.n_ctx,
            enable_thinking: self.cfg.enable_thinking,
            // In the TUI a sub-agent has the parent's budgets (no limit unless
            // one is set; Esc and the guards still stop it); print runs and
            // evals keep MAX_TURNS as a backstop.
            budgets: if self.safety.approver.interactive() {
                self.cfg.budgets.clone()
            } else {
                Budgets {
                    max_turns: MAX_TURNS,
                    ..self.cfg.budgets.clone()
                }
            },
            system_prompt: format!("{}\n\n{SUB_NOTE}\n\n{}", self.cfg.system_prompt, def.prompt),
            tools: def.core_tools(&self.cfg.tools),
            home: self.cfg.home.clone(),
        };
        // Its own file and todo records; the parent's cwd, sandbox and
        // cancel token, so Esc stops both.
        let mut ctx = ToolCtx::new(&self.ctx.cwd, &self.ctx.tmpdir, self.ctx.max_result_chars);
        ctx.cancel = self.ctx.cancel.clone();
        ctx.sandbox = self.ctx.sandbox.clone();
        let safety = Safety {
            policy: self.safety.policy.clone(),
            // The parent checkpoints after the call.
            checkpoints: None,
            approver: self.safety.approver.clone(),
            grants: self.safety.grants.clone(),
            info: self.safety.info.clone(),
        };
        let mut child = Agent::new(client, session, cfg, ctx);
        // Set before the extensions, so it leaves their shared state alone.
        child.sub = Some(name.clone());
        child.reads_only = read_only;
        let mut child = child
            .with_safety(safety)
            .with_extensions(self.ext.clone())
            .with_mcp(self.mcp.clone());
        if let Some(hook) = &self.on_config_write {
            child = child.with_config_write(hook.clone());
        }
        if def.tools.is_some() {
            let allowed = child
                .tool_names()
                .into_iter()
                .filter(|n| def.allows(n))
                .collect();
            child.active = Some(allowed);
        }
        if let Some(tx) = self.events.clone() {
            let (child_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            child = child.with_events(child_tx);
            let agent = name.clone();
            tokio::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    if matches!(ev, AgentEvent::ToolStart { .. }) {
                        let _ = tx.send(AgentEvent::Subagent {
                            agent: agent.clone(),
                            event: Box::new(ev),
                        });
                    }
                }
            });
        }

        let mut out: RunOutcome = child.run(&task).await;
        let reported = matches!(out.stop, Stop::Budget(_) | Stop::Guard(_))
            && child.last_report(&mut out).await;

        let (mut tokens_in, mut tokens_out) = (0, 0);
        for e in child.session().entries() {
            if let EntryBody::Message { usage: Some(u), .. } = &e.body {
                tokens_in += u.prompt_total();
                tokens_out += u.output;
            }
        }
        self.record(EntryBody::Custom {
            custom_type: "subagent".into(),
            data: json!({"tool_call_id": id, "agent": name, "task": task,
                         "session": path.display().to_string(),
                         "stop": out.stop.name(), "reason": out.stop.reason(),
                         "turns": out.turns, "tool_calls": out.tool_calls,
                         "report": reported, "tokens_in": tokens_in, "tokens_out": tokens_out}),
        })?;
        if !read_only && self.capture("tool", Some(id)).await? {
            self.turn_changed = true;
        }

        let (mut state, is_error) = match &out.stop {
            Stop::Done => ("done".to_owned(), false),
            Stop::Budget(r) => (format!("stopped: {r}"), false),
            Stop::Guard(r) => (format!("paused: {r}"), false),
            Stop::Aborted => ("stopped by the user".to_owned(), true),
            Stop::Failed(r) => (format!("failed: {r}"), true),
        };
        if reported {
            state.push_str("; it wrote this report when stopped");
        }
        let answer = match out.text.trim() {
            "" => "(no answer)".to_owned(),
            t => cut(t, self.ctx.max_result_chars),
        };
        Ok(ToolOutput {
            text: format!(
                "{answer}\n\n[{name}: {} model requests, {} tool calls; {state}]",
                out.turns, out.tool_calls
            ),
            is_error,
        })
    }

    /// After a budget or a guard stopped this sub-agent: one more request
    /// asking for what it found so far, since the parent sees only its last
    /// answer, and a looping agent's is empty or mid-work (scout 02 in the
    /// cog trial returned "(no answer)" after 191 requests). The tools stay
    /// in the request so the prompt cache holds; calls it makes don't run.
    /// Whether it answered.
    async fn last_report(&mut self, out: &mut RunOutcome) -> bool {
        let reason = out.stop.reason().unwrap_or("").to_owned();
        let ask = EntryBody::CustomMessage {
            custom_type: "report".into(),
            content: vec![Block::Text {
                text: format!("Stopped: {reason}. {REPORT}"),
            }],
        };
        if let Err(e) = self.record(ask) {
            tracing::warn!("no last report from the sub-agent: {e}");
            return false;
        }
        if let Err(e) = self.fit_context().await {
            tracing::warn!("no last report from the sub-agent: {e}");
            return false;
        }
        out.turns += 1;
        let resp = match self.request(out.turns).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("no last report from the sub-agent: {e}");
                return false;
            }
        };
        if let Err(e) = self.record_assistant(&resp) {
            tracing::warn!("the sub-agent's last report isn't recorded: {e}");
        }
        if resp.text.trim().is_empty() {
            return false;
        }
        out.text = resp.text;
        true
    }

    /// `<session>.agents/NN-<name>.jsonl`: next to the parent's session,
    /// in a directory, so /resume and --continue don't list it.
    fn subagent_path(&self, name: &str) -> PathBuf {
        let p = self.session.path().to_string_lossy();
        let dir = PathBuf::from(format!("{}.agents", p.trim_end_matches(".jsonl")));
        let n = std::fs::read_dir(&dir)
            .map(|d| {
                d.flatten()
                    .filter(|e| {
                        let f = e.file_name().to_string_lossy().into_owned();
                        f.ends_with(".jsonl") && !f.ends_with(".wire.jsonl")
                    })
                    .count()
            })
            .unwrap_or(0);
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        dir.join(format!("{:02}-{safe}.jsonl", n + 1))
    }
}

/// The first `max` characters, and a note when there were more.
fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}\n[answer cut at {max} characters]")
}
