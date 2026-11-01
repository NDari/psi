//! Keeping the context in the window before each request (psi-plan.md
//! §7.4): clearing old tool results first, then a summary of everything
//! before a verbatim tail. The decisions themselves are in `crate::compact`.

use std::time::Instant;

use psi_ai::{AiError, ChatMessage, ChatRequest, Response};
use psi_ext::Gate;
use psi_session::EntryBody;
use serde_json::json;

use super::Agent;
use crate::compact::{
    CHARS_PER_TOKEN, SUMMARIZER_SYSTEM, check_summary, estimate_tokens, flatten, merge_prompt,
    plain_summary_prompt, plan_clear, plan_cut, summary_chain, summary_instruction, summary_words,
    tail_tokens, trigger,
};
use crate::events::AgentEvent;
use crate::messages::build_with_sources;

/// What the summary instruction adds to a request, in tokens (generous).
const INSTRUCTION_TOKENS: u64 = 400;

impl Agent {
    /// Past the trigger, clear old tool results if that frees enough;
    /// otherwise summarize, and clear in what is left if still needed.
    pub(super) async fn fit_context(&mut self) -> Result<(), String> {
        let (tokens, at) = self.context_size();
        if tokens < at {
            return Ok(());
        }
        let clearing = self.plan_clearing(tokens, at);
        if let Some((ids, freed)) = &clearing
            && *freed >= self.room_needed(tokens, at)
        {
            return self.record_clearing(ids.clone(), tokens, *freed);
        }
        let n_ctx = self.cfg.n_ctx;
        let retry = self
            .compact_failed_at
            .is_none_or(|t| tokens >= t + n_ctx / 10);
        if retry && self.compact(tokens).await? {
            let (tokens, at) = self.context_size();
            if tokens >= at
                && let Some((ids, freed)) = self.plan_clearing(tokens, at)
            {
                self.record_clearing(ids, tokens, freed)?;
            }
            return Ok(());
        }
        match clearing {
            Some((ids, freed)) => self.record_clearing(ids, tokens, freed),
            None => Ok(()),
        }
    }

    /// The estimated size of the next request, and the trigger.
    fn context_size(&self) -> (u64, u64) {
        let branch = self.session.branch();
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

    /// How much to free: down to a fifth of the window below the trigger.
    fn room_needed(&self, tokens: u64, at: u64) -> u64 {
        tokens - at.saturating_sub(self.cfg.n_ctx / 5)
    }

    fn plan_clearing(&self, tokens: u64, at: u64) -> Option<(Vec<String>, u64)> {
        let branch = self.session.branch();
        let (messages, sources) = build_with_sources(&self.system_prompt(), &branch);
        plan_clear(
            &messages,
            &sources,
            self.room_needed(tokens, at),
            self.cfg.n_ctx / 10,
        )
    }

    fn record_clearing(&mut self, ids: Vec<String>, tokens: u64, freed: u64) -> Result<(), String> {
        self.record(EntryBody::ClearedResults {
            entry_ids: ids,
            tokens_before: tokens,
            tokens_freed: freed,
        })
        .map(|_| ())
    }

    /// Summarizes the entries since the last cut, up to a verbatim tail,
    /// and adds the summary to the chain (D-40). First by extending the
    /// last request, so the server reuses its prefix cache (D-38), then
    /// from the entries as text. Returns whether it compacted.
    pub(super) async fn compact(&mut self, tokens: u64) -> Result<bool, String> {
        let gate = self
            .ext
            .gate(
                "session_before_compact",
                json!({"tokens": tokens, "reason": "summary"}),
            )
            .await;
        if let Gate::Block(why) = gate {
            tracing::info!("an extension stopped compaction: {why}");
            return Ok(false);
        }
        let n_ctx = self.cfg.n_ctx;
        let branch = self.session.branch();
        let chain = summary_chain(&branch);
        let from = chain.keep_from;
        let summaries_chars: usize = chain.summaries.iter().map(|(_, s)| s.len()).sum();
        let overhead = (self.system_prompt().len() + self.tools_chars() + summaries_chars) as u64
            / CHARS_PER_TOKEN;
        let tail = tail_tokens(n_ctx, self.trigger(), overhead);
        let Some(cut) = plan_cut(&branch, from, tail) else {
            return Ok(false);
        };
        let first_kept_id = branch[cut].id.clone();
        let after_summary = !chain.summaries.is_empty();
        let words = summary_words(n_ctx);
        let max_chars = n_ctx.saturating_sub(self.cfg.max_output + 1500) * CHARS_PER_TOKEN;
        let text = flatten(&branch[from..cut], max_chars as usize);
        drop(branch);

        self.emit(AgentEvent::Compacting { tokens });
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
            r.messages
                .push(ChatMessage::User(summary_instruction(after_summary, words)));
            match self.summary_request(&r).await {
                Ok(resp) => match check_summary(&resp) {
                    Ok(s) => done = Some(("extend", s, resp)),
                    Err(e) => fallback = Some(e),
                },
                Err(AiError::Aborted) => return Ok(false),
                Err(e) => fallback = Some(e.to_string()),
            }
        }
        if done.is_none() {
            let r = self.plain_request(plain_summary_prompt(&text, words));
            let failed = match self.summary_request(&r).await {
                Ok(resp) => match check_summary(&resp) {
                    Ok(s) => {
                        done = Some(("plain", s, resp));
                        None
                    }
                    Err(e) => Some(e),
                },
                Err(AiError::Aborted) => return Ok(false),
                Err(e) => Some(e.to_string()),
            };
            if let Some(e) = failed {
                self.compact_failed_at = Some(tokens);
                self.record(EntryBody::GuardEvent {
                    guard: "compaction_failed".into(),
                    message: format!("could not summarize the earlier conversation: {e}"),
                    tool_call_id: None,
                })?;
                return Ok(false);
            }
        }
        let Some((method, summary, resp)) = done else {
            return Ok(false);
        };
        self.compact_failed_at = None;
        self.record(EntryBody::Compaction {
            summary,
            first_kept_id,
            tokens_before: Some(tokens),
            replaces: vec![],
        })?;
        self.record(EntryBody::Custom {
            custom_type: "compaction".into(),
            data: json!({"method": method, "fallback_reason": fallback,
                         "usage": resp.usage, "timings": resp.timings,
                         "ms": started.elapsed().as_millis() as u64}),
        })?;
        self.merge_summaries().await?;
        Ok(true)
    }

    /// When the summaries together pass a tenth of the window, the two
    /// oldest are merged into one: the only place a summary is summarized
    /// again (D-40).
    async fn merge_summaries(&mut self) -> Result<(), String> {
        let branch = self.session.branch();
        let chain = summary_chain(&branch);
        let chars: usize = chain.summaries.iter().map(|(_, s)| s.len()).sum();
        if chain.summaries.len() < 2 || chars as u64 / CHARS_PER_TOKEN <= self.cfg.n_ctx / 10 {
            return Ok(());
        }
        let ((a_id, a), (b_id, b)) = (chain.summaries[0], chain.summaries[1]);
        let replaces = vec![a_id.to_owned(), b_id.to_owned()];
        let first_kept_id = branch[chain.keep_from].id.clone();
        let prompt = merge_prompt(a, b, summary_words(self.cfg.n_ctx));
        drop(branch);
        let started = Instant::now();
        let r = self.plain_request(prompt);
        let merged = match self.summary_request(&r).await {
            Ok(resp) => check_summary(&resp).map(|s| (s, resp)),
            Err(AiError::Aborted) => return Ok(()),
            Err(e) => Err(e.to_string()),
        };
        match merged {
            Ok((summary, resp)) => {
                self.record(EntryBody::Compaction {
                    summary,
                    first_kept_id,
                    tokens_before: None,
                    replaces,
                })?;
                self.record(EntryBody::Custom {
                    custom_type: "compaction".into(),
                    data: json!({"method": "merge", "usage": resp.usage,
                                 "timings": resp.timings,
                                 "ms": started.elapsed().as_millis() as u64}),
                })?;
            }
            Err(e) => {
                self.record(EntryBody::GuardEvent {
                    guard: "compaction_failed".into(),
                    message: format!("could not merge the two oldest summaries: {e}"),
                    tool_call_id: None,
                })?;
            }
        }
        Ok(())
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

    /// Sends a summary request; its text is not streamed anywhere.
    async fn summary_request(&self, req: &ChatRequest) -> Result<Response, AiError> {
        self.client.chat(req, &self.ctx.cancel, &mut |_| {}).await
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
