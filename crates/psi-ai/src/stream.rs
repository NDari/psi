//! Accumulates streamed chat chunks into a [`Response`].

use serde_json::Value;

use crate::types::{Delta, FinishReason, RawToolCall, Response, Timings, TokenUsage};

#[derive(Debug, Default)]
pub(crate) struct Accumulator {
    text: String,
    reasoning: String,
    calls: Vec<RawToolCall>,
    finish: Option<FinishReason>,
    usage: Option<TokenUsage>,
    timings: Option<Timings>,
    pub done: bool,
}

/// What one chunk held besides deltas.
pub(crate) enum ChunkError {
    /// The server sent `{"error": ...}` in the stream.
    Server(String),
    NotJson(String),
}

impl Accumulator {
    /// Applies one SSE data payload and returns its deltas.
    pub fn apply(&mut self, data: &str) -> Result<Vec<Delta>, ChunkError> {
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(Vec::new());
        }
        let v: Value =
            serde_json::from_str(data).map_err(|e| ChunkError::NotJson(format!("{e}: {data}")))?;
        if let Some(err) = v.get("error") {
            let msg = err["message"].as_str().map(str::to_owned);
            return Err(ChunkError::Server(msg.unwrap_or_else(|| err.to_string())));
        }
        let mut out = Vec::new();
        if let Some(choice) = v["choices"].get(0) {
            let d = &choice["delta"];
            if let Some(t) = d["content"].as_str().filter(|t| !t.is_empty()) {
                self.text.push_str(t);
                out.push(Delta::Text(t.to_owned()));
            }
            if let Some(t) = d["reasoning_content"].as_str().filter(|t| !t.is_empty()) {
                self.reasoning.push_str(t);
                out.push(Delta::Reasoning(t.to_owned()));
            }
            for tc in d["tool_calls"].as_array().into_iter().flatten() {
                let index = tc["index"].as_u64().unwrap_or(self.calls.len() as u64) as usize;
                while self.calls.len() <= index {
                    self.calls.push(RawToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }
                let call = &mut self.calls[index];
                if let Some(id) = tc["id"].as_str() {
                    call.id = id.to_owned();
                }
                if let Some(name) = tc["function"]["name"].as_str() {
                    call.name.push_str(name);
                    out.push(Delta::ToolCallStart {
                        index,
                        name: call.name.clone(),
                    });
                }
                if let Some(args) = tc["function"]["arguments"].as_str() {
                    call.arguments.push_str(args);
                }
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                self.finish = Some(match f {
                    "stop" => FinishReason::Stop,
                    "tool_calls" => FinishReason::ToolCalls,
                    "length" => FinishReason::Length,
                    _ => FinishReason::Unknown,
                });
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
            let cached = u["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0);
            self.usage = Some(TokenUsage {
                input: prompt.saturating_sub(cached),
                output: u["completion_tokens"].as_u64().unwrap_or(0),
                cache_read: cached,
            });
        }
        if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
            self.timings = Some(Timings {
                prompt_ms: t["prompt_ms"].as_f64().unwrap_or(0.0),
                predicted_ms: t["predicted_ms"].as_f64().unwrap_or(0.0),
                predicted_per_second: t["predicted_per_second"].as_f64().unwrap_or(0.0),
            });
        }
        Ok(out)
    }

    pub fn finish(self, first_token_ms: Option<u64>) -> Response {
        let mut calls = self.calls;
        for (i, c) in calls.iter_mut().enumerate() {
            // Some servers omit ids; the agent needs one to pair results.
            if c.id.is_empty() {
                c.id = format!("call_{i}");
            }
        }
        Response {
            text: self.text,
            reasoning: self.reasoning,
            tool_calls: calls,
            finish: self.finish.unwrap_or(FinishReason::Unknown),
            usage: self.usage,
            timings: self.timings,
            first_token_ms,
        }
    }
}
