#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Extensions through the manager: real Lua states on their own threads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use psi_ext::{ExtManager, Gate, Limits, Request, Shared, Source};
use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;

fn write(dir: &Path, name: &str, code: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, code).unwrap();
    p
}

const EXT: &str = r#"
return {
  name = "demo", version = "0.1.0", description = "A demo",
  setup = function(psi)
    psi.register_tool{
      name = "shout", description = "Upper-case a text",
      params = { text = { type = "string", description = "The text" } },
      execute = function(args, ctx) return string.upper(args.text) .. " in " .. ctx.cwd end,
    }
    psi.on("tool_call", function(ev)
      if ev.name == "bash" and ev.args.command:find("rm -rf /", 1, true) then
        return { block = true, reason = "not that" }
      end
      if ev.name == "bash" then return { args = { command = ev.args.command .. " --dry-run" } } end
    end)
    psi.on("tool_result", function(ev) return { text = ev.text .. " (checked)" } end)
    psi.on("before_agent_start", function(ev) return { system = "Be brief." } end)
    psi.register_command("ask", {
      description = "Ask a question",
      handler = function(args, ctx)
        local yes = ctx.ui.confirm("Really?", args)
        psi.append_entry("asked", { answer = yes })
        psi.events.emit("asked", { answer = yes })
      end,
    })
    psi.events.on("asked", function(data) psi.ui.notify("heard " .. tostring(data.answer)) end)
  end,
}
"#;

async fn start(
    sources: Vec<Source>,
    limits: Limits,
) -> (
    Arc<ExtManager>,
    Vec<String>,
    UnboundedReceiver<(String, Request)>,
) {
    let shared = Arc::new(RwLock::new(Shared {
        cwd: "/w".into(),
        mode: "test".into(),
        ..Shared::default()
    }));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (m, errors) =
        tokio::task::spawn_blocking(move || ExtManager::start(&sources, shared, limits, tx))
            .await
            .unwrap();
    (m, errors, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_events_commands_and_ui() {
    let d = tempfile::tempdir().unwrap();
    let p = write(d.path(), "demo.lua", EXT);
    let (m, errors, mut ui) = start(vec![Source::Lua(p)], Limits::default()).await;
    assert!(errors.is_empty(), "{errors:?}");
    let tools = m.tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].params,
        [(
            "text".to_string(),
            "string".to_string(),
            "The text".to_string(),
            true
        )]
    );
    assert_eq!(
        m.call_tool("shout", json!({"text": "hi"})).await,
        ("HI in /w".into(), false)
    );

    // tool_call: a gate that blocks one call and rewrites another.
    let g = m
        .gate(
            "tool_call",
            json!({"id": "c1", "name": "bash", "args": {"command": "rm -rf /"}}),
        )
        .await;
    assert_eq!(
        g,
        Gate::Block {
            by: "demo".into(),
            reason: "not that".into()
        }
    );
    let g = m
        .gate(
            "tool_call",
            json!({"id": "c2", "name": "bash", "args": {"command": "make"}}),
        )
        .await;
    assert_eq!(
        g,
        Gate::Allow(json!({"id": "c2", "name": "bash", "args": {"command": "make --dry-run"}}))
    );
    // tool_result and before_agent_start chain.
    let r = m
        .chain(
            "tool_result",
            json!({"id": "c2", "name": "bash", "args": {}, "text": "ok", "is_error": false}),
        )
        .await;
    assert_eq!(r["text"], "ok (checked)");
    assert_eq!(
        m.chain("before_agent_start", json!({"prompt": "x", "system": ""}))
            .await["system"],
        "Be brief."
    );

    // A command asks the user; the answer comes back over the channel.
    let run = {
        let m = m.clone();
        tokio::spawn(async move { m.run_command("ask", "drop the table?").await })
    };
    let (ext, req) = ui.recv().await.unwrap();
    assert_eq!(ext, "demo");
    let Request::Confirm {
        title,
        message,
        reply,
    } = req
    else {
        panic!()
    };
    assert_eq!(
        (title.as_str(), message.as_str()),
        ("Really?", "drop the table?")
    );
    reply.send(true).unwrap();
    run.await.unwrap().unwrap();
    // The entry went to the agent's queue, and the bus event to every handler.
    let (_, req) = ui.recv().await.unwrap();
    let Request::Notify { message, .. } = req else {
        panic!("{req:?}")
    };
    assert_eq!(message, "heard true");
    assert_eq!(
        m.take_agent_requests(),
        [psi_ext::AgentRequest::AppendEntry {
            custom_type: "asked".into(),
            data: json!({"answer": true})
        }]
    );
}

/// §15 Phase 5: a runaway Lua loop is killed without freezing psi.
#[tokio::test(flavor = "multi_thread")]
async fn a_runaway_handler_is_stopped_and_the_gate_fails_closed() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        d.path(),
        "loop.lua",
        r#"return { name = "loop", version = "1", description = "spins",
          setup = function(psi)
            psi.on("tool_call", function() while true do pcall(function() while true do end end) end end)
            psi.register_command("ok", { description = "still alive", handler = function() end })
          end }"#,
    );
    let limits = Limits {
        busy: Duration::from_millis(300),
        ..Limits::default()
    };
    let (m, errors, _ui) = start(vec![Source::Lua(p)], limits).await;
    assert!(errors.is_empty(), "{errors:?}");
    // psi stays responsive while the loop spins: a timer keeps ticking.
    let ticks = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let t = ticks.clone();
    let ticker = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            t.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });
    let start = Instant::now();
    let g = m
        .gate("tool_call", json!({"id": "c", "name": "bash", "args": {}}))
        .await;
    let took = start.elapsed();
    ticker.abort();
    let Gate::Block { reason, .. } = g else {
        panic!("{g:?}")
    };
    assert!(reason.contains("ran too long"), "{reason}");
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert!(ticks.load(std::sync::atomic::Ordering::Relaxed) >= 5);
    // The extension still works afterwards.
    m.run_command("ok", "").await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn one_extensions_error_keeps_anothers_change() {
    // A redacts tool results; B's handler fails. The model must still get
    // A's redaction, and an input A consumed must stay consumed.
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.lua",
        r#"return { name = "a", version = "1", description = "redacts",
          setup = function(psi)
            psi.on("tool_result", function(ev) return { text = "[redacted]" } end)
            psi.on("input", function(ev) return { handled = true } end)
          end }"#,
    );
    let b = write(
        d.path(),
        "b.lua",
        r#"return { name = "b", version = "1", description = "fails",
          setup = function(psi)
            psi.on("tool_result", function(ev) error("boom") end)
            psi.on("input", function(ev) error("boom") end)
          end }"#,
    );
    let (m, errors, mut ui) = start(vec![Source::Lua(a), Source::Lua(b)], Limits::default()).await;
    assert!(errors.is_empty(), "{errors:?}");
    let out = m
        .chain(
            "tool_result",
            json!({"id": "c", "name": "read", "text": "secret", "is_error": false}),
        )
        .await;
    assert_eq!(out["text"], "[redacted]");
    let out = m
        .chain("input", json!({"text": "hi", "source": "interactive"}))
        .await;
    assert_eq!(out["handled"], true);
    // Each failure is shown to the user, not dropped.
    for event in ["tool_result", "input"] {
        let (ext, req) = ui.recv().await.unwrap();
        let Request::Notify { message, level } = req else {
            panic!("{req:?}")
        };
        assert_eq!((ext.as_str(), level.as_str()), ("b", "error"));
        assert!(
            message.starts_with(&format!("the {event} handler failed")) && message.contains("boom"),
            "{message}"
        );
    }
}

/// A gate whose result can't become JSON blocks, and says why; `cancel`
/// stops compaction as the API documents.
#[tokio::test(flavor = "multi_thread")]
async fn a_gate_result_that_isnt_json_blocks_and_cancel_stops_compaction() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        d.path(),
        "gate.lua",
        r#"return { name = "gate", version = "1", description = "odd",
          setup = function(psi)
            psi.on("tool_call", function(ev)
              return { block = true, reason = "no", why = function() end }
            end)
            psi.on("session_before_compact", function(ev) return { cancel = true } end)
          end }"#,
    );
    let (m, errors, mut ui) = start(vec![Source::Lua(p)], Limits::default()).await;
    assert!(errors.is_empty(), "{errors:?}");
    let call = json!({"id": "c", "name": "bash", "args": {"command": "ls"}});
    let Gate::Block { by, reason } = m.gate("tool_call", call).await else {
        panic!("a result with a function in it let the call through");
    };
    assert_eq!(by, "gate");
    assert!(reason.contains("the gate extension failed"), "{reason}");
    let (_, req) = ui.recv().await.unwrap();
    assert!(
        matches!(&req, Request::Notify { level, .. } if level == "error"),
        "{req:?}"
    );
    let compact = json!({"tokens": 1, "reason": "threshold"});
    assert!(matches!(
        m.gate("session_before_compact", compact).await,
        Gate::Block { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gate_whose_extension_stopped_blocks() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        d.path(),
        "gate.lua",
        r#"return { name = "gate", version = "1", description = "allows",
          setup = function(psi) psi.on("tool_call", function(ev) end) end }"#,
    );
    let (m, errors, _ui) = start(vec![Source::Lua(p)], Limits::default()).await;
    assert!(errors.is_empty(), "{errors:?}");
    let call = json!({"id": "c", "name": "bash", "args": {"command": "ls"}});
    assert!(matches!(
        m.gate("tool_call", call.clone()).await,
        Gate::Allow(_)
    ));
    m.shutdown();
    let Gate::Block { by, reason } = m.gate("tool_call", call).await else {
        panic!("a stopped gate let the call through");
    };
    assert!(reason.contains("the gate extension failed"), "{reason}");
    assert_eq!(by, "gate");
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_the_user_in_setup_answers_at_once() {
    // Nothing can answer while extensions load: an empty answer now and a
    // load problem that says where asking works, not a 20 s wait.
    let d = tempfile::tempdir().unwrap();
    let p = write(
        d.path(),
        "eager.lua",
        r#"return { name = "eager", version = "1", description = "asks too early",
          setup = function(psi)
            local yes = psi.ui.confirm("Enable?", "")
            psi.register_command("state", { description = "x", handler = function() end })
          end }"#,
    );
    let start_at = Instant::now();
    let (m, errors, _ui) = start(vec![Source::Lua(p)], Limits::default()).await;
    assert!(
        start_at.elapsed() < Duration::from_secs(3),
        "{:?}",
        start_at.elapsed()
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("ui.confirm in setup got an empty answer"),
        "{errors:?}"
    );
    // The extension loaded and works.
    m.run_command("state", "").await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn load_errors_name_the_file_and_the_problem() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.lua", "return 42");
    let b = write(
        d.path(),
        "b.lua",
        r#"return { name = "b", version = "1", description = "d", setup = function(psi) error("bad setup") end }"#,
    );
    let c = write(d.path(), "c.lua", "return {");
    let (m, errors, _ui) = start(
        vec![Source::Lua(a), Source::Lua(b), Source::Lua(c)],
        Limits::default(),
    )
    .await;
    assert!(m.is_empty());
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(
        errors[0].contains("an extension returns a table"),
        "{}",
        errors[0]
    );
    assert!(
        errors[1].contains("b: setup failed") && errors[1].contains("bad setup"),
        "{}",
        errors[1]
    );
    assert!(errors[2].contains("c.lua"), "{}", errors[2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_setup_names_the_returned_table() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.lua",
        r#"local function setup(psi) end
return { name = "a", version = "1", description = "d" }"#,
    );
    let (_m, errors, _ui) = start(vec![Source::Lua(a)], Limits::default()).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("the table it returns (the manifest) needs 'setup'")
            && errors[0].contains("setup = setup }"),
        "{}",
        errors[0]
    );
}
