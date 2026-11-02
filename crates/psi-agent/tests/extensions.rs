#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! Extensions in the agent loop (psi-plan.md §9.4, Phase 5): tools from Lua
//! and script tools, the tool_call gate and rewrite, tool_result, and the
//! system-prompt addition, against the replay server.

use std::path::Path;
use std::sync::{Arc, RwLock};

use psi_agent::{Agent, AgentConfig, Budgets, Safety};
use psi_ai::Client;
use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_ext::{ExtManager, Limits, Shared, Source};
use psi_safety::isolation::{Isolation, SandboxConfig, choose};
use psi_session::{EntryBody, SessionFile};
use psi_tools::{ToolCtx, core_tools};
use serde_json::{Value, json};

const EXT: &str = r#"
return {
  name = "helper", version = "0.1.0", description = "Test helper",
  setup = function(psi)
    psi.register_tool{
      name = "line_count", description = "Count the lines of a file",
      params = { path = { type = "string", description = "The file" } },
      annotations = { read_only = true },
      execute = function(args, ctx)
        local r = ctx.execute_tool("read", { path = args.path })
        if r.is_error then return { text = r.text, is_error = true } end
        local n = 0
        for _ in r.text:gmatch("[^\n]+") do n = n + 1 end
        return args.path .. " has " .. n .. " lines"
      end,
    }
    psi.on("tool_call", function(ev)
      if ev.name == "bash" and ev.args.command == "rm -rf build" then
        return { block = true, reason = "the build directory is shared; ask the user" }
      end
      if ev.name == "bash" and ev.args.command == "pytest" then
        return { args = { command = "echo pytest -q" } }
      end
    end)
    psi.on("tool_result", function(ev)
      if ev.name == "bash" then return { text = ev.text .. "\n(checked by helper)" } end
    end)
    psi.on("before_agent_start", function() return { system = "Answer in one line." } end)
  end,
}
"#;

fn call(name: &str, args: Value) -> Reply {
    Reply::Chat(ScriptedReply::call(name, args))
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

#[tokio::test(flavor = "multi_thread")]
async fn extension_tools_gates_rewrites_and_results() {
    // Not under /tmp: bash's /tmp is private inside the sandbox.
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let tmp = tempfile::tempdir_in(base).unwrap();
    let cwd = tmp.path().join("project");
    let exts = tmp.path().join("exts");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(exts.join("wc")).unwrap();
    std::fs::write(cwd.join("notes.txt"), "a\nb\nc\n").unwrap();
    std::fs::write(exts.join("helper.lua"), EXT).unwrap();
    // A script tool: JSON on stdin, the result on stdout.
    std::fs::write(
        exts.join("wc/tool.json"),
        r#"{"name": "word_count", "description": "Count words", "read_only": true,
            "params": {"path": {"type": "string", "description": "A file"}}, "command": "run.sh"}"#,
    )
    .unwrap();
    std::fs::write(
        exts.join("wc/run.sh"),
        "#!/bin/sh\npath=$(sed 's/.*\"path\": *\"\\([^\"]*\\)\".*/\\1/')\nwc -w < \"$path\" | tr -d ' '\ntest -w / && echo 'not isolated'; exit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(
        exts.join("wc/run.sh"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();

    let sources: Vec<Source> = psi_ext::load::discover(&exts)
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    let shared = Arc::new(RwLock::new(Shared {
        cwd: cwd.display().to_string(),
        mode: "test".into(),
        ..Shared::default()
    }));
    let (ui, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let (ext, errors) = tokio::task::spawn_blocking(move || {
        ExtManager::start(&sources, shared, Limits::default(), ui)
    })
    .await
    .unwrap();
    assert!(errors.is_empty(), "{errors:?}");

    let server = ReplayServer::start(vec![
        Reply::Chat(ScriptedReply {
            tool_calls: vec![
                (
                    "line_count".into(),
                    json!({"path": "notes.txt"}).to_string(),
                ),
                (
                    "word_count".into(),
                    json!({"path": "notes.txt"}).to_string(),
                ),
            ],
            ..Default::default()
        }),
        call("bash", json!({"command": "rm -rf build"})),
        call("bash", json!({"command": "pytest"})),
        text("done"),
    ])
    .unwrap();
    let tmpdir = tmp.path().join("tmp");
    let mut ctx = ToolCtx::new(&cwd, &tmpdir, 20_000);
    let cfg = SandboxConfig {
        root: cwd.clone(),
        git_dirs: vec![],
        cache_dirs: vec![],
        tmpdir: tmpdir.clone(),
        network: true,
    };
    let Isolation::On(sb) = choose(true, cfg, None) else {
        panic!("the sandbox must work")
    };
    // The script lives outside the workspace; the sandbox reads it.
    ctx.sandbox = Some(sb);
    let session = SessionFile::create(&tmp.path().join("sessions"), &cwd).unwrap();
    let cfg = AgentConfig {
        model: "local".into(),
        max_output: 256,
        n_ctx: 8192,
        enable_thinking: None,
        budgets: Budgets::default(),
        system_prompt: "sys".into(),
        tools: core_tools(false),
        home: None,
    };
    let safety = Safety::headless(&cwd, None, &tmpdir);
    let mut a = Agent::new(Client::new(&server.base_url()), session, cfg, ctx)
        .with_safety(safety)
        .with_extensions(ext.clone());
    let out = a.run("look at notes").await;
    assert_eq!(out.text, "done");

    let results: Vec<(String, bool)> = a
        .session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::ToolResult {
                content, is_error, ..
            } => Some((psi_session::text_of(content), *is_error)),
            _ => None,
        })
        .collect();
    assert_eq!(results[0], ("notes.txt has 3 lines".into(), false));
    assert_eq!(
        results[1],
        ("3".into(), false),
        "the script ran inside the sandbox"
    );
    assert_eq!(
        results[2],
        (
            "Blocked: the build directory is shared; ask the user".into(),
            true
        )
    );
    assert_eq!(results[3], ("pytest -q\n(checked by helper)".into(), false));
    let decisions: Vec<(String, Option<Value>)> = a
        .session()
        .entries()
        .iter()
        .filter_map(|e| match &e.body {
            EntryBody::PolicyDecision {
                decision, rewrite, ..
            } => Some((decision.clone(), rewrite.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        decisions,
        [
            ("blocked_by_extension".to_string(), None),
            (
                "rewritten".to_string(),
                Some(json!({"command": "echo pytest -q"}))
            )
        ]
    );
    // The extension tools are offered, and the system prompt got its line.
    let reqs = server.chat_requests();
    let names: Vec<&str> = reqs[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"line_count") && names.contains(&"word_count"),
        "{names:?}"
    );
    assert_eq!(
        reqs[0]["messages"][0]["content"],
        "sys\n\nAnswer in one line."
    );
    ext.shutdown();
}

/// A sub-agent's tool calls go through the same gates, tagged with its
/// name; its lifecycle events don't reach extensions (§10.1).
#[tokio::test(flavor = "multi_thread")]
async fn sub_agent_calls_go_through_the_gates_with_their_name() {
    const GATE: &str = r#"
return {
  name = "gate", version = "0.1.0", description = "Test gate",
  setup = function(psi)
    local turns = 0
    psi.on("turn_start", function() turns = turns + 1 end)
    psi.on("tool_call", function(ev)
      if ev.name == "bash" and ev.args.command == "rm -rf build" then
        return { block = true, reason = "blocked for " .. (ev.agent or "main") .. " after " .. turns .. " turns" }
      end
    end)
  end,
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().join("project");
    let exts = tmp.path().join("exts");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&exts).unwrap();
    std::fs::write(exts.join("gate.lua"), GATE).unwrap();
    let sources: Vec<Source> = psi_ext::load::discover(&exts)
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    let shared = Arc::new(RwLock::new(Shared {
        cwd: cwd.display().to_string(),
        mode: "test".into(),
        ..Shared::default()
    }));
    let (ui, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let (ext, errors) = tokio::task::spawn_blocking(move || {
        ExtManager::start(&sources, shared, Limits::default(), ui)
    })
    .await
    .unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let server = ReplayServer::start(vec![
        call("agent", json!({"agent": "worker", "task": "Remove build."})),
        call("bash", json!({"command": "rm -rf build"})),
        text("It was blocked."),
        text("done"),
    ])
    .unwrap();
    let session = SessionFile::create(&tmp.path().join("sessions"), &cwd).unwrap();
    let cfg = AgentConfig {
        model: "local".into(),
        max_output: 256,
        n_ctx: 32_768,
        enable_thinking: None,
        budgets: Budgets::default(),
        system_prompt: "sys".into(),
        tools: core_tools(false),
        home: None,
    };
    let ctx = ToolCtx::new(&cwd, &tmp.path().join("tmp"), 20_000);
    let mut agent = Agent::new(Client::new(&server.base_url()), session, cfg, ctx)
        .with_extensions(ext)
        .with_subagents(psi_agent::subagent::built_in());
    agent.run("clean").await;
    let reqs = server.chat_requests();
    let child_result = reqs[2]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_owned();
    // One turn: the parent's; the sub-agent's turn_start never reached it.
    assert!(
        child_result.contains("blocked for worker after 1 turns"),
        "{child_result}"
    );
}
