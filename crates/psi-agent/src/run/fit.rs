//! Keeping the context in the window before each request (psi-plan.md
//! §7.4): clearing old tool results first, then a summary of everything
//! before a verbatim tail. The decisions themselves are in `crate::compact`.

use std::time::Instant;

use psi_ai::{AiError, ChatMessage, ChatRequest, Delta, RawToolCall, Response};
use psi_ext::Gate;
use psi_session::EntryBody;
use psi_tools::ToolOutput;
use serde_json::json;

use super::{Agent, Planned};
use crate::compact::{
    CHARS_PER_TOKEN, Chain, SUMMARIZER_SYSTEM, check_summary, estimate_tokens, flatten,
    merge_prompt, plain_summary_prompt, plan_clear, plan_cut, request_before, step_budget,
    summary_chain, summary_instruction, summary_words, tail_tokens, trigger,
};
use crate::events::AgentEvent;
use crate::messages::build_with_sources;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Compacted {
    Done,
    /// The kept part already holds everything since the last cut.
    Nothing,
    Blocked(String),
    Failed,
    Aborted,
}

/// What the summary instruction adds to a request, in tokens (generous).
const INSTRUCTION_TOKENS: u64 = 400;

/// The size clearing brings the context down to: 30% of the window, or a
/// fifth of the window below the trigger when that is lower. Each clearing
/// makes the server read again everything after the first cleared result,
/// nearly the whole context, however much it frees; so it frees a lot, and
/// the next one comes later (2026-11-03: down to a fifth below the trigger,
/// a 64K review cleared 4 times in 30 requests and spent 114 s re-reading).
pub(crate) fn clear_target(n_ctx: u64, trigger: u64) -> u64 {
    (n_ctx * 3 / 10).min(trigger.saturating_sub(n_ctx / 5))
}

/// Whether clearing `freed` tokens of a `tokens`-token context is enough:
/// down to [`clear_target`], or to within a twentieth of the window of what
/// a summary would leave (`after_summary`). A summary can't go below its
/// tail, so at 64K the target is out of its reach too, and Psi summarized
/// every time, though clearing alone would have done as well, without the
/// summary call and with the conversation kept word for word (the cog
/// trial, 2026-11-03: summaries left 22K to 28K, clearing would have left
/// about 28K).
pub(crate) fn clearing_will_do(
    tokens: u64,
    freed: u64,
    need: u64,
    after_summary: u64,
    n_ctx: u64,
) -> bool {
    freed >= need || tokens.saturating_sub(freed) <= after_summary + n_ctx / 20
}

impl Agent {
    /// Past the trigger, clear old tool results if that frees enough;
    /// otherwise summarize, and clear in what is left if still needed.
    /// Below it, clear only where the cache breaks anyway. With clearing
    /// off (the default), past the trigger it summarizes, and clears only
    /// when the summary can't make room. Returns whether it summarized.
    pub(super) async fn fit_context(&mut self) -> Result<bool, String> {
        let clear = self.compaction.clear;
        let (tokens, at) = self.context_size();
        if tokens < at {
            if clear {
                self.clear_at_cache_break()?;
            }
            return Ok(false);
        }
        let clearing = self.plan_clearing(tokens, at);
        if clear
            && let Some((ids, freed)) = &clearing
            && clearing_will_do(
                tokens,
                *freed,
                self.room_needed(tokens, at),
                self.after_summary(),
                self.cfg.n_ctx,
            )
        {
            return self
                .record_clearing(ids.clone(), tokens, *freed)
                .map(|()| false);
        }
        let n_ctx = self.cfg.n_ctx;
        let retry = self
            .compact_failed_at
            .is_none_or(|t| tokens >= t + n_ctx / 10);
        if retry && self.compact(tokens, None).await? == Compacted::Done {
            let (tokens, at) = self.context_size();
            if tokens >= at
                && let Some((ids, freed)) = self.plan_clearing(tokens, at)
            {
                self.record_clearing(ids, tokens, freed)?;
            }
            // The summary replaced the start of the context: the server
            // reads what follows it again, so clearing there is free.
            if clear {
                self.clear_at_cache_break()?;
            }
            return Ok(true);
        }
        match clearing {
            Some((ids, freed)) => self.record_clearing(ids, tokens, freed).map(|()| false),
            None => Ok(false),
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

    /// About what a summary would leave, in tokens: the system prompt,
    /// tools and summaries so far, a new summary at its full length (about
    /// 7 characters a word) and the tail.
    fn after_summary(&self) -> u64 {
        let chain = summary_chain(&self.session.branch());
        let overhead = self.overhead(&chain);
        let n_ctx = self.cfg.n_ctx;
        let tail = tail_tokens(self.trigger(), overhead, self.compaction.tail);
        overhead + summary_words(n_ctx) * 7 / CHARS_PER_TOKEN + tail
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
        let overhead = self.overhead(&summary_chain(&self.session.branch()));
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
        let scout = if self.offers_agents() {
            " To go through many files, give the reading to scout with the agent tool."
        } else {
            ""
        };
        let note = format!(
            "Not run: the results before this one take about {used} tokens, as much as one step can keep in your {}-token context; more would be cleared before you read them. Use those first, then make this call again, or read only the part you need.{scout}",
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

    /// How much to free: down to [`clear_target`].
    fn room_needed(&self, tokens: u64, at: u64) -> u64 {
        tokens.saturating_sub(clear_target(self.cfg.n_ctx, at))
    }

    fn plan_clearing(&self, tokens: u64, at: u64) -> Option<(Vec<String>, u64)> {
        let branch = self.session.branch();
        let (messages, sources) = build_with_sources(&self.system_prompt(), &branch);
        plan_clear(
            &messages,
            &sources,
            self.room_needed(tokens, at),
            self.cfg.n_ctx / 10,
            0,
        )
    }

    /// When the next request breaks the server's prompt cache anyway (after
    /// a summary; a changed system prompt or tool list: `/reload`, an MCP
    /// server joining, plan mode), old results after the break are cleared
    /// down to [`clear_target`] even below the trigger: the server reads
    /// them again either way, so the clearing costs nothing, and the next
    /// clearing or summary, which does cost a re-read, comes later.
    fn clear_at_cache_break(&mut self) -> Result<(), String> {
        let Some(last) = &self.last_request else {
            return Ok(());
        };
        let next = self.build_request();
        let Some(from) = cache_break(last, &next) else {
            return Ok(());
        };
        let (tokens, at) = self.context_size();
        let need = self.room_needed(tokens, at);
        if need == 0 {
            return Ok(());
        }
        let branch = self.session.branch();
        let (messages, sources) = build_with_sources(&self.system_prompt(), &branch);
        let planned = plan_clear(&messages, &sources, need, self.cfg.n_ctx / 20, from);
        drop(branch);
        let Some((ids, freed)) = planned else {
            return Ok(());
        };
        self.record_clearing(ids, tokens, freed)?;
        self.record(EntryBody::Custom {
            custom_type: "clearing".into(),
            data: json!({"reason": "cache_break", "from_message": from}),
        })
        .map(|_| ())
    }

    fn record_clearing(&mut self, ids: Vec<String>, tokens: u64, freed: u64) -> Result<(), String> {
        self.record(EntryBody::ClearedResults {
            entry_ids: ids,
            tokens_before: tokens,
            tokens_freed: freed,
        })
        .map(|_| ())
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
            Compacted::Blocked(why) => format!("an extension stopped compaction: {why}"),
            Compacted::Failed => "compaction failed; the conversation is unchanged".into(),
            Compacted::Aborted => "compaction stopped".into(),
        })
    }

    /// Summarizes the entries since the last cut, up to a verbatim tail,
    /// and adds the summary to the chain (D-40). First by extending the
    /// last request, so the server reuses its prefix cache (D-38), then
    /// from the entries as text.
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
        let gate = self
            .ext
            .gate(
                "session_before_compact",
                json!({"tokens": tokens, "reason": "summary"}),
            )
            .await;
        if let Gate::Block { reason: why, .. } = gate {
            tracing::info!("an extension stopped compaction: {why}");
            return Ok(Compacted::Blocked(why));
        }
        let n_ctx = self.cfg.n_ctx;
        let branch = self.session.branch();
        let chain = summary_chain(&branch);
        let from = chain.keep_from;
        let overhead = self.overhead(&chain);
        let set = self.compaction;
        let tail = tail_tokens(self.trigger(), overhead, set.tail);
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
        // The fallback sees the earlier summaries when they take at most
        // half its room, so it can correct them as the extended request can.
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
                summary_instruction(after_summary, words, set.summary) + &focus_text,
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
                plain_summary_prompt(&text, request.as_deref(), &earlier, words, set.summary)
                    + &focus_text,
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
        Ok(Compacted::Done)
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
        let prompt = merge_prompt(a, b, summary_words(self.cfg.n_ctx), self.compaction.summary);
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

/// Where `new` stops matching `last`, so the server reads it again from
/// there: the first message that differs, or 0 when the tools, model or
/// thinking setting differ (the template puts tools before the messages).
/// `None` when `new` sends everything `last` did, as a request in a run
/// usually does.
fn cache_break(last: &ChatRequest, new: &ChatRequest) -> Option<usize> {
    if last.tools != new.tools
        || last.enable_thinking != new.enable_thinking
        || last.model != new.model
    {
        return Some(0);
    }
    let same = last
        .messages
        .iter()
        .zip(&new.messages)
        .take_while(|(a, b)| a == b)
        .count();
    (same < last.messages.len()).then_some(same)
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

#[cfg(test)]
mod tests {
    use super::{cache_break, clear_target, clearing_will_do};
    use crate::compact::trigger;
    use psi_ai::{ChatMessage, ChatRequest};

    fn req(messages: &[&str], tools: usize) -> ChatRequest {
        ChatRequest {
            model: "local".into(),
            messages: messages
                .iter()
                .map(|m| ChatMessage::User((*m).into()))
                .collect(),
            tools: (0..tools).map(|i| serde_json::json!({"n": i})).collect(),
            max_tokens: 512,
            enable_thinking: None,
        }
    }

    #[test]
    fn a_request_breaks_the_cache_where_it_stops_matching_the_last() {
        let last = req(&["sys", "a", "b"], 2);
        // A request in a run sends everything the last one did, and more.
        assert_eq!(cache_break(&last, &req(&["sys", "a", "b", "c"], 2)), None);
        // A summary replaced what followed the system prompt.
        assert_eq!(
            cache_break(&last, &req(&["sys", "summary", "c"], 2)),
            Some(1)
        );
        // A changed system prompt or tool list: from the start.
        assert_eq!(cache_break(&last, &req(&["sys2", "a", "b"], 2)), Some(0));
        assert_eq!(cache_break(&last, &req(&["sys", "a", "b"], 1)), Some(0));
    }

    #[test]
    fn clearing_does_when_it_matches_a_summary() {
        let n = 65_536;
        let at = trigger(n, 8192, 9830);
        let need = 45_900 - clear_target(n, at);
        // The cog trial's first trigger: 18K clearable of 26K needed, and a
        // summary would leave about 25.7K: clearing to 27.9K is as good.
        assert!(clearing_will_do(45_900, 18_000, need, 25_700, n));
        // Clearing that leaves much more than a summary is not.
        assert!(!clearing_will_do(45_900, 8_000, need, 25_700, n));
        // Reaching the target is always enough.
        assert!(clearing_will_do(45_900, need, need, 0, n));
    }

    #[test]
    fn clearing_frees_room_for_many_steps() {
        // 64K: the trigger is at 45,875; clearing goes down to about 19.7K,
        // leaving about 26K before the next one (13K before).
        let at = trigger(65_536, 8192, 9830);
        assert_eq!(clear_target(65_536, at), 19_660);
        // 16K: 30% of the window, a little below the old 6,555.
        let at = trigger(16_384, 4096, 2457);
        assert_eq!(clear_target(16_384, at), 4915);
        // Tiny windows: never above a fifth below the trigger, as before.
        let at = trigger(8192, 8192, 1000);
        assert_eq!(clear_target(8192, at), 2048 - 1638);
    }
}
