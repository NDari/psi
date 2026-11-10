//! Keeping the context in the window before each request (psi-plan.md
//! §7.4): one summary of everything before a verbatim tail. The decisions
//! themselves are in `crate::compact`.

use std::time::Instant;

use psi_ai::{AiError, ChatMessage, ChatRequest, Delta, RawToolCall, Response};
use psi_session::EntryBody;
use psi_tools::ToolOutput;
use serde_json::json;

use super::{Agent, Planned};
use crate::compact::{
    CHARS_PER_TOKEN, Chain, SUMMARIZER_SYSTEM, check_summary, estimate_tokens, flatten,
    plain_summary_prompt, plan_cut, request_before, step_budget, summary_chain,
    summary_instruction, summary_words, tail_tokens, trigger,
};
use crate::events::AgentEvent;
use crate::messages::build_with_sources;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Compacted {
    Done,
    /// The kept part already holds everything since the last cut.
    Nothing,
    Failed,
    Aborted,
}

/// What the summary instruction adds to a request, in tokens (generous).
const INSTRUCTION_TOKENS: u64 = 400;

impl Agent {
    /// Whether the next request is past the size at which the earlier
    /// conversation is summarized.
    pub(super) fn past_trigger(&self) -> bool {
        let (tokens, at) = self.context_size();
        tokens >= at
    }

    /// Past the trigger, summarizes the earlier conversation. Returns
    /// whether it did.
    pub(super) async fn fit_context(&mut self) -> Result<bool, String> {
        let (tokens, at) = self.context_size();
        if tokens < at {
            return Ok(false);
        }
        let n_ctx = self.cfg.n_ctx;
        let retry = self
            .compact_failed_at
            .is_none_or(|t| tokens >= t + n_ctx / 10);
        Ok(retry && self.compact(tokens, None).await? == Compacted::Done)
    }

    /// The estimated size of the next request, and the trigger.
    fn context_size(&self) -> (u64, u64) {
        let branch = self.context();
        let (messages, _) = build_with_sources(&self.system_prompt(), &branch);
        let tokens = estimate_tokens(&branch, &messages, self.tools_chars());
        (tokens, self.trigger())
    }

    fn trigger(&self) -> u64 {
        let result_tokens = self.ctx.max_result_chars as u64 / CHARS_PER_TOKEN;
        trigger(self.cfg.n_ctx, self.cfg.max_output, result_tokens)
    }

    fn tools_chars(&self) -> usize {
        let offered = self.tool_names();
        self.all_schemas()
            .iter()
            .filter(|s| offered.iter().any(|n| n == s.name))
            .map(|s| s.to_openai().to_string().len())
            .sum()
    }

    /// The system prompt, tools and summaries, in tokens.
    fn overhead(&self, chain: &Chain) -> u64 {
        let summaries: usize = chain.summaries.iter().map(|(_, s)| s.len()).sum();
        (self.system_prompt().len() + self.tools_chars() + summaries) as u64 / CHARS_PER_TOKEN
    }

    /// Read-only results of one step past what the context can keep are
    /// not given to the model: a note says so and how to go on. Otherwise
    /// they are cleared or summarized before it reads them, and it reads
    /// them again. The first result always stays. Returns the indices of
    /// the calls held back.
    pub(super) fn limit_step(
        &mut self,
        planned: &[(RawToolCall, Planned)],
        outputs: &mut [ToolOutput],
    ) -> Result<Vec<usize>, String> {
        let overhead = self.overhead(&summary_chain(&self.context()));
        let result_tokens = self.ctx.max_result_chars as u64 / CHARS_PER_TOKEN;
        let budget = step_budget(self.cfg.n_ctx, self.trigger(), overhead, result_tokens);
        let mut used = 0;
        let mut held = Vec::new();
        for (i, ((_, p), o)) in planned.iter().zip(outputs.iter()).enumerate() {
            let tokens = o.text.len() as u64 / CHARS_PER_TOKEN;
            let read_only = matches!(p, Ok((c, _)) if c.read_only());
            if i > 0 && read_only && (!held.is_empty() || used + tokens > budget) {
                held.push(i);
            } else {
                used += tokens;
            }
        }
        let Some(&first) = held.first() else {
            return Ok(held);
        };
        let note = format!(
            "Not run: the results before this one take about {used} tokens, as much as one step can keep in your {}-token context; more would be summarized before you read them. Use those first, then make this call again, or read only the part you need.",
            self.cfg.n_ctx
        );
        for &i in &held {
            outputs[i] = ToolOutput::err(note.clone());
        }
        self.record(EntryBody::GuardEvent {
            guard: "step_budget".into(),
            message: format!(
                "{} read-only calls not run: the step's results passed about {budget} tokens",
                held.len()
            ),
            tool_call_id: Some(planned[first].0.id.clone()),
        })?;
        Ok(held)
    }

    /// /compact: summarizes now, whatever the size; `focus` is added to
    /// the instruction. Says what happened.
    pub async fn compact_now(&mut self, focus: Option<&str>) -> Result<String, String> {
        self.renew_cancel();
        let (tokens, _) = self.context_size();
        Ok(match self.compact(tokens, focus).await? {
            Compacted::Done => {
                let (after, _) = self.context_size();
                format!("compacted: about {tokens} tokens before, {after} after")
            }
            Compacted::Nothing => {
                "nothing to compact: the conversation fits in the part kept as is".into()
            }
            Compacted::Failed => "compaction failed; the conversation is unchanged".into(),
            Compacted::Aborted => "compaction stopped".into(),
        })
    }

    /// Summarizes the entries since the last cut, up to a verbatim tail.
    /// The new summary is written with the earlier one in view and takes
    /// its place. First by extending the last request, so the server
    /// reuses its prefix cache (D-38), then from the entries as text.
    pub(super) async fn compact(
        &mut self,
        tokens: u64,
        focus: Option<&str>,
    ) -> Result<Compacted, String> {
        let r = self.write_summary(tokens, focus).await;
        if self.compacting {
            self.compacting = false;
            let done = matches!(r, Ok(Compacted::Done));
            self.emit(AgentEvent::Compacted { done });
        }
        r
    }

    async fn write_summary(
        &mut self,
        tokens: u64,
        focus: Option<&str>,
    ) -> Result<Compacted, String> {
        let n_ctx = self.cfg.n_ctx;
        let branch = self.context();
        let chain = summary_chain(&branch);
        let from = chain.keep_from;
        let overhead = self.overhead(&chain);
        let tail = tail_tokens(self.trigger(), overhead);
        let Some(cut) = plan_cut(&branch, from, tail) else {
            return Ok(Compacted::Nothing);
        };
        let first_kept_id = branch[cut].id.clone();
        let after_summary = !chain.summaries.is_empty();
        let words = summary_words(n_ctx);
        let focus_text = focus
            .filter(|f| !f.trim().is_empty())
            .map(|f| format!("\n\nThe user asks the summary to focus on: {}", f.trim()))
            .unwrap_or_default();
        let max_chars =
            (n_ctx.saturating_sub(self.cfg.max_output + 1500) * CHARS_PER_TOKEN) as usize;
        // The fallback sees the earlier summary when it takes at most half
        // its room (always, but for a chain from an older session).
        let all_ids: Vec<String> = chain
            .summaries
            .iter()
            .map(|(id, _)| (*id).to_owned())
            .collect();
        let earlier_chars: usize = chain.summaries.iter().map(|(_, s)| s.len()).sum();
        let earlier: Vec<String> = if earlier_chars <= max_chars / 2 {
            chain
                .summaries
                .iter()
                .map(|(_, s)| (*s).to_owned())
                .collect()
        } else {
            Vec::new()
        };
        let room = max_chars - earlier.iter().map(String::len).sum::<usize>();
        let text = flatten(&branch[from..cut], room);
        let request = request_before(&branch[..cut], from);
        drop(branch);

        self.emit(AgentEvent::Compacting { tokens });
        self.compacting = true;
        let started = Instant::now();
        let req = self.build_request();
        let extends = self
            .last_request
            .as_ref()
            .is_some_and(|last| extends_request(last, &req));
        let fits = tokens + INSTRUCTION_TOKENS + self.cfg.max_output <= n_ctx;
        let mut fallback = match (extends, fits) {
            (false, _) => Some("the request does not extend the last one".to_owned()),
            (true, false) => Some("no room to extend the last request".to_owned()),
            (true, true) => None,
        };
        let mut done = None;
        if fallback.is_none() {
            let mut r = req;
            r.messages.push(ChatMessage::User(
                summary_instruction(after_summary, words) + &focus_text,
            ));
            match self.summary_request(&r).await {
                Ok(resp) => match check_summary(&resp) {
                    Ok(s) => done = Some(("extend", s, resp)),
                    Err(e) => fallback = Some(e),
                },
                Err(AiError::Aborted) => return Ok(Compacted::Aborted),
                Err(e) => fallback = Some(e.to_string()),
            }
        }
        if done.is_none() {
            let earlier: Vec<&str> = earlier.iter().map(String::as_str).collect();
            let r = self.plain_request(
                plain_summary_prompt(&text, request.as_deref(), &earlier, words) + &focus_text,
            );
            let failed = match self.summary_request(&r).await {
                Ok(resp) => match check_summary(&resp) {
                    Ok(s) => {
                        done = Some(("plain", s, resp));
                        None
                    }
                    Err(e) => Some(e),
                },
                Err(AiError::Aborted) => return Ok(Compacted::Aborted),
                Err(e) => Some(e.to_string()),
            };
            if let Some(e) = failed {
                self.compact_failed_at = Some(tokens);
                self.record(EntryBody::GuardEvent {
                    guard: "compaction_failed".into(),
                    message: format!("could not summarize the earlier conversation: {e}"),
                    tool_call_id: None,
                })?;
                return Ok(Compacted::Failed);
            }
        }
        let Some((method, summary, resp)) = done else {
            return Ok(Compacted::Failed);
        };
        self.compact_failed_at = None;
        // The summary takes the place of the ones it was written with.
        let replaces = if method == "extend" || !earlier.is_empty() {
            all_ids
        } else {
            Vec::new()
        };
        self.record(EntryBody::Compaction {
            summary,
            first_kept_id,
            tokens_before: Some(tokens),
            replaces,
        })?;
        self.record(EntryBody::Custom {
            custom_type: "compaction".into(),
            data: json!({"method": method, "fallback_reason": fallback,
                         "usage": resp.usage, "timings": resp.timings,
                         "ms": started.elapsed().as_millis() as u64}),
        })?;
        Ok(Compacted::Done)
    }

    /// A request with only the summarizer's system prompt and `prompt`.
    fn plain_request(&self, prompt: String) -> ChatRequest {
        ChatRequest {
            model: self.cfg.model.clone(),
            messages: vec![
                ChatMessage::System(SUMMARIZER_SYSTEM.into()),
                ChatMessage::User(prompt),
            ],
            tools: vec![],
            max_tokens: self.cfg.max_output,
            enable_thinking: self.cfg.enable_thinking,
        }
    }

    /// Sends a summary request; its text is not streamed anywhere, but how
    /// far the server has read the prompt is, for the spinner.
    async fn summary_request(&self, req: &ChatRequest) -> Result<Response, AiError> {
        let events = self.events.clone();
        let mut on_delta = move |d: Delta| {
            if let (
                Delta::PromptProgress {
                    total,
                    cache,
                    processed,
                },
                Some(tx),
            ) = (d, &events)
            {
                let _ = tx.send(AgentEvent::PromptProgress {
                    total,
                    cache,
                    processed,
                });
            }
        };
        self.client.chat(req, &self.ctx.cancel, &mut on_delta).await
    }
}

/// Whether `new` sends everything `last` did, in the same order, so the
/// server's prefix cache holds for all of it (D-38).
fn extends_request(last: &ChatRequest, new: &ChatRequest) -> bool {
    last.tools == new.tools
        && last.enable_thinking == new.enable_thinking
        && last.model == new.model
        && new.messages.len() >= last.messages.len()
        && new.messages[..last.messages.len()] == last.messages[..]
}
