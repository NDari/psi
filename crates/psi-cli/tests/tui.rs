#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The interactive mode end to end (Phase 4): the real binary in a
//! pseudo-terminal, the replay server as the model, and a vt100 parser
//! reading the screen. These cover the manual QA checklist's automatable
//! items and measure the feel targets (§11.3).

use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use psi_ai::replay::{ReplayServer, Reply, ScriptedReply};
use serde_json::{Value, json};

const W: u16 = 100;

fn nz(n: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(n).unwrap()
}
const H: u16 = 30;

struct Tui {
    _tmp: tempfile::TempDir,
    project: PathBuf,
    master: std::fs::File,
    screen: Arc<Mutex<vt100::Parser>>,
    /// The same output on a terminal too tall to scroll: everything ever
    /// committed, once each.
    tall: Arc<Mutex<vt100::Parser>>,
    child: Child,
    started: Instant,
}

fn pty() -> (OwnedFd, OwnedFd) {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name.to_str().unwrap())
        .unwrap();
    let ws = rustix::termios::Winsize {
        ws_row: H,
        ws_col: W,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    rustix::termios::tcsetwinsize(&master, ws).unwrap();
    (master, slave.into())
}

impl Tui {
    fn start(replies: Vec<Reply>, setup: impl FnOnce(&std::path::Path)) -> (Self, ReplayServer) {
        Self::start_with(replies, setup, &[])
    }

    fn start_with(
        replies: Vec<Reply>,
        setup: impl FnOnce(&std::path::Path),
        args: &[&str],
    ) -> (Self, ReplayServer) {
        let server = ReplayServer::start(replies).unwrap();
        // Not inside the psi repository, whose target/ is gitignored.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let models =
            json!({"models": [{"id": "local", "base_url": server.base_url(), "max_output": 256}]});
        std::fs::write(home.join(".psi/models.jsonc"), models.to_string()).unwrap();
        setup(&project);
        let (master, slave) = pty();
        let started = Instant::now();
        let child = Command::new(env!("CARGO_BIN_EXE_psi"))
            .args(args)
            .env("HOME", &home)
            .env("TERM", "xterm-256color")
            .env_remove("PSI_LOG")
            .current_dir(&project)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .unwrap();
        let screen = Arc::new(Mutex::new(vt100::Parser::new(nz(H), nz(W), 10_000)));
        let mut reader = std::fs::File::from(master.try_clone().unwrap());
        let tall = Arc::new(Mutex::new(vt100::Parser::new(nz(5000), nz(W), 0)));
        let (s, t2) = (screen.clone(), tall.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                s.lock().unwrap().process(&buf[..n]);
                t2.lock().unwrap().process(&buf[..n]);
            }
        });
        let tui = Self {
            _tmp: tmp,
            project,

            master: std::fs::File::from(master),
            screen,
            tall,
            child,
            started,
        };
        (tui, server)
    }

    fn contents(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }

    /// Everything printed so far, as on a terminal that never scrolls.
    fn all(&self) -> String {
        self.tall.lock().unwrap().screen().contents()
    }

    fn wait_for(&self, what: &str, max: Duration) -> Duration {
        let start = Instant::now();
        while start.elapsed() < max {
            if self.contents().contains(what) || self.all().contains(what) {
                return start.elapsed();
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!(
            "waited {max:?} for {what:?}; the screen:\n{}",
            self.contents()
        );
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
        self.master.flush().unwrap();
    }

    fn typ(&mut self, s: &str) {
        self.send(s.as_bytes());
    }

    /// Esc on its own: a byte after it too soon reads as Alt+key.
    fn esc(&mut self) {
        self.send(b"\x1b");
        std::thread::sleep(Duration::from_millis(100));
    }

    fn quit(mut self) -> String {
        self.send(b"\x04"); // Ctrl+D on an empty input
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return self.all();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        panic!(
            "psi did not exit on Ctrl+D; the screen:\n{}",
            self.contents()
        );
    }
}

fn call(name: &str, args: Value) -> Reply {
    Reply::Chat(ScriptedReply::call(name, args))
}

fn text(t: &str) -> Reply {
    Reply::Chat(ScriptedReply::text(t))
}

const SLOW: Duration = Duration::from_secs(10);

#[test]
fn a_prompt_streams_an_answer_and_tool_calls_commit_once() {
    let (mut t, _s) = Tui::start(
        vec![
            call("write", json!({"path": "hello.txt", "text": "hi\n"})),
            text("Created **hello.txt** for you.\n\n```sh\ncat hello.txt\n```"),
        ],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    t.typ("make hello.txt\r");
    // The code block comes last (and loads the grammars on first use).
    t.wait_for("cat hello.txt", SLOW);
    let all = t.all();
    assert!(all.contains("› make hello.txt"), "{all}");
    assert!(all.contains("● write  hello.txt"), "{all}");
    assert!(all.contains("Created hello.txt (1 line)."), "{all}");
    assert!(all.contains("cat hello.txt"), "{all}");
    assert_eq!(
        std::fs::read_to_string(t.project.join("hello.txt")).unwrap(),
        "hi\n"
    );
    // The tool call is committed once, not redrawn into scrollback twice.
    assert_eq!(all.matches("● write  hello.txt").count(), 1, "{all}");
    let out = t.quit();
    assert!(out.contains("session: "), "{out}");
}

#[test]
fn the_approval_card_sits_under_the_input_and_keeps_the_draft() {
    let (mut t, _s) = Tui::start(
        vec![
            call(
                "bash",
                json!({"command": "cargo publish --dry-run", "intent": "check the package"}),
            ),
            text("done"),
        ],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    t.typ("publish it\r");
    t.wait_for("approve bash", SLOW);
    // A draft typed while the card is up isn't lost: the card takes y/s/n/e,
    // and the box keeps its text.
    let screen = t.contents();
    let input = screen.find("› ").unwrap();
    let card = screen.find("approve bash").unwrap();
    assert!(card > input, "the card is under the input box:\n{screen}");
    assert!(screen.contains("intent: check the package"), "{screen}");
    assert!(screen.contains("is on the always-ask list"), "{screen}");
    t.typ("n");
    t.wait_for("denied", SLOW);
    t.wait_for("done", SLOW);
    t.quit();
}

#[test]
fn slash_completion_puts_the_selection_in_the_box() {
    let (mut t, _s) = Tui::start(vec![text("ok")], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("/un");
    t.wait_for("/undo", SLOW);
    t.send(b"\x1b[B"); // Down: the selection goes into the box
    std::thread::sleep(Duration::from_millis(100));
    let screen = t.contents();
    assert!(
        screen.contains("› /undo"),
        "what the box shows is what Enter runs:\n{screen}"
    );
    t.esc(); // Esc closes the list
    t.send(b"\x15"); // Ctrl+U clears the line
    t.quit();
}

#[test]
fn resize_redraws_only_the_live_region() {
    let (mut t, _s) = Tui::start(vec![text("first answer")], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("hi\r");
    t.wait_for("first answer", SLOW);
    // Wider and shorter. Narrowing relies on the terminal reflowing lines,
    // which vt100 doesn't; that case is on the manual QA checklist.
    let ws = rustix::termios::Winsize {
        ws_row: 20,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    rustix::termios::tcsetwinsize(&t.master, ws).unwrap();
    t.screen
        .lock()
        .unwrap()
        .screen_mut()
        .set_size(nz(20), nz(120));
    t.tall
        .lock()
        .unwrap()
        .screen_mut()
        .set_size(nz(5000), nz(120));
    let pid = rustix::process::Pid::from_raw(t.child.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::WINCH).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    t.typ("x");
    t.wait_for("› x", SLOW);
    let all = t.all();
    assert_eq!(
        all.matches("first answer").count(),
        1,
        "committed lines aren't redrawn:\n{all}"
    );
    t.send(b"\x15");
    t.quit();
}

/// §11.3: cold start to interactive under 100 ms (excluding the model) and
/// keystroke to screen under 16 ms. Debug builds get more room; the
/// release numbers are the targets.
#[test]
fn feel_targets() {
    let (mut t, _s) = Tui::start(vec![text("ok")], |_| {});
    let startup = {
        t.wait_for("› ", SLOW);
        t.started.elapsed()
    };
    let mut worst = Duration::ZERO;
    for ch in ["a", "b", "c", "d", "e"] {
        let start = Instant::now();
        t.typ(ch);
        let want = format!("› {}", "abcde".split(ch).next().unwrap().to_owned() + ch);
        t.wait_for(&want, SLOW);
        worst = worst.max(start.elapsed());
    }
    let (start_max, key_max) = if cfg!(debug_assertions) {
        (Duration::from_millis(1000), Duration::from_millis(100))
    } else {
        (Duration::from_millis(100), Duration::from_millis(16))
    };
    eprintln!("startup {startup:?}, worst keystroke {worst:?}");
    assert!(startup < start_max, "startup {startup:?}");
    assert!(worst < key_max, "keystroke {worst:?}");
    t.send(b"\x15");
    t.quit();
}

#[test]
fn steering_esc_and_user_bash() {
    let (mut t, server) = Tui::start(
        vec![
            call("bash", json!({"command": "sleep 1; echo slept"})),
            text("first done"),
            call("bash", json!({"command": "sleep 30"})),
            // The abort comes before this reply is used; the next prompt gets it.
            text("third"),
        ],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    t.typ("go\r");
    t.wait_for("◌ bash", SLOW);
    // Enter while running steers the run: the message goes in before the
    // next request.
    t.typ("also check the logs\r");
    t.wait_for("(steering) also check the logs", SLOW);
    t.wait_for("first done", SLOW);
    let reqs = server.chat_requests();
    let last = reqs[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["content"], "also check the logs", "{last}");
    // Esc aborts a long call.
    t.typ("again\r");
    t.wait_for("◌ bash  sleep 30", SLOW);
    let start = Instant::now();
    t.esc();
    t.wait_for("└ aborted", SLOW);
    assert!(start.elapsed() < Duration::from_secs(5));
    // !cmd runs as the user; its output goes to the model with the next prompt.
    t.typ("!echo from-the-user\r");
    t.wait_for("from-the-user", SLOW);
    t.typ("next\r");
    t.wait_for("third", SLOW);
    let reqs = server.chat_requests();
    let m = reqs.last().unwrap()["messages"].to_string();
    assert!(m.contains("I ran `echo from-the-user`"), "{m}");
    t.quit();
}

#[test]
fn undo_diff_show_stats_and_mode() {
    let (mut t, _s) = Tui::start(
        vec![
            call("write", json!({"path": "notes.md", "text": "one\ntwo\n"})),
            text("wrote it"),
        ],
        |p| std::fs::write(p.join("README.md"), "readme\n").unwrap(),
    );
    t.wait_for("› ", SLOW);
    t.typ("write notes\r");
    t.wait_for("wrote it", SLOW);
    assert!(t.project.join("notes.md").exists());
    t.typ("/diff\r");
    t.wait_for("▌ by write call 1", SLOW);
    t.wait_for("+two", SLOW);
    t.typ("/show 1\r");
    t.wait_for("\"text\": \"one\\ntwo\\n\"", SLOW);
    t.typ("/stats\r");
    t.wait_for("model requests 2, tool calls 1 (0 failed)", SLOW);
    t.typ("/undo\r");
    t.wait_for("undo: 0 files written, 1 deleted", SLOW);
    assert!(!t.project.join("notes.md").exists());
    assert!(t.project.join("README.md").exists());
    t.typ("/mode strict\r");
    t.wait_for("· strict ·", SLOW);
    // @ completion: the list shows matches; Tab puts one in the box.
    t.typ("look at @READ");
    t.wait_for("@README.md", SLOW);
    t.typ("\t");
    t.wait_for("› look at @README.md ", SLOW);
    t.send(b"\x15");
    t.quit();
}

#[test]
fn rewind_picks_a_prompt_and_restores_its_files() {
    let (mut t, _s) = Tui::start(
        vec![
            call("write", json!({"path": "a.txt", "text": "a\n"})),
            text("made a"),
            call("write", json!({"path": "b.txt", "text": "b\n"})),
            text("made b"),
        ],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    t.typ("make a\r");
    t.wait_for("made a", SLOW);
    t.typ("make b\r");
    t.wait_for("made b", SLOW);
    t.typ("/rewind\r");
    t.wait_for("↑↓ choose", SLOW);
    // The newest prompt is selected; go up one to "make a".
    t.send(b"\x1b[A");
    std::thread::sleep(Duration::from_millis(100));
    t.typ("\r");
    t.wait_for("rewind: ", SLOW);
    assert!(!t.project.join("a.txt").exists() && !t.project.join("b.txt").exists());
    t.quit();
}

/// /fork (pi parity): a new session from the history before a prompt,
/// with that prompt back in the editor; the original file stays as it was.
#[test]
fn fork_starts_a_new_session_before_a_prompt() {
    let (mut t, s) = Tui::start(vec![text("one"), text("two"), text("three")], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("first question\r");
    t.wait_for("one", SLOW);
    t.typ("second question\r");
    t.wait_for("two", SLOW);
    t.typ("/fork\r");
    t.wait_for("↑↓ choose", SLOW);
    t.typ("\r"); // the newest prompt
    t.wait_for("forked: ", SLOW);
    t.wait_for("› second question", SLOW);
    t.typ(" again\r");
    t.wait_for("three", SLOW);
    let reqs = s.chat_requests();
    let msgs = reqs[2]["messages"].as_array().unwrap();
    let texts: Vec<&str> = msgs[1..]
        .iter()
        .map(|m| m["content"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(texts, ["first question", "one", "second question again"]);
    let dir = t.project.parent().unwrap().join("home/.psi/sessions");
    let files: Vec<_> = walk(&dir)
        .into_iter()
        .filter(|p| {
            p.extension().is_some_and(|e| e == "jsonl")
                && !p.to_string_lossy().ends_with(".wire.jsonl")
        })
        .collect();
    assert_eq!(files.len(), 2, "{files:?}");
    let forked = files
        .iter()
        .find(|f| std::fs::read_to_string(f).unwrap().contains("forked_from"))
        .expect("the new session records where it came from");
    let original = files.iter().find(|f| *f != forked).unwrap();
    assert!(std::fs::read_to_string(original).unwrap().contains("two"));
    t.quit();
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn example(name: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/extensions")
        .join(name)
        .display()
        .to_string()
}

#[test]
fn extension_dialogs_confirm_select_input_and_custom() {
    let ext = example("ask-first");
    let (mut t, _s) = Tui::start_with(vec![text("ok")], |_| {}, &["--extension", &ext]);
    t.wait_for("› ", SLOW);
    t.typ("/dep");
    t.wait_for("/deploy", SLOW);
    t.esc();
    t.typ("loy\r");
    t.wait_for("ask-first: Deploy?", SLOW);
    t.typ("y");
    t.wait_for("Where to?", SLOW);
    t.typ("\r"); // staging
    t.wait_for("A note for the log", SLOW);
    t.typ("first\r");
    t.wait_for("How fast?", SLOW);
    t.send(b"\x1b[B"); // down to "all at once"
    std::thread::sleep(Duration::from_millis(100));
    t.typ("\r");
    t.wait_for("[ask-first] deploying to staging (all at once)", SLOW);
    t.quit();
}

/// Phase 5: a runaway Lua loop is killed without freezing the UI.
#[test]
fn a_runaway_extension_doesnt_freeze_the_ui() {
    let (mut t, _s) = Tui::start_with(
        vec![text("ok")],
        |p| {
            std::fs::write(
                p.join("spin.lua"),
                r#"return { name = "spin", version = "1", description = "spins",
                  setup = function(psi)
                    psi.register_command("spin", { description = "loop forever", handler = function()
                      while true do pcall(function() while true do end end) end
                    end })
                  end }"#,
            )
            .unwrap();
        },
        &["--extension", "spin.lua"],
    );
    t.wait_for("› ", SLOW);
    t.typ("/spin\r");
    std::thread::sleep(Duration::from_millis(300));
    // Typing still shows at once while the extension spins.
    let start = Instant::now();
    t.typ("still here");
    t.wait_for("› still here", SLOW);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
    // The loop is stopped after the busy limit and the error is shown.
    t.wait_for("ran too long without yielding", Duration::from_secs(15));
    t.send(b"\x15");
    t.quit();
}

#[test]
fn prompt_templates_and_skill_commands() {
    let (mut t, s) = Tui::start(vec![text("hello Bob"), text("read it")], |p| {
        let home = p.parent().unwrap().join("home/.psi");
        std::fs::create_dir_all(home.join("prompts")).unwrap();
        std::fs::create_dir_all(home.join("skills/pdf")).unwrap();
        std::fs::write(
            home.join("prompts/greet.md"),
            "---\ndescription: greet someone\n---\nSay hello to $1.",
        )
        .unwrap();
        std::fs::write(
            home.join("skills/pdf/SKILL.md"),
            "---\ndescription: Read PDF files.\n---\nUse pdftotext.",
        )
        .unwrap();
    });
    t.wait_for("› ", SLOW);
    t.typ("/gr");
    t.wait_for("greet someone", SLOW);
    t.send(b"\x15");
    t.typ("/greet Bob\r");
    t.wait_for("hello Bob", SLOW);
    t.typ("/skill:pdf the report\r");
    t.wait_for("read it", SLOW);
    let all = t.all();
    assert!(all.contains("prompt /greet"), "{all}");
    assert!(all.contains("› [skill pdf] the report"), "{all}");
    assert!(
        !all.contains("Use pdftotext"),
        "the skill body isn't shown:\n{all}"
    );
    let reqs = s.chat_requests();
    let first = reqs[0]["messages"].as_array().unwrap();
    assert!(
        first[0]["content"]
            .as_str()
            .unwrap()
            .contains("- pdf: Read PDF files. ("),
        "skills are listed in the system prompt"
    );
    assert_eq!(first[1]["content"], "Say hello to Bob.");
    let second = reqs[1]["messages"].as_array().unwrap();
    let skill = second.last().unwrap()["content"].as_str().unwrap();
    assert!(
        skill.starts_with("<skill name=\"pdf\"") && skill.ends_with("</skill>\n\nthe report"),
        "{skill}"
    );
    t.quit();
}

/// The `input` event (pi parity): a handler consumes "ping" (and answers
/// with a notification) and rewrites "hi" into a prompt template, which
/// then expands. Commands still go first.
#[test]
fn input_handlers_rewrite_or_consume_typed_text() {
    let (mut t, s) = Tui::start_with(
        vec![text("hello there")],
        |p| {
            let home = p.parent().unwrap().join("home/.psi");
            std::fs::create_dir_all(home.join("prompts")).unwrap();
            std::fs::write(home.join("prompts/greet.md"), "Say hello to $1.").unwrap();
            std::fs::write(
                p.join("router.lua"),
                r#"return { name = "router", version = "1", description = "d", setup = function(psi)
                  psi.on("input", function(ev, ctx)
                    if ev.text == "ping" then ctx.ui.notify("pong"); return { handled = true } end
                    if ev.text == "hi" then return { text = "/greet " .. ev.source } end
                  end)
                end }"#,
            )
            .unwrap();
        },
        &["--extension", "router.lua"],
    );
    t.wait_for("› ", SLOW);
    t.typ("ping\r");
    t.wait_for("pong", SLOW);
    t.typ("hi\r");
    t.wait_for("hello there", SLOW);
    let reqs = s.chat_requests();
    assert_eq!(reqs.len(), 1, "ping never reached the model");
    let msgs = reqs[0]["messages"].as_array().unwrap();
    assert_eq!(msgs.last().unwrap()["content"], "Say hello to interactive.");
    t.quit();
}

#[test]
fn plan_mode_proposes_and_carries_out_a_plan() {
    let (mut t, s) = Tui::start(
        vec![
            text("Plan:\n1. Create notes.txt\n2. Fill it in"),
            call("write", json!({"path": "notes.txt", "text": "n\n"})),
            text("Made the file. [DONE:1]"),
        ],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    t.typ("/plan\r");
    t.wait_for("plan mode: read-only", SLOW);
    t.typ("write notes\r");
    t.wait_for("what next?", SLOW);
    let screen = t.contents();
    assert!(screen.contains("plan: 2 steps"), "{screen}");
    assert!(screen.contains("Carry out the plan"), "{screen}");
    t.send(b"\r");
    t.wait_for("Made the file.", SLOW);
    t.wait_for("plan 1/2", SLOW);
    let screen = t.contents();
    assert!(screen.contains("✓ Create notes.txt"), "{screen}");
    assert!(screen.contains("○ Fill it in"), "{screen}");
    assert!(t.project.join("notes.txt").exists());
    let reqs = s.chat_requests();
    assert_eq!(
        reqs[0]["tools"].as_array().unwrap().len(),
        2,
        "read and bash only"
    );
    let exec = reqs[1]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(exec.starts_with("Carry out the plan."), "{exec}");
    t.quit();
}

#[test]
fn keybindings_file_rebinds_actions() {
    let (mut t, _s) = Tui::start(vec![], |p| {
        let home = p.parent().unwrap().join("home/.psi");
        std::fs::write(
            home.join("keybindings.json"),
            r#"{ "app.plan.toggle": "ctrl+g", "tui.editor.cursorLineStart": [], "bogus": "x" }"#,
        )
        .unwrap();
    });
    t.wait_for("unknown action 'bogus'", SLOW);
    t.send(b"\x07"); // Ctrl+G
    t.wait_for("plan mode: read-only", SLOW);
    t.typ("/help\r");
    t.wait_for("plan mode on or off", SLOW);
    let all = t.all();
    assert!(all.contains("Ctrl+g"), "{all}");
    t.quit();
}

#[test]
fn themes_color_the_screen() {
    let (mut t, _s) = Tui::start(vec![text("ok")], |p| {
        let home = p.parent().unwrap().join("home/.psi");
        std::fs::create_dir_all(home.join("themes")).unwrap();
        std::fs::write(
            home.join("themes/mine.json"),
            r##"{"name": "mine", "colors": {"userMessageText": 196, "accent": "#00ff00", "syntaxKeyword": "#123456"}}"##,
        )
        .unwrap();
        std::fs::write(home.join("settings.jsonc"), r#"{"theme": "mine"}"#).unwrap();
    });
    t.wait_for("› ", SLOW);
    t.typ("hi\r");
    t.wait_for("ok", SLOW);
    {
        let tall = t.tall.lock().unwrap();
        let screen = tall.screen();
        let row = (0..200)
            .find(|r| screen.contents_between(*r, 0, *r, 10).starts_with("› hi"))
            .expect("the prompt line");
        assert_eq!(
            screen.cell(row, 0).unwrap().fgcolor(),
            vt100::Color::Idx(196)
        );
    }
    t.typ("/theme\r");
    t.wait_for("themes: default, mine (now)", SLOW);
    t.typ("/theme nope\r");
    t.wait_for("no theme named 'nope'", SLOW);
    t.quit();
}

#[test]
fn a_cut_answer_and_its_retry_dont_run_together() {
    let (mut t, _s) = Tui::start(
        vec![Reply::cut("The answer is"), text("The answer is 42.")],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    t.typ("what is it?\r");
    t.wait_for("42.", SLOW);
    let all = t.all();
    assert!(
        all.contains("the server closed the stream before the answer finished; trying again"),
        "{all}"
    );
    assert!(!all.contains("The answer isThe"), "{all}");
    assert_eq!(all.matches("The answer is").count(), 1, "{all}");
    t.quit();
}
