#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! `psi -p` end to end against the replay server: the real binary, client,
//! loop, tools, session file and wire log.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use psi_session::{EntryBody, read_entries};
use serde_json::{Value, json};

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

fn env_for(server: &ReplayServer) -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let project = tmp.path().join("work/project");
    std::fs::create_dir_all(home.join(".psi")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let models =
        json!({"models": [{"id": "local", "base_url": server.base_url(), "max_output": 256}]});
    std::fs::write(home.join(".psi/models.jsonc"), models.to_string()).unwrap();
    Env {
        _tmp: tmp,
        home,
        project,
    }
}

fn psi(e: &Env, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_psi"))
        .args(args)
        .env("HOME", &e.home)
        .current_dir(&e.project)
        .output()
        .unwrap()
}

fn sessions(e: &Env) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(e.home.join(".psi/sessions"))
        .unwrap()
        .flatten()
        .flat_map(|d| {
            std::fs::read_dir(d.path())
                .unwrap()
                .flatten()
                .map(|f| f.path())
        })
        .filter(|p| !p.to_string_lossy().ends_with(".wire.jsonl"))
        .collect();
    out.sort();
    out
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

#[test]
fn print_mode_writes_session_and_wire_log() {
    let server = ReplayServer::start(vec![
        Reply::Chat(ScriptedReply::call(
            "write",
            json!({"path": "a.txt", "content": "x\n"}),
        )),
        text("Created a.txt."),
    ])
    .unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-p", "make a.txt"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "Created a.txt.\n");
    assert_eq!(
        std::fs::read_to_string(e.project.join("a.txt")).unwrap(),
        "x\n"
    );

    let s = sessions(&e);
    assert_eq!(s.len(), 1);
    let entries =
        read_entries(std::io::BufReader::new(std::fs::File::open(&s[0]).unwrap())).unwrap();
    assert!(
        matches!(&entries[0].body, EntryBody::Custom { custom_type, .. } if custom_type == "session_info")
    );
    let wire = psi_ai::wire::read_wire(&PathBuf::from(
        s[0].to_string_lossy().replace(".jsonl", ".wire.jsonl"),
    ))
    .unwrap();
    assert_eq!(wire.len(), 4, "two requests and two responses");
    // The system prompt names the tools and the working directory.
    let sys = &server.chat_requests()[0]["messages"][0]["content"];
    assert!(
        sys.as_str()
            .unwrap()
            .contains("Tools: read, write, edit, bash")
    );
    assert!(sys.as_str().unwrap().ends_with(&format!(
        "Current working directory: {}",
        e.project.display()
    )));
}

#[test]
fn json_mode_streams_events() {
    let server = ReplayServer::start(vec![text("hello")]).unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-p", "hi", "--json"]);
    assert!(out.status.success());
    let lines: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["type"], "run_start");
    let end = lines.last().unwrap();
    assert_eq!(end["type"], "run_end");
    assert_eq!(end["stop"], "done");
    assert_eq!(end["text"], "hello");
}

#[test]
fn continue_picks_up_the_last_session() {
    let server = ReplayServer::start(vec![text("one"), text("two")]).unwrap();
    let e = env_for(&server);
    assert!(psi(&e, &["-p", "first"]).status.success());
    assert!(psi(&e, &["-c", "-p", "second"]).status.success());
    assert_eq!(sessions(&e).len(), 1);
    let m = server.chat_requests()[1]["messages"].clone();
    let roles: Vec<&str> = m
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    assert_eq!(m[3]["content"], "second");
}

#[test]
fn continue_survives_a_session_cut_by_a_crash() {
    let server = ReplayServer::start(vec![text("one"), text("two"), text("three")]).unwrap();
    let e = env_for(&server);
    assert!(psi(&e, &["-p", "first"]).status.success());
    // A crash while saving: half an entry, no newline.
    let file = sessions(&e).pop().unwrap();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    std::io::Write::write_all(&mut f, br#"{"type":"message","id":"zz","parent_id":"#).unwrap();
    drop(f);
    let out = psi(&e, &["-c", "-p", "second"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("1 line(s) couldn't be read"), "{err}");
    // The history before the cut reached the model.
    let m = server.chat_requests()[1]["messages"].clone();
    assert_eq!(m[1]["content"], "first");
    assert_eq!(m.as_array().unwrap().last().unwrap()["content"], "second");
    // The new entries landed on lines of their own: a third run reads them.
    assert!(psi(&e, &["-c", "-p", "third"]).status.success());
    let m = server.chat_requests()[2]["messages"].clone();
    let users: Vec<&str> = m
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["role"] == "user")
        .map(|x| x["content"].as_str().unwrap())
        .collect();
    assert_eq!(users, ["first", "second", "third"]);
}

#[test]
fn budget_and_failures_have_exit_codes() {
    let server = ReplayServer::start(vec![Reply::Chat(ScriptedReply::call(
        "bash",
        json!({"command": "true"}),
    ))])
    .unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-p", "loop", "--max-turns", "1"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("reached the limit of 1 model requests"));

    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let down = Env {
        home: e.home.clone(),
        project: e.project.clone(),
        _tmp: tempfile::tempdir().unwrap(),
    };
    std::fs::write(
        down.home.join(".psi/models.jsonc"),
        json!({"models": [{"id": "x", "base_url": format!("http://127.0.0.1:{port}/v1")}]})
            .to_string(),
    )
    .unwrap();
    let out = psi(&down, &["-p", "hi"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Is llama-server running?"));
}

#[test]
fn no_tool_support_refuses_to_start() {
    let server = ReplayServer::start(vec![]).unwrap();
    server.set_props(json!({"default_generation_settings": {"n_ctx": 4096}, "chat_template_caps": {"supports_tool_calls": false}}));
    let e = env_for(&server);
    let out = psi(&e, &["-p", "hi"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--jinja"));
}

#[test]
fn replay_resends_a_recorded_step() {
    let server = ReplayServer::start(vec![
        Reply::Chat(ScriptedReply::call("read", json!({"path": "a"}))),
        text("done"),
    ])
    .unwrap();
    let e = env_for(&server);
    std::fs::write(e.project.join("a"), "1\n").unwrap();
    assert!(psi(&e, &["-p", "read a"]).status.success());
    let session = sessions(&e).pop().unwrap();
    // Point models.jsonc at a fresh server that answers differently.
    let again = ReplayServer::start(vec![text("a different answer")]).unwrap();
    std::fs::write(
        e.home.join(".psi/models.jsonc"),
        json!({"models": [{"id": "local", "base_url": again.base_url()}]}).to_string(),
    )
    .unwrap();
    let out = psi(&e, &["replay", session.to_str().unwrap(), "--at", "2"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "answer: a different answer\n"
    );
    assert_eq!(
        again.requests().last().unwrap().body,
        server
            .requests()
            .iter()
            .filter(|r| r.path.ends_with("completions"))
            .nth(1)
            .unwrap()
            .body
    );
}

#[test]
fn a_real_pi_transcript_replayed_through_psi_leaves_the_same_files() {
    // pi + Qwen3.5 4B passed fix_invoice_rounding with these responses; psi's
    // tools must turn the same calls into the same files.
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi-qwen-fix-invoice");
    let entries = read_entries(std::io::BufReader::new(
        std::fs::File::open(fx.join("session.jsonl")).unwrap(),
    ))
    .unwrap();
    let script = psi_lab::replay::script_from_entries(&entries);
    assert_eq!(script.len(), 18);
    let server = ReplayServer::start(vec![]).unwrap();
    let e = env_for(&server);
    // The trial ran in /work/project; this one runs in a temp directory.
    let here = e.project.display().to_string();
    let script: Vec<Reply> = script
        .into_iter()
        .map(|r| match r {
            Reply::Chat(mut c) => {
                for (_, args) in &mut c.tool_calls {
                    *args = args.replace("/work/project", &here);
                }
                Reply::Chat(c)
            }
            other => other,
        })
        .collect();
    let server = ReplayServer::start(script).unwrap();
    std::fs::write(
        e.home.join(".psi/models.jsonc"),
        json!({"models": [{"id": "local", "base_url": server.base_url()}]}).to_string(),
    )
    .unwrap();
    let repo =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/code/fix_invoice_rounding/repo");
    copy_dir(&repo, &e.project);
    let out = psi(&e, &["-p", "fix the invoices"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for f in ["money.py", "tax.py"] {
        assert_eq!(
            std::fs::read_to_string(e.project.join("billing").join(f)).unwrap(),
            std::fs::read_to_string(fx.join("final").join(f)).unwrap(),
            "{f}"
        );
    }
    assert_eq!(server.remaining(), 0);
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

/// Headless runs never prompt (D-18): an always-ask command is denied and
/// reported, `--allow` permits a named entry, and an unknown name is an error.
#[test]
fn print_mode_denies_always_ask_unless_allowed() {
    let cmd = json!({"command": "cargo publish --dry-run"});
    let replies = || {
        vec![
            Reply::Chat(ScriptedReply::call("bash", cmd.clone())),
            text("done"),
        ]
    };
    let server = ReplayServer::start(replies()).unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-p", "publish"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = sessions(&e);
    let entries = read_entries(
        std::fs::File::open(&s[0])
            .map(std::io::BufReader::new)
            .unwrap(),
    )
    .unwrap();
    let result = entries.iter().find_map(|en| match &en.body {
        EntryBody::ToolResult { content, .. } => Some(psi_session::text_of(content)),
        _ => None,
    });
    assert!(
        result
            .unwrap()
            .starts_with("Not run: `cargo publish` is on the always-ask list.")
    );
    // The session records how safety was set up, and the submit checkpoint.
    let info = entries.iter().find_map(|en| match &en.body {
        EntryBody::Custom { custom_type, data } if custom_type == "session_info" => {
            Some(data.clone())
        }
        _ => None,
    });
    assert_eq!(info.unwrap()["safety"]["mode"], "auto");
    assert!(entries.iter().any(
        |en| matches!(&en.body, EntryBody::Checkpoint { trigger: Some(t), .. } if t == "submit")
    ));

    let server = ReplayServer::start(replies()).unwrap();
    let e = env_for(&server);
    let out = psi(
        &e,
        &[
            "-p",
            "publish",
            "--allow",
            "cargo-publish",
            "--isolation",
            "off",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = psi(&e, &["-p", "x", "--allow", "rm-rf"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("--allow rm-rf: no always-ask entry by that name"),
        "{err}"
    );
}

/// psi.register_provider: `--model name/id` reaches the extension's
/// endpoint with its key and the model's name.
#[test]
fn a_registered_provider_serves_the_model() {
    let server = ReplayServer::start(vec![text("from the provider")]).unwrap();
    let e = env_for(&server);
    let ext = e.home.join("box.lua");
    std::fs::write(
        &ext,
        format!(
            r#"return {{ name = "box", version = "0.1.0", description = "a provider", setup = function(psi)
  psi.register_provider("box", {{ base_url = "{}", api_key = "$BOX_TEST_KEY",
    headers = {{ ["X-Team"] = "psi" }},
    models = {{ {{ id = "big-model", context_window = 4096, max_output = 300 }} }} }})
end }}"#,
            server.base_url()
        ),
    )
    .unwrap();
    let ext = ext.display().to_string();
    let missing = Command::new(env!("CARGO_BIN_EXE_psi"))
        .args(["-p", "hi", "--extension", &ext, "--model", "box/big-model"])
        .env("HOME", &e.home)
        .env_remove("BOX_TEST_KEY")
        .current_dir(&e.project)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let err = String::from_utf8_lossy(&missing.stderr);
    assert!(
        err.contains("provider box: $BOX_TEST_KEY is not set"),
        "{err}"
    );
    let out = Command::new(env!("CARGO_BIN_EXE_psi"))
        .args(["-p", "hi", "--extension", &ext, "--model", "big-model"])
        .env("HOME", &e.home)
        .env("BOX_TEST_KEY", "sekrit")
        .current_dir(&e.project)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "from the provider"
    );
    let chat = server
        .requests()
        .into_iter()
        .find(|r| r.path.ends_with("/chat/completions"))
        .unwrap();
    let h = |k: &str| {
        chat.headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(h("authorization").as_deref(), Some("Bearer sekrit"));
    assert_eq!(h("x-team").as_deref(), Some("psi"));
    let body = chat.json();
    assert_eq!(body["model"], "big-model");
    assert_eq!(body["max_tokens"], 300);
}

/// Packages (G-04): `psi install` lists a local package; its skills,
/// prompts and extensions load; `psi remove` takes it out again.
#[test]
fn a_local_package_installs_and_loads() {
    let server = ReplayServer::start(vec![text("one"), text("two")]).unwrap();
    let e = env_for(&server);
    let pkg = e.home.join("pkgs/kit");
    std::fs::create_dir_all(pkg.join("skills/deploy")).unwrap();
    std::fs::create_dir_all(pkg.join("prompts")).unwrap();
    std::fs::create_dir_all(pkg.join("extensions/hello")).unwrap();
    std::fs::write(
        pkg.join("skills/deploy/SKILL.md"),
        "---\ndescription: Deploys the app.\n---\nRun make deploy.",
    )
    .unwrap();
    std::fs::write(pkg.join("prompts/ship.md"), "Ship $1 now.").unwrap();
    std::fs::write(
        pkg.join("extensions/hello/init.lua"),
        r#"return { name = "hello", version = "1", description = "x", setup = function(psi)
  psi.on("before_agent_start", function() return { system = "HELLO FROM THE PACKAGE" } end)
end }"#,
    )
    .unwrap();
    std::fs::write(
        e.home.join(".psi/settings.jsonc"),
        "// my settings\n{ \"mode\": \"auto\" }\n",
    )
    .unwrap();
    let out = psi(&e, &["install", "~/pkgs/kit"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let settings = std::fs::read_to_string(e.home.join(".psi/settings.jsonc")).unwrap();
    assert!(settings.starts_with("// my settings"), "{settings}");
    assert!(settings.contains(&pkg.display().to_string()), "{settings}");
    let list = psi(&e, &["list"]);
    assert!(String::from_utf8_lossy(&list.stdout).contains("user packages"));

    let out = psi(&e, &["-p", "/ship v2"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let reqs = server.chat_requests();
    let sys = reqs[0]["messages"][0]["content"].as_str().unwrap();
    assert!(sys.contains("- deploy: Deploys the app."), "{sys}");
    assert!(sys.contains("HELLO FROM THE PACKAGE"), "{sys}");
    assert_eq!(reqs[0]["messages"][1]["content"], "Ship v2 now.");

    let out = psi(&e, &["remove", "~/pkgs/kit"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(pkg.exists(), "a local package's files stay");
    let out = psi(&e, &["-p", "hi"]);
    assert!(out.status.success());
    let sys = server.chat_requests()[1]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(!sys.contains("deploy"), "{sys}");
    let again = psi(&e, &["remove", "~/pkgs/kit"]);
    assert!(!again.status.success());
}

/// The `input` event (pi parity): handlers see the prompt before template
/// expansion and can rewrite it or consume it.
#[test]
fn input_handlers_rewrite_or_consume_the_prompt() {
    let server = ReplayServer::start(vec![text("done")]).unwrap();
    let e = env_for(&server);
    std::fs::create_dir_all(e.home.join(".psi/prompts")).unwrap();
    std::fs::write(e.home.join(".psi/prompts/greet.md"), "Say hello to $1.\n").unwrap();
    let ext = e.home.join("router.lua");
    std::fs::write(
        &ext,
        r#"return { name = "router", version = "0.1.0", description = "d", setup = function(psi)
  psi.on("input", function(ev)
    if ev.text == "ping" then return { handled = true } end
    if ev.text == "hi" then return { text = "/greet " .. ev.source } end
  end)
end }"#,
    )
    .unwrap();
    let ext = ext.display().to_string();
    let out = psi(&e, &["-p", "ping", "--extension", &ext]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    assert!(
        !server
            .requests()
            .iter()
            .any(|r| r.path.ends_with("/chat/completions")),
        "a consumed prompt sends nothing"
    );
    let out = psi(&e, &["-p", "hi", "--extension", &ext]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let chat = server
        .requests()
        .into_iter()
        .find(|r| r.path.ends_with("/chat/completions"))
        .unwrap();
    let body = chat.json();
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.last().unwrap()["content"], "Say hello to print.\n");
}

fn error(status: u16, message: &str) -> Reply {
    Reply::Error {
        status,
        body: json!({"error": {"code": status, "message": message, "type": "server_error"}}),
    }
}

#[test]
fn the_prompt_can_come_from_stdin() {
    let server = ReplayServer::start(vec![text("ok")]).unwrap();
    let e = env_for(&server);
    let mut child = Command::new(env!("CARGO_BIN_EXE_psi"))
        .args(["-p", "-"])
        .env("HOME", &e.home)
        .current_dir(&e.project)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"a prompt\nover two lines\n")
        .unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let m = server.chat_requests()[0]["messages"].clone();
    assert_eq!(m[1]["content"], "a prompt\nover two lines\n");
}

#[test]
fn a_failure_mid_run_says_why_and_exits_1() {
    let server = ReplayServer::start(vec![
        Reply::Chat(ScriptedReply::call("bash", json!({"command": "true"}))),
        error(500, "CUDA error: out of memory"),
    ])
    .unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-p", "go"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("out of memory"), "{err}");
    // The same in --json: the last event says how it stopped and why.
    let server = ReplayServer::start(vec![
        Reply::Chat(ScriptedReply::call("bash", json!({"command": "true"}))),
        error(500, "CUDA error: out of memory"),
    ])
    .unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-p", "go", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let end: Value = serde_json::from_str(
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(end["type"], "run_end");
    assert_eq!(end["stop"], "failed");
    assert!(
        end["reason"].as_str().unwrap().contains("out of memory"),
        "{end}"
    );
}

#[test]
fn a_guard_stop_exits_3_with_its_reason() {
    let replies = (0..6)
        .map(|_| Reply::Chat(ScriptedReply::call("bash", json!({"command": "false"}))))
        .collect();
    let server = ReplayServer::start(replies).unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-p", "go"]);
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("the same call failed 5 times in a row"),
        "{err}"
    );
}

#[test]
fn continue_with_no_earlier_session_says_so() {
    let server = ReplayServer::start(vec![text("x")]).unwrap();
    let e = env_for(&server);
    let out = psi(&e, &["-c", "-p", "hi"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no earlier session in"), "{err}");
    assert!(server.chat_requests().is_empty());
}

#[test]
fn a_session_file_can_be_named() {
    let server = ReplayServer::start(vec![text("one"), text("two"), text("three")]).unwrap();
    let e = env_for(&server);
    assert!(psi(&e, &["-p", "first"]).status.success());
    assert!(psi(&e, &["-p", "other"]).status.success());
    // (Files of the same minute sort by a random suffix: find it by content.)
    let first = sessions(&e)
        .into_iter()
        .find(|p| std::fs::read_to_string(p).unwrap().contains("\"first\""))
        .unwrap();
    let out = psi(&e, &["--session", first.to_str().unwrap(), "-p", "back"]);
    assert!(out.status.success());
    let m = server.chat_requests()[2]["messages"].clone();
    assert_eq!(m[1]["content"], "first");
    assert_eq!(m.as_array().unwrap().last().unwrap()["content"], "back");
}

#[test]
fn ctrl_c_stops_a_print_run_with_130() {
    let server = ReplayServer::start(vec![Reply::hang("Thinking")]).unwrap();
    let e = env_for(&server);
    let child = Command::new(env!("CARGO_BIN_EXE_psi"))
        .args(["-p", "go"])
        .env("HOME", &e.home)
        .current_dir(&e.project)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let requests = server.chat_counter();
    let start = std::time::Instant::now();
    while requests() == 0 && start.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(130));
    // The session records the stopped answer and still opens.
    let s = psi_session::SessionFile::open(&sessions(&e)[0]).unwrap();
    assert!(s.entries().iter().any(|en| matches!(
        &en.body,
        EntryBody::Message {
            stop_reason: Some(psi_session::StopReason::Aborted),
            ..
        }
    )));
}

/// How the eval runner classifies a trial, fed with the real binary's
/// `--json` output and exit code (the strings it reads are psi's own).
#[test]
fn the_eval_runner_classifies_real_psi_output() {
    use psi_lab::eval::{Grade, Outcome, psi_outcome};
    let run = |replies: Vec<Reply>, args: &[&str]| {
        let server = ReplayServer::start(replies).unwrap();
        let e = env_for(&server);
        let mut all = vec!["-p", "task", "--json"];
        all.extend_from_slice(args);
        let out = psi(&e, &all);
        (
            String::from_utf8(out.stdout).unwrap(),
            out.status.code().unwrap_or(-1),
        )
    };
    let grade = |p, t| {
        Ok(Grade {
            passed: p,
            total: t,
        })
    };
    let bash = || Reply::Chat(ScriptedReply::call("bash", json!({"command": "true"})));

    let (out, code) = run(vec![text("done")], &[]);
    assert_eq!(
        psi_outcome(&out, code, &grade(3, 3)),
        (Outcome::Pass, String::new())
    );

    let (out, code) = run(vec![bash()], &["--max-turns", "1"]);
    assert_eq!(
        psi_outcome(&out, code, &grade(1, 3)),
        (Outcome::ModelFail, "step cap; checks: 1 passed of 3".into())
    );

    let (out, code) = run(vec![bash(), error(500, "CUDA error: out of memory")], &[]);
    let (o, why) = psi_outcome(&out, code, &grade(0, 3));
    assert_eq!(o, Outcome::HarnessFail);
    assert!(
        why.starts_with("psi failed:") && why.contains("out of memory"),
        "{why}"
    );

    let failing = (0..6)
        .map(|_| Reply::Chat(ScriptedReply::call("bash", json!({"command": "false"}))))
        .collect();
    let (out, code) = run(failing, &[]);
    assert_eq!(
        psi_outcome(&out, code, &grade(0, 3)),
        (Outcome::ModelFail, "checks: 0 passed of 3".into())
    );

    // A trial past its time: the runner wraps psi in `timeout -s INT`.
    let server = ReplayServer::start(vec![Reply::hang("thinking")]).unwrap();
    let e = env_for(&server);
    let out = Command::new("timeout")
        .args([
            "-k",
            "5",
            "-s",
            "INT",
            "2",
            env!("CARGO_BIN_EXE_psi"),
            "-p",
            "task",
            "--json",
        ])
        .env("HOME", &e.home)
        .current_dir(&e.project)
        .output()
        .unwrap();
    let (o, why) = psi_outcome(
        &String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
        &grade(0, 3),
    );
    assert_eq!(o, Outcome::ModelFail);
    assert!(why.starts_with("timeout"), "{why}");
}

/// `psi inspect` on sessions the binary itself wrote: filters, a trial
/// directory with its result, and a session with a branch left behind.
#[test]
fn inspect_reads_real_sessions() {
    let server = ReplayServer::start(vec![
        Reply::Chat(ScriptedReply::call("bash", json!({"command": "git push"}))),
        Reply::Chat(ScriptedReply::call("bash", json!({"command": "false"}))),
        text("done"),
    ])
    .unwrap();
    let e = env_for(&server);
    assert_eq!(psi(&e, &["-p", "ship it"]).status.code(), Some(0));
    let session = sessions(&e).pop().unwrap();
    let inspect = |args: &[&str], path: &Path| {
        let out = Command::new(env!("CARGO_BIN_EXE_psi"))
            .arg("inspect")
            .args(args)
            .arg(path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    };
    // --blocked: the denied push, and nothing else.
    let (out, _) = inspect(&["--blocked", "--json"], &session);
    let v: Value = serde_json::from_str(&out).unwrap();
    let events = v["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{v}");
    assert!(events[0].to_string().contains("always-ask"), "{v}");
    // --step: one step, in full; --failures: the failed command's step.
    let (out, _) = inspect(&["--step", "2"], &session);
    assert!(
        out.contains("bash {\"command\":\"false\"}") && !out.contains("git push"),
        "{out}"
    );
    let (out, _) = inspect(&["--failures", "--json"], &session);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(v["events"].to_string().contains("false"), "{v}");

    // A trial directory: its result leads the timeline.
    let trial = e.home.join("trial-0");
    std::fs::create_dir_all(&trial).unwrap();
    std::fs::copy(&session, trial.join("session.jsonl")).unwrap();
    std::fs::write(
        trial.join("result.json"),
        json!({"task": "ship", "trial": 0, "agent": "psi", "outcome": "MODEL-FAIL",
               "reason": "checks: 1 passed of 2", "checks_passed": 1, "checks_total": 2,
               "steps": 3, "tool_calls": 2, "tool_errors": 2, "tokens_in": 10,
               "tokens_out": 5, "step_cap_hit": false, "exit_code": 0, "wall_s": 1})
        .to_string(),
    )
    .unwrap();
    let (out, _) = inspect(&[], &trial);
    assert!(out.starts_with("psi ship: MODEL-FAIL checks 1/2"), "{out}");

    // A branch left behind (/tree): the timeline follows the last one.
    let mut s = psi_session::SessionFile::open(&session).unwrap();
    let first_user = s
        .entries()
        .iter()
        .find(|en| {
            matches!(
                &en.body,
                EntryBody::Message {
                    role: psi_session::Role::User,
                    ..
                }
            )
        })
        .unwrap()
        .id
        .clone();
    s.set_leaf(&first_user).unwrap();
    s.append(EntryBody::Message {
        role: psi_session::Role::Assistant,
        content: vec![psi_session::Block::Text {
            text: "another way".into(),
        }],
        usage: None,
        stop_reason: None,
        error: None,
        model: None,
    })
    .unwrap();
    let (out, err) = inspect(&[], &session);
    assert!(
        out.contains("another way") && !out.contains("git push"),
        "{out}"
    );
    assert!(err.contains("entries on other branches left out"), "{err}");
}

/// Records every event it sees (one line each) and uses the API through
/// the real binary: flags, entries, active tools, messages, the bus.
const LIFECYCLE: &str = r#"
local function rec(line)
  local f = io.open("events.log", "a")
  f:write(line .. "\n")
  f:close()
end
local EVENTS = {"session_start", "agent_start", "turn_start", "context", "message_start",
  "message_update", "message_end", "tool_call", "tool_execution_update", "tool_result",
  "policy_decision", "guard_event", "turn_end", "agent_end", "session_shutdown"}
return { name = "lifecycle", version = "1", description = "records events",
  setup = function(psi)
    psi.register_flag("tag", { description = "a tag", default = "none" })
    for _, name in ipairs(EVENTS) do
      psi.on(name, function(e)
        local s = name
        if e.turn ~= nil then s = s .. " turn=" .. tostring(e.turn) end
        if e.decision ~= nil then s = s .. " decision=" .. tostring(e.decision) end
        if e.guard ~= nil then s = s .. " guard=" .. tostring(e.guard) end
        if name == "context" then s = s .. " messages=" .. tostring(#e.messages) end
        rec(s)
      end)
    end
    psi.on("agent_start", function(e)
      rec("flag=" .. tostring(psi.get_flag("tag")))
      rec("entries=" .. tostring(#psi.entries("seen")))
      psi.append_entry("seen", { prompt = e.prompt })
      psi.set_active_tools({ "read", "bash" })
      rec("active=" .. table.concat(psi.get_active_tools(), ","))
      psi.events.emit("hello", { from = "lifecycle" })
    end)
    psi.on("turn_end", function(e)
      if e.turn == 1 then psi.send_message("note", "the extension says hi") end
    end)
  end }
"#;

const LISTENER: &str = r#"
return { name = "listener", version = "1", description = "hears the bus",
  setup = function(psi)
    psi.events.on("hello", function(d)
      local f = io.open("events.log", "a")
      f:write("bus from=" .. tostring(d.from) .. "\n")
      f:close()
    end)
  end }
"#;

#[test]
fn extensions_see_every_event_and_use_the_api() {
    let bash = |c: &str| Reply::Chat(ScriptedReply::call("bash", json!({"command": c})));
    let server = ReplayServer::start(vec![
        bash("git push"),
        bash("false"),
        bash("false"),
        bash("false"),
        text("done"),
        text("again"),
    ])
    .unwrap();
    let e = env_for(&server);
    let exts = e.home.join("exts");
    std::fs::create_dir_all(&exts).unwrap();
    std::fs::write(exts.join("lifecycle.lua"), LIFECYCLE).unwrap();
    std::fs::write(exts.join("listener.lua"), LISTENER).unwrap();
    let args = |prompt: &'static str| {
        vec![
            "--extension".to_owned(),
            exts.join("lifecycle.lua").display().to_string(),
            "--extension".to_owned(),
            exts.join("listener.lua").display().to_string(),
            "--ext-flag".to_owned(),
            "tag=blue".to_owned(),
            "-p".to_owned(),
            prompt.to_owned(),
        ]
    };
    let a = args("ship it");
    let out = psi(&e, &a.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = std::fs::read_to_string(e.project.join("events.log")).unwrap();
    let lines: Vec<&str> = log.lines().collect();

    // Session events first and last; the run's in between.
    assert_eq!(lines.first(), Some(&"session_start"), "{log}");
    assert_eq!(lines.last(), Some(&"session_shutdown"), "{log}");
    for want in [
        "agent_start",
        "agent_end",
        "message_update",
        "tool_execution_update",
    ] {
        assert!(lines.contains(&want), "{want} missing:\n{log}");
    }
    // Each request: turn_start, context, message_start and message_end
    // with its turn number (message_start used to say 0).
    for turn in 1..=5 {
        for ev in ["turn_start", "message_start", "turn_end"] {
            assert!(
                lines.contains(&format!("{ev} turn={turn}").as_str()),
                "{ev} {turn}:\n{log}"
            );
        }
    }
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("context messages="))
            .count(),
        5
    );
    assert_eq!(lines.iter().filter(|l| **l == "message_end").count(), 5);
    assert_eq!(lines.iter().filter(|l| **l == "tool_call").count(), 4);
    assert_eq!(lines.iter().filter(|l| **l == "tool_result").count(), 4);
    assert!(lines.contains(&"policy_decision decision=denied"), "{log}");
    assert!(
        lines.contains(&"guard_event guard=repeated_failure"),
        "{log}"
    );
    // The API: the flag, the bus to another extension, active tools.
    assert!(lines.contains(&"flag=blue"), "{log}");
    assert!(lines.contains(&"bus from=lifecycle"), "{log}");
    assert!(lines.contains(&"active=read,bash"), "{log}");
    let reqs = server.chat_requests();
    let tools: Vec<&str> = reqs[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["read", "bash"]);
    // send_message: the model sees it with the next request.
    assert!(
        reqs[1]["messages"]
            .to_string()
            .contains("the extension says hi")
    );
    assert!(
        !reqs[0]["messages"]
            .to_string()
            .contains("the extension says hi")
    );

    // append_entry is kept in the session: a continued run reads it back.
    assert!(lines.contains(&"entries=0"), "{log}");
    std::fs::remove_file(e.project.join("events.log")).unwrap();
    let mut a = args("once more");
    a.insert(0, "-c".to_owned());
    let out = psi(&e, &a.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = std::fs::read_to_string(e.project.join("events.log")).unwrap();
    assert!(log.lines().any(|l| l == "entries=1"), "{log}");
}

#[test]
fn the_thinking_switch_reaches_the_server() {
    let server = ReplayServer::start(vec![text("a"), text("b")]).unwrap();
    let e = env_for(&server);
    assert!(psi(&e, &["-p", "hi"]).status.success());
    // Unset: the server's own default.
    assert!(
        server.chat_requests()[0]
            .get("chat_template_kwargs")
            .is_none()
    );
    let models = json!({"models": [{"id": "local", "base_url": server.base_url(),
        "max_output": 256, "reasoning": {"enable_thinking": false}}]});
    std::fs::write(e.home.join(".psi/models.jsonc"), models.to_string()).unwrap();
    assert!(psi(&e, &["-p", "hi"]).status.success());
    assert_eq!(
        server.chat_requests()[1]["chat_template_kwargs"],
        json!({"enable_thinking": false})
    );
}
