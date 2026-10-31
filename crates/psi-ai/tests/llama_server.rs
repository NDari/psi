#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.

//! The client against llama-server responses recorded from both reference
//! models (tests/fixtures/llama-server).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_ai::wire::{WireKind, read_wire};
use psi_ai::{AiError, ChatMessage, ChatRequest, Client, Delta, FinishReason, WireLog};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const MODELS: [&str; 2] = ["qwen3.5-4b", "gemma4-12b-qat"];

fn dir(model: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/llama-server")
        .join(model)
}

/// Replays one recorded response to its recorded request.
async fn replay(model: &str, name: &str) -> (Result<psi_ai::Response, AiError>, Vec<Delta>) {
    let d = dir(model);
    let server = ReplayServer::start(vec![
        Reply::from_http_file(&d.join(format!("{name}.response.http"))).unwrap(),
    ])
    .unwrap();
    let req: Value =
        serde_json::from_slice(&std::fs::read(d.join(format!("{name}.request.json"))).unwrap())
            .unwrap();
    let messages = req["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| match m["role"].as_str().unwrap() {
            "system" => ChatMessage::System(m["content"].as_str().unwrap().into()),
            _ => ChatMessage::User(m["content"].as_str().unwrap_or_default().into()),
        })
        .collect();
    let request = ChatRequest {
        model: "local".into(),
        messages,
        tools: vec![],
        max_tokens: 64,
        enable_thinking: None,
    };
    let client = Client::new(&server.base_url());
    let mut deltas = Vec::new();
    let r = client
        .chat(&request, &CancellationToken::new(), &mut |d| deltas.push(d))
        .await;
    (r, deltas)
}

#[tokio::test]
async fn plain_answers_stream_text_and_usage() {
    for m in MODELS {
        let (r, deltas) = replay(m, "plain").await;
        let r = r.unwrap();
        assert_eq!(r.text.trim(), "hello there", "{m}");
        assert_eq!(r.finish, FinishReason::Stop, "{m}");
        assert!(r.usage.unwrap().output > 0, "{m}");
        assert!(r.timings.is_some(), "{m}");
        assert!(r.first_token_ms.is_some(), "{m}");
        assert!(deltas.iter().all(|d| matches!(d, Delta::Text(_))), "{m}");
    }
}

#[tokio::test]
async fn streamed_tool_calls_assemble() {
    let (r, _) = replay("qwen3.5-4b", "tool_calls").await;
    let r = r.unwrap();
    assert_eq!(r.finish, FinishReason::ToolCalls);
    let paths: Vec<Value> = r
        .tool_calls
        .iter()
        .map(|c| {
            assert_eq!(c.name, "read");
            assert!(!c.id.is_empty());
            serde_json::from_str::<Value>(&c.arguments).unwrap()["path"].clone()
        })
        .collect();
    assert_eq!(paths, [json!("a.py"), json!("b.py")]);

    let (r, _) = replay("gemma4-12b-qat", "tool_calls").await;
    let r = r.unwrap();
    assert_eq!(r.tool_calls.len(), 1);
    let args: Value = serde_json::from_str(&r.tool_calls[0].arguments).unwrap();
    assert_eq!(args["path"], "a.py");
}

#[tokio::test]
async fn output_limit_is_reported_even_mid_tool_call() {
    for m in MODELS {
        let (r, _) = replay(m, "length_text").await;
        assert_eq!(r.unwrap().finish, FinishReason::Length, "{m}");
        let (r, _) = replay(m, "length_tool_call").await;
        let r = r.unwrap();
        assert_eq!(r.finish, FinishReason::Length, "{m}");
        assert_eq!(r.tool_calls.len(), 1, "{m}");
        assert!(
            serde_json::from_str::<Value>(&r.tool_calls[0].arguments).is_err(),
            "{m}: args should be cut off"
        );
    }
}

#[tokio::test]
async fn context_overflow_is_typed() {
    for m in MODELS {
        let (r, _) = replay(m, "overflow").await;
        match r {
            Err(AiError::ContextOverflow {
                n_prompt_tokens,
                n_ctx,
            }) => {
                assert!(n_prompt_tokens > n_ctx, "{m}");
                assert_eq!(n_ctx, 8192, "{m}");
            }
            other => panic!("{m}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn template_errors_are_typed() {
    let (r, _) = replay("qwen3.5-4b", "system_not_first").await;
    match r {
        Err(AiError::Template(msg)) => assert!(
            msg.contains("System message must be at the beginning"),
            "{msg}"
        ),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn answer_after_tool_result() {
    for m in MODELS {
        let (r, _) = replay(m, "after_tools").await;
        let r = r.unwrap();
        assert_eq!(r.finish, FinishReason::Stop, "{m}");
        assert!(r.text.contains("hi"), "{m}: {}", r.text);
    }
}

#[tokio::test]
async fn props_from_recordings() {
    for m in MODELS {
        let raw = std::fs::read_to_string(dir(m).join("props.response.http")).unwrap();
        let body = raw.split_once("\r\n\r\n").unwrap().1;
        let server = ReplayServer::start(vec![]).unwrap();
        server.set_props(serde_json::from_str(body).unwrap());
        let p = Client::new(&server.base_url()).props().await.unwrap();
        assert_eq!(p.n_ctx, Some(8192), "{m}");
        assert_eq!(p.supports_tool_calls, Some(true), "{m}");
        assert!(p.model_path.unwrap().ends_with(".gguf"), "{m}");
    }
}

#[tokio::test]
async fn wire_log_keeps_exact_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let wire = Arc::new(WireLog::open(&tmp.path().join("w.jsonl")).unwrap());
    let server = ReplayServer::start(vec![Reply::Chat(ScriptedReply::call(
        "read",
        json!({"path": "x"}),
    ))])
    .unwrap();
    let client = Client::new(&server.base_url()).with_wire(wire);
    let req = ChatRequest {
        model: "local".into(),
        messages: vec![ChatMessage::User("hi".into())],
        tools: vec![],
        max_tokens: 5,
        enable_thinking: None,
    };
    let r = client
        .chat(&req, &CancellationToken::new(), &mut |_| {})
        .await
        .unwrap();
    assert_eq!(r.tool_calls[0].name, "read");
    let log = read_wire(&tmp.path().join("w.jsonl")).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].kind, WireKind::Request);
    assert_eq!(log[0].body.as_bytes(), req.to_bytes().as_slice());
    assert_eq!(server.requests().last().unwrap().body, req.to_bytes());
    assert_eq!(log[1].kind, WireKind::Response);
    assert_eq!(log[1].status, Some(200));
    assert!(log[1].body.ends_with("data: [DONE]\n\n"));
    assert_eq!(log[0].seq, log[1].seq);
}

#[tokio::test]
async fn abort_returns_quickly() {
    let server = ReplayServer::start(vec![Reply::Chat(ScriptedReply::text("x"))]).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let req = ChatRequest {
        model: "local".into(),
        messages: vec![],
        tools: vec![],
        max_tokens: 5,
        enable_thinking: None,
    };
    let r = Client::new(&server.base_url())
        .chat(&req, &cancel, &mut |_| {})
        .await;
    assert!(matches!(r, Err(AiError::Aborted)), "{r:?}");
}

#[tokio::test]
async fn unreachable_server_is_a_connect_error() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let r = Client::new(&format!("http://127.0.0.1:{port}/v1"))
        .props()
        .await;
    assert!(matches!(r, Err(AiError::Connect { .. })), "{r:?}");
}
