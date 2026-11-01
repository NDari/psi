//! The OpenAI-compatible client, first for llama-server (psi-plan.md §4.1-4.2).

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::sse::SseDecoder;
use crate::stream::{Accumulator, ChunkError};
use crate::types::{ChatRequest, Delta, Response};
use crate::wire::{WireKind, WireLog};

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("cannot reach the model server at {url}: {message}")]
    Connect { url: String, message: String },
    /// The request is larger than the server's context window.
    #[error("the request ({n_prompt_tokens} tokens) exceeds the context window ({n_ctx} tokens)")]
    ContextOverflow { n_prompt_tokens: u64, n_ctx: u64 },
    /// The chat template rejected the conversation. Deterministic: never retry.
    #[error("the server's chat template failed: {0}")]
    Template(String),
    #[error(
        "the server's chat template does not support tool calls; start llama-server with --jinja"
    )]
    NoToolSupport,
    #[error("the model server returned {status}: {message}")]
    Http { status: u16, message: String },
    #[error("the response stream failed: {0}")]
    Stream(String),
    #[error("aborted")]
    Aborted,
}

/// What llama-server reports at `/props`.
#[derive(Debug, Clone, PartialEq)]
pub struct Props {
    pub n_ctx: Option<u64>,
    pub supports_tool_calls: Option<bool>,
    pub model_path: Option<String>,
    pub build_info: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Client {
    /// The server root, without `/v1`.
    root: String,
    http: reqwest::Client,
    wire: Option<Arc<WireLog>>,
}

impl Client {
    /// `base_url` may end in `/v1` (as in models.jsonc) or be the root.
    pub fn new(base_url: &str) -> Self {
        let root = base_url.trim_end_matches('/');
        let root = root.strip_suffix("/v1").unwrap_or(root).to_owned();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            root,
            http,
            wire: None,
        }
    }

    /// Headers sent with every request (an API key from
    /// `psi.register_provider`). Invalid names or values are skipped.
    pub fn with_headers(mut self, headers: &[(String, String)]) -> Self {
        let mut map = reqwest::header::HeaderMap::new();
        for (k, v) in headers {
            if let (Ok(k), Ok(mut v)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                v.set_sensitive(true);
                map.insert(k, v);
            }
        }
        if let Ok(http) = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .default_headers(map)
            .build()
        {
            self.http = http;
        }
        self
    }

    pub fn with_wire(mut self, wire: Arc<WireLog>) -> Self {
        self.wire = Some(wire);
        self
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    fn connect_err(&self, e: reqwest::Error) -> AiError {
        AiError::Connect {
            url: self.root.clone(),
            message: e.to_string(),
        }
    }

    /// Reads `/props`. Servers other than llama-server may not have it.
    pub async fn props(&self) -> Result<Props, AiError> {
        let url = format!("{}/props", self.root);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| self.connect_err(e))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| self.connect_err(e))?;
        if status != 200 {
            return Err(AiError::Http {
                status,
                message: error_message(&body),
            });
        }
        let v: Value = serde_json::from_str(&body).map_err(|e| AiError::Http {
            status,
            message: format!("/props is not JSON: {e}"),
        })?;
        Ok(Props {
            n_ctx: v["default_generation_settings"]["n_ctx"].as_u64(),
            supports_tool_calls: v["chat_template_caps"]["supports_tool_calls"].as_bool(),
            model_path: v["model_path"].as_str().map(str::to_owned),
            build_info: v["build_info"].as_str().map(str::to_owned),
        })
    }

    /// Counts tokens with the server's tokenizer (D-20).
    pub async fn tokenize(&self, text: &str) -> Result<u64, AiError> {
        let url = format!("{}/tokenize", self.root);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({"content": text, "add_special": false}))
            .send()
            .await
            .map_err(|e| self.connect_err(e))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| self.connect_err(e))?;
        if status != 200 {
            return Err(AiError::Http {
                status,
                message: error_message(&body),
            });
        }
        let v: Value = serde_json::from_str(&body).unwrap_or_default();
        Ok(v["tokens"].as_array().map_or(0, |t| t.len() as u64))
    }

    /// Sends one chat request and streams the answer. `on_delta` sees text
    /// and tool-call names as they arrive.
    pub async fn chat(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(Delta) + Send),
    ) -> Result<Response, AiError> {
        self.chat_bytes(req.to_bytes(), cancel, on_delta).await
    }

    /// Sends a chat request body as is (for replay, which resends the
    /// exact bytes of a recorded request).
    pub async fn chat_bytes(
        &self,
        body: Vec<u8>,
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(Delta) + Send),
    ) -> Result<Response, AiError> {
        let url = format!("{}/v1/chat/completions", self.root);
        let seq = self.wire.as_ref().map(|w| {
            let seq = w.next_seq();
            w.write(seq, WireKind::Request, &url, None, &body);
            seq
        });
        let log_response = |status: u16, raw: &[u8]| {
            if let (Some(w), Some(seq)) = (&self.wire, seq) {
                w.write(seq, WireKind::Response, &url, Some(status), raw);
            }
        };
        let start = Instant::now();
        let send = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .body(body)
            .send();
        let resp = tokio::select! {
            r = send => r.map_err(|e| self.connect_err(e))?,
            () = cancel.cancelled() => return Err(AiError::Aborted),
        };
        let status = resp.status().as_u16();
        if status != 200 {
            let raw = resp.bytes().await.map_err(|e| self.connect_err(e))?;
            log_response(status, &raw);
            return Err(http_error(status, &String::from_utf8_lossy(&raw)));
        }

        let mut raw = Vec::new();
        let mut decoder = SseDecoder::default();
        let mut acc = Accumulator::default();
        let mut first_token_ms = None;
        let mut stream = resp.bytes_stream();
        let mut feed = |events: Vec<String>,
                        acc: &mut Accumulator,
                        first: &mut Option<u64>|
         -> Result<(), AiError> {
            for data in events {
                match acc.apply(&data) {
                    Ok(deltas) => {
                        for d in deltas {
                            if first.is_none() {
                                *first = Some(start.elapsed().as_millis() as u64);
                            }
                            on_delta(d);
                        }
                    }
                    Err(ChunkError::Server(m)) => return Err(AiError::Stream(m)),
                    Err(ChunkError::NotJson(m)) => return Err(AiError::Stream(m)),
                }
            }
            Ok(())
        };
        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                () = cancel.cancelled() => {
                    log_response(status, &raw);
                    return Err(AiError::Aborted);
                }
            };
            match next {
                Some(Ok(bytes)) => {
                    raw.extend_from_slice(&bytes);
                    let events = decoder.push(&bytes);
                    if let Err(e) = feed(events, &mut acc, &mut first_token_ms) {
                        log_response(status, &raw);
                        return Err(e);
                    }
                    if acc.done {
                        break;
                    }
                }
                Some(Err(e)) => {
                    log_response(status, &raw);
                    return Err(AiError::Stream(e.to_string()));
                }
                None => break,
            }
        }
        let rest = decoder.finish();
        let fed = feed(rest, &mut acc, &mut first_token_ms);
        log_response(status, &raw);
        fed?;
        Ok(acc.finish(first_token_ms))
    }
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| body.trim().to_owned())
}

/// Maps llama-server's error responses (recorded in tests/fixtures).
fn http_error(status: u16, body: &str) -> AiError {
    let v: Value = serde_json::from_str(body).unwrap_or_default();
    let err = &v["error"];
    if err["type"] == "exceed_context_size_error" {
        return AiError::ContextOverflow {
            n_prompt_tokens: err["n_prompt_tokens"].as_u64().unwrap_or(0),
            n_ctx: err["n_ctx"].as_u64().unwrap_or(0),
        };
    }
    let message = error_message(body);
    if status == 500 && (message.contains("Jinja") || message.contains("template")) {
        return AiError::Template(message);
    }
    AiError::Http { status, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_strips_v1() {
        assert_eq!(Client::new("http://h:1/v1/").root(), "http://h:1");
        assert_eq!(Client::new("http://h:1").root(), "http://h:1");
    }
}
