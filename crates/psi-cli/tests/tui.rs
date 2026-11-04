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
    /// Every byte psi wrote, for sequences the screen doesn't keep (titles,
    /// notifications).
    raw: Arc<Mutex<Vec<u8>>>,
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
        // Not inside the psi repository, whose target/ is gitignored.
        Self::start_at(tempfile::tempdir().unwrap(), replies, setup, args)
    }

    /// As [`Self::start_with`], in `tmp`'s `home` and `project`, which may
    /// hold earlier sessions (`-c`, `/resume`).
    fn start_at(
        tmp: tempfile::TempDir,
        replies: Vec<Reply>,
        setup: impl FnOnce(&std::path::Path),
        args: &[&str],
    ) -> (Self, ReplayServer) {
        let server = ReplayServer::start(replies).unwrap();
        let home = tmp.path().join("home");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(home.join(".psi")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        // The pager for long command output keeps what it was given in
        // home/paged.txt; a test may rewrite the script.
        let pager = home.join("pager.sh");
        std::fs::write(&pager, "#!/bin/sh\ncat > \"$HOME/paged.txt\"\n").unwrap();
        std::fs::set_permissions(&pager, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
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
            // Ctrl+G's editor: a script a test may write, never the developer's own.
            .env("VISUAL", home.join("editor.sh"))
            .env_remove("EDITOR")
            .env("PAGER", home.join("pager.sh"))
            .env_remove("LESS")
            // The terminal is the test's, not the one the tests run in.
            .env_remove("TMUX")
            .env_remove("KITTY_WINDOW_ID")
            .env_remove("LC_TERMINAL")
            .env_remove("TERM_PROGRAM")
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
        let raw = Arc::new(Mutex::new(Vec::new()));
        let (s, t2, r2) = (screen.clone(), tall.clone(), raw.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                // Both under their locks at once, so text seen on one is on
                // the other (a test that waits on the screen and then reads
                // the scrollback failed under load on macOS).
                let (mut a, mut b) = (s.lock().unwrap(), t2.lock().unwrap());
                a.process(&buf[..n]);
                b.process(&buf[..n]);
                r2.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        let tui = Self {
            _tmp: tmp,
            project,

            master: std::fs::File::from(master),
            screen,
            tall,
            raw,
            child,
            started,
        };
        (tui, server)
    }

    /// What the pager was given last (home/paged.txt).
    fn paged(&self) -> String {
        std::fs::read_to_string(self.project.parent().unwrap().join("home/paged.txt"))
            .unwrap_or_default()
    }

    fn wait_paged(&self, what: &str, max: Duration) {
        let start = Instant::now();
        while start.elapsed() < max {
            if self.paged().contains(what) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "{what:?} never reached the pager; it got:\n{}",
            self.paged()
        );
    }

    fn contents(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }

    /// Everything printed so far, as on a terminal that never scrolls.
    fn all(&self) -> String {
        self.tall.lock().unwrap().screen().contents()
    }

    /// Waits until `what` is on the screen as it is now (not scrollback).
    fn wait_screen(&self, what: &str, max: Duration) {
        let start = Instant::now();
        while start.elapsed() < max {
            if self.contents().contains(what) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!(
            "waited {max:?} for {what:?} on screen:\n{}",
            self.contents()
        );
    }

    /// Waits until `what` has been printed `n` times in all: for text that
    /// is already in the scrollback once.
    fn wait_count(&self, what: &str, n: usize, max: Duration) {
        let start = Instant::now();
        while start.elapsed() < max {
            if self.all().matches(what).count() >= n {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!(
            "waited {max:?} for {what:?} {n} times; all:\n{}",
            self.all()
        );
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

    fn raw(&self) -> String {
        String::from_utf8_lossy(&self.raw.lock().unwrap()).into_owned()
    }

    /// Waits until psi has written `what` (raw bytes, escapes included).
    fn wait_raw(&self, what: &str, max: Duration) {
        let start = Instant::now();
        while start.elapsed() < max {
            if self.raw().contains(what) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("psi never wrote {what:?}; it wrote:\n{:?}", self.raw());
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
    assert!(all.contains("▌ make hello.txt"), "{all}");
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
    // What the box shows is what Enter runs.
    t.wait_screen("› /undo", SLOW);
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
    // The new width is used: 110 characters fit on one row of the box (at
    // the old 100 they would wrap).
    let long = "x".repeat(110);
    t.typ(&long);
    t.wait_screen(&format!("› {long}"), SLOW);
    let all = t.all();
    assert_eq!(
        all.matches("first answer").count(),
        1,
        "committed lines aren't redrawn:\n{all}"
    );
    t.send(b"\x15");
    t.quit();
}

/// A resize whose event never arrives (crossterm drops one that comes with
/// input, seen on macOS) is caught at the next key.
#[test]
fn a_lost_resize_is_caught_at_the_next_key() {
    let (mut t, _s) = Tui::start(vec![], |_| {});
    t.wait_for("› ", SLOW);
    let ws = rustix::termios::Winsize {
        ws_row: 20,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // No SIGWINCH: psi isn't in the pty's session, so none is sent.
    rustix::termios::tcsetwinsize(&t.master, ws).unwrap();
    t.screen
        .lock()
        .unwrap()
        .screen_mut()
        .set_size(nz(20), nz(120));
    let long = "x".repeat(110);
    t.typ(&long);
    t.wait_screen(&format!("› {long}"), SLOW);
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
    // (Output only the command's result has: the command line itself is on
    // screen at once, and the command runs off the UI thread.)
    t.typ("!printf 'from-the-%s' user\r");
    t.wait_for("from-the-user", SLOW);
    t.typ("next\r");
    t.wait_for("third", SLOW);
    let reqs = server.chat_requests();
    let m = reqs.last().unwrap()["messages"].to_string();
    assert!(m.contains("I ran `printf 'from-the-%s' user`"), "{m}");
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
    t.wait_paged("▌ by write call 1", SLOW);
    t.wait_paged("+two", SLOW);
    t.typ("/show 1\r");
    t.wait_paged("\"text\": \"one\\ntwo\\n\"", SLOW);
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
    // The prompt is back in the editor, under the committed one.
    t.wait_for("▌ second question", SLOW);
    t.wait_screen("› second question", SLOW);
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
    assert!(all.contains("▌ [skill pdf] the report"), "{all}");
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
    // Wait for each part: the card can reach the screen in pieces.
    t.wait_screen("plan: 2 steps", SLOW);
    t.wait_screen("Carry out the plan", SLOW);
    t.send(b"\r");
    t.wait_for("Made the file.", SLOW);
    t.wait_for("plan 1/2", SLOW);
    t.wait_screen("✓ Create notes.txt", SLOW);
    t.wait_screen("○ Fill it in", SLOW);
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
    t.wait_paged("Ctrl+g", SLOW);
    t.quit();
}

#[test]
fn command_output_goes_to_the_pager() {
    let (mut t, _s) = Tui::start(vec![], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("/help\r");
    t.wait_paged("the session as an inspection timeline", SLOW);
    // The colors go to the pager too, and nothing reaches the scrollback.
    assert!(t.paged().contains("\x1b["));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!t.all().contains("the session as an inspection timeline"));
    // Other commands' output is committed as before.
    t.typ("/todos\r");
    t.wait_for("no todos on this branch", SLOW);
    // A pager that can't run: the output is committed, with a note.
    let pager = t.project.parent().unwrap().join("home/pager.sh");
    std::fs::write(&pager, "#!/bin/sh\nexit 127\n").unwrap();
    t.typ("/help\r");
    t.wait_for("could not run the pager", SLOW);
    assert!(t.all().contains("the session as an inspection timeline"));
    t.quit();
}

#[test]
fn ctrl_g_edits_the_prompt_in_the_editor() {
    let (mut t, _s) = Tui::start(vec![text("got it")], |p| {
        let script = p.parent().unwrap().join("home/editor.sh");
        std::fs::write(&script, "#!/bin/sh\nprintf ' and more\\n' >> \"$1\"\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    });
    t.wait_for("› ", SLOW);
    t.typ("draft");
    t.wait_for("draft", SLOW);
    t.send(b"\x07"); // Ctrl+G
    t.wait_for("draft and more", SLOW);
    t.typ("\r");
    t.wait_for("got it", SLOW);
    // A failing editor leaves the prompt as it was.
    let script = t.project.parent().unwrap().join("home/editor.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 3\n").unwrap();
    t.typ("keep me");
    t.send(b"\x07");
    t.wait_for("exit status: 3; the prompt is unchanged", SLOW);
    assert!(t.contents().contains("keep me"));
    t.send(b"\x03"); // clear the input so Ctrl+D quits
    t.quit();
}

#[test]
fn title_progress_and_a_notification_when_unfocused() {
    let (mut t, _s) = Tui::start(vec![text("all done")], |_| {});
    t.wait_raw("\x1b]2;ψ project\x1b\\", SLOW);
    t.send(b"\x1b[O"); // focus lost
    t.typ("go\r");
    t.wait_for("all done", SLOW);
    t.wait_raw("\x07\x1b]777;notify;psi · project;done\x1b\\", SLOW);
    let raw = t.raw();
    let working = raw
        .find("\x1b]9;4;3\x1b\\")
        .expect("progress while working");
    assert!(raw.contains("\x1b]2;ψ project · working\x1b\\"));
    assert!(
        raw[working..].contains("\x1b]9;4;0\x1b\\"),
        "progress removed when idle"
    );
    // Focused again: no notification for the next turn.
    t.send(b"\x1b[I");
    let all = t.quit();
    assert!(all.contains("all done"));
}

#[test]
fn file_paths_and_markdown_links_are_clickable() {
    let (mut t, _s) = Tui::start(
        vec![
            call("read", json!({"path": "notes.txt"})),
            text("See [the docs](https://example.com/docs)."),
        ],
        |p| std::fs::write(p.join("notes.txt"), "hello\n").unwrap(),
    );
    t.wait_for("› ", SLOW);
    t.typ("read it\r");
    t.wait_for("the docs", SLOW);
    let raw = t.raw();
    let file = format!("\x1b]8;;file://{}/notes.txt\x1b\\", t.project.display());
    assert!(raw.contains(&file), "no file link in {raw:?}");
    assert!(raw.contains("\x1b]8;;https://example.com/docs\x1b\\"));
    t.quit();
}

#[test]
fn ctrl_r_finds_an_earlier_prompt_and_ctrl_s_stashes_the_draft() {
    let (mut t, _s) = Tui::start(vec![text("one"), text("two")], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("first question about parsers\r");
    t.wait_for("one", SLOW);
    t.typ("second question about lexers\r");
    t.wait_for("two", SLOW);
    t.send(b"\x12"); // Ctrl+R
    t.wait_for("history: ", SLOW);
    t.typ("pars");
    t.wait_for("history: pars", SLOW);
    // The committed prompt is on screen once; the box makes it twice.
    let count = |t: &Tui| {
        let c = t.contents();
        c.matches("▌ first question about parsers").count()
            + c.matches("› first question about parsers").count()
    };
    let wait_count = |t: &Tui, n: usize| {
        let start = Instant::now();
        while count(t) != n && start.elapsed() < SLOW {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(count(t), n, "{}", t.contents());
    };
    wait_count(&t, 1);
    t.typ("\r");
    wait_count(&t, 2);
    t.send(b"\x13"); // Ctrl+S
    t.wait_for("draft stashed", SLOW);
    wait_count(&t, 1);
    t.send(b"\x13");
    wait_count(&t, 2);
    t.send(b"\x03");
    t.quit();
}

#[test]
fn a_long_paste_shows_as_a_placeholder_and_is_sent_whole() {
    let (mut t, s) = Tui::start(vec![text("got the log")], |_| {});
    t.wait_for("› ", SLOW);
    let log: String = (0..20).map(|i| format!("log line {i}\n")).collect();
    t.typ("explain this: ");
    t.send(format!("\x1b[200~{log}\x1b[201~").as_bytes());
    t.wait_for("[Pasted text #1 +20 lines]", SLOW);
    // Hidden tag characters spelling "hi" are dropped from a paste.
    t.send("\x1b[200~ ok\u{E0068}\u{E0069}\x1b[201~".as_bytes());
    t.wait_for("removed 2 invisible characters from the paste", SLOW);
    t.typ("\r");
    t.wait_for("got the log", SLOW);
    let reqs = s.chat_requests();
    let sent = reqs[0]["messages"].to_string();
    assert!(
        sent.contains("log line 0") && sent.contains("log line 19"),
        "{sent}"
    );
    assert!(!sent.contains("Pasted text"), "{sent}");
    assert!(!sent.contains('\u{E0068}'));
    t.quit();
}

#[test]
fn copy_puts_the_answer_or_its_code_on_the_clipboard() {
    let (mut t, _s) = Tui::start(vec![text("Run this:\n\n```sh\nls\n```")], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("how?\r");
    t.wait_for("Run this", SLOW);
    t.typ("/copy code\r");
    t.wait_for("copied the last code block (1 line)", SLOW);
    t.wait_raw("\x1b]52;c;bHM=\x1b\\", SLOW); // "ls"
    t.typ("/copy\r");
    t.wait_for("copied the last answer (5 lines)", SLOW);
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
            .find(|r| screen.contents_between(*r, 0, *r, 10).starts_with("▌ hi"))
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

#[test]
fn a_slow_bang_command_keeps_the_screen_live_and_esc_stops_it() {
    let (mut t, _s) = Tui::start(vec![], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("!echo $$ > bang.pid; exec sleep 30\r");
    // Typing shows while it runs: the screen isn't frozen.
    t.typ("still here");
    let took = t.wait_for("still here", Duration::from_secs(2));
    assert!(took < Duration::from_secs(2), "{took:?}");
    let pid_file = t.project.join("bang.pid");
    let start = Instant::now();
    while !pid_file.exists() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    t.esc();
    t.wait_for("stopped the command", SLOW);
    t.wait_for("[stopped]", SLOW);
    let gone = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        // ps, not /proc: macOS has no /proc. A zombie counts as gone.
        let ps = Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&ps.stdout);
        !ps.status.success() || stat.trim().is_empty() || stat.trim().starts_with('Z')
    });
    assert!(gone, "the command was left running");
    t.typ("\x03"); // Ctrl+C clears the typed text, so Ctrl+D quits
    t.quit();
}

/// Runs `psi -p <prompt>` in `tmp`'s home and project (sessions for `-c`
/// and `/resume`), against its own replay server.
fn print_run(tmp: &std::path::Path, prompt: &str, answer: &str) {
    print_run_with(tmp, prompt, vec![text(answer)]);
}

/// As [`print_run`], with the model's replies scripted.
fn print_run_with(tmp: &std::path::Path, prompt: &str, replies: Vec<Reply>) {
    let server = ReplayServer::start(replies).unwrap();
    let home = tmp.join("home");
    let project = tmp.join("project");
    std::fs::create_dir_all(home.join(".psi")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let models =
        json!({"models": [{"id": "local", "base_url": server.base_url(), "max_output": 256}]});
    std::fs::write(home.join(".psi/models.jsonc"), models.to_string()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_psi"))
        .args(["-p", prompt])
        .env("HOME", &home)
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn strict_project(p: &std::path::Path) {
    std::fs::create_dir_all(p.join(".psi")).unwrap();
    std::fs::write(p.join(".psi/settings.jsonc"), r#"{"mode": "strict"}"#).unwrap();
}

#[test]
fn trusting_a_project_loads_its_settings() {
    let (mut t, _s) = Tui::start(vec![], strict_project);
    t.wait_for("Load it? [y/N]", SLOW);
    t.typ("y\r");
    t.wait_for("· strict ·", SLOW);
    let home = t.project.parent().unwrap().join("home");
    assert!(home.join(".psi/trust.json").exists());
    t.quit();
}

#[test]
fn not_trusting_a_project_leaves_its_settings_out() {
    let (mut t, _s) = Tui::start(vec![], strict_project);
    t.wait_for("Load it? [y/N]", SLOW);
    t.typ("n\r");
    // (Its "not loading" line goes out before the screen starts.)
    t.wait_for("· auto ·", SLOW);
    let home = t.project.parent().unwrap().join("home");
    assert!(!home.join(".psi/trust.json").exists());
    t.quit();
}

#[test]
fn the_approval_card_allows_once_for_the_session_or_denies_with_a_reason() {
    let push = || call("bash", json!({"command": "git push --dry-run"}));
    let (mut t, s) = Tui::start(
        vec![
            push(),
            text("one"),
            push(),
            push(),
            text("two"),
            push(),
            text("three"),
        ],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    // y: this call only.
    t.typ("first\r");
    t.wait_for("approve bash", SLOW);
    t.typ("y");
    t.wait_for("allowed once", SLOW);
    t.wait_for("one", SLOW);
    // s: the rest of the session; the next push runs without a card.
    // (The card is drawn in place and goes when answered.)
    t.typ("second\r");
    t.wait_screen("approve bash", SLOW);
    t.typ("s");
    t.wait_for("allowed for this session", SLOW);
    t.wait_for("two", SLOW);
    let all = t.all();
    assert_eq!(all.matches("● bash  git push").count(), 3, "{all}");
    assert_eq!(all.matches("  allowed ").count(), 2, "{all}");
    // A new session (grants don't carry), then e: denied with a reason the
    // model reads.
    t.typ("/new\r");
    t.wait_for("new session", SLOW);
    t.typ("third\r");
    t.wait_screen("approve bash", SLOW);
    t.typ("e");
    t.typ("use the staging remote\r");
    t.wait_for("denied: use the staging remote", SLOW);
    t.wait_for("three", SLOW);
    let last = s.chat_requests().last().unwrap()["messages"].to_string();
    assert!(last.contains("use the staging remote"), "{last}");
    t.quit();
}

#[test]
fn continue_and_resume_bring_back_a_session() {
    let tmp = tempfile::tempdir().unwrap();
    print_run(tmp.path(), "alpha question", "alpha answer");
    let (mut t, s) = Tui::start_at(tmp, vec![text("more")], |_| {}, &["-c"]);
    // -c replays the history and continues from it.
    t.wait_for("▌ alpha question", SLOW);
    t.wait_for("alpha answer", SLOW);
    t.typ("and then?\r");
    t.wait_for("more", SLOW);
    let m = s.chat_requests()[0]["messages"].clone();
    assert_eq!(m[1]["content"], "alpha question");
    assert_eq!(m[2]["content"], "alpha answer");
    // /new, then /resume picks a session back.
    t.typ("/new\r");
    t.wait_for("new session", SLOW);
    t.typ("/resume\r");
    t.wait_for("↑↓ choose", SLOW);
    t.typ("\r");
    t.wait_for("resumed", SLOW);
    t.wait_count("▌ alpha question", 2, SLOW);
    t.quit();
}

/// The color of the `●` before the committed call whose line starts with `head`.
fn mark_color(t: &Tui, head: &str) -> vt100::Color {
    let tall = t.tall.lock().unwrap();
    let screen = tall.screen();
    let row = (0..5000)
        .find(|r| screen.contents_between(*r, 0, *r, 60).starts_with(head))
        .unwrap_or_else(|| panic!("no line starting {head:?}:\n{}", screen.contents()));
    screen.cell(row, 0).unwrap().fgcolor()
}

#[test]
fn the_stats_sit_under_the_input_box_and_user_text_is_white() {
    let (mut t, _s) = Tui::start(vec![text("ok")], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("hi there\r");
    t.wait_for("ok", SLOW);
    t.wait_screen("local", SLOW);
    let screen = t.contents();
    let rows: Vec<&str> = screen.lines().collect();
    let input = rows
        .iter()
        .position(|r| r.starts_with("› "))
        .expect("the input");
    // A line above and below the box, then the stats.
    assert!(rows[input - 1].starts_with("───"), "{screen}");
    assert!(rows[input + 1].starts_with("───"), "{screen}");
    assert!(rows[input + 2].starts_with("local"), "{screen}");
    let tall = t.tall.lock().unwrap();
    let s = tall.screen();
    let row = (0..200)
        .find(|r| s.contents_between(*r, 0, *r, 12).starts_with("▌ hi there"))
        .expect("the committed prompt");
    // The words are bright white and bold.
    assert_eq!(s.cell(row, 2).unwrap().fgcolor(), vt100::Color::Idx(15));
    assert!(s.cell(row, 2).unwrap().bold());
}

#[test]
fn the_tui_has_no_request_limit_by_default() {
    // 2026-11-03: a long cog run stopped at 40 model requests, the print
    // runs' backstop; the TUI has none unless --max-turns asks for one.
    let mut replies: Vec<Reply> = (0..45)
        .map(|_| call("read", json!({"path": "a.txt"})))
        .collect();
    replies.push(text("all done"));
    let (mut t, s) = Tui::start(replies, |p| std::fs::write(p.join("a.txt"), "x\n").unwrap());
    t.wait_for("› ", SLOW);
    t.typ("read it a lot\r");
    t.wait_for("all done", Duration::from_secs(30));
    assert!(!t.all().contains("stopped:"), "{}", t.all());
    assert_eq!(s.chat_requests().len(), 46);
    t.quit();
}

#[test]
fn starts_without_a_model_server_and_connects_when_it_is_up() {
    // A port nothing listens on yet.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (mut t, _unused) = Tui::start(vec![], move |p| {
        let models = json!({"models": [{"id": "local", "base_url": format!("http://127.0.0.1:{port}"), "max_output": 256}]});
        std::fs::write(
            p.parent().unwrap().join("home/.psi/models.jsonc"),
            models.to_string(),
        )
        .unwrap();
    });
    t.wait_for("no model server at http://127.0.0.1", SLOW);
    t.wait_screen("no model server", SLOW);
    // Commands work; a prompt stays in the box instead of being sent.
    t.typ("/help\r");
    t.wait_paged("/resume", SLOW);
    t.typ("hello there\r");
    t.wait_for("send again", SLOW);
    t.wait_screen("› hello there", SLOW);
    assert!(!t.all().contains("▌ hello there"), "{}", t.all());
    // The server comes up: Psi connects, and the prompt goes through.
    let server = psi_ai::replay::ReplayServer::start_on(port, vec![text("hi back")]).unwrap();
    t.wait_for("connected to the model server", SLOW);
    t.typ("\r");
    t.wait_for("hi back", SLOW);
    assert!(t.all().contains("▌ hello there"), "{}", t.all());
    let screen = t.contents();
    let status = screen
        .lines()
        .rfind(|l| l.starts_with("local"))
        .unwrap_or("");
    assert!(!status.contains("no model server"), "{screen}");
    assert_eq!(server.chat_requests().len(), 1);
    t.quit();
}

#[test]
fn the_input_box_is_pinned_to_the_bottom() {
    let (mut t, _s) = Tui::start(
        vec![call("read", json!({"path": "a.txt"})), text("ok")],
        |p| std::fs::write(p.join("a.txt"), "x\n").unwrap(),
    );
    let at_bottom = |t: &Tui| {
        let screen = t.contents();
        let rows: Vec<&str> = screen.lines().collect();
        // The stats line is the screen's last row, the box two above it.
        rows.len() == usize::from(H) && rows[usize::from(H) - 3].starts_with("› ")
    };
    t.wait_for("› ", SLOW);
    t.wait_screen("local", SLOW);
    // From the start, with the banner at the top and empty rows between.
    assert!(at_bottom(&t), "{}", t.contents());
    t.typ("read it\r");
    t.wait_for("ok", SLOW);
    t.wait_screen("checkpoint", SLOW);
    assert!(at_bottom(&t), "{}", t.contents());
    t.quit();
}

#[test]
fn call_marks_show_how_each_call_went() {
    let (mut t, _s) = Tui::start(
        vec![
            call("read", json!({"path": "here.txt"})),
            call("read", json!({"path": "missing.txt"})),
            text("done"),
        ],
        |p| std::fs::write(p.join("here.txt"), "x\n").unwrap(),
    );
    t.wait_for("› ", SLOW);
    t.typ("read them\r");
    t.wait_for("done", SLOW);
    // Green when the call worked, red when it failed (the 16 colors).
    assert_eq!(mark_color(&t, "● read  here.txt"), vt100::Color::Idx(2));
    assert_eq!(mark_color(&t, "● read  missing.txt"), vt100::Color::Idx(1));
    t.quit();
}

#[test]
fn a_resumed_session_shows_its_tool_calls() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("project")).unwrap();
    std::fs::write(tmp.path().join("project/notes.txt"), "a\nb\n").unwrap();
    print_run_with(
        tmp.path(),
        "read the notes",
        vec![
            call("read", json!({"path": "notes.txt"})),
            call("read", json!({"path": "gone.txt"})),
            text("The notes say a and b."),
        ],
    );
    let (mut t, _s) = Tui::start_at(tmp, vec![text("more")], |_| {}, &["-c"]);
    t.wait_for("The notes say a and b.", SLOW);
    let all = t.all();
    let at = |s: &str| all.find(s).unwrap_or_else(|| panic!("no {s:?}:\n{all}"));
    // In order, with the outcome of each call, as when it ran.
    assert!(
        at("▌ read the notes") < at("● read  notes.txt  #1"),
        "{all}"
    );
    assert!(
        at("● read  notes.txt  #1") < at("● read  gone.txt  #2"),
        "{all}"
    );
    assert!(
        at("● read  gone.txt  #2") < at("The notes say a and b."),
        "{all}"
    );
    assert!(all.contains("└ 2 lines"), "{all}");
    assert_eq!(mark_color(&t, "● read  notes.txt"), vt100::Color::Idx(2));
    assert_eq!(mark_color(&t, "● read  gone.txt"), vt100::Color::Idx(1));
    // /show finds a call from the earlier run by its number.
    t.typ("/show 1\r");
    t.wait_paged("notes.txt", SLOW);
    t.quit();
}

#[test]
fn session_commands_answer() {
    let (mut t, _s) = Tui::start(vec![text("hello")], |_| {});
    t.wait_for("› ", SLOW);
    t.typ("hi\r");
    t.wait_for("hello", SLOW);
    t.typ("/label start\r");
    t.wait_for("labelled", SLOW);
    t.typ("/compact\r");
    t.wait_for("nothing to compact", SLOW);
    t.typ("/mcp\r");
    t.wait_for("no MCP servers", SLOW);
    t.typ("/todos\r");
    t.wait_for("todo", SLOW);
    t.typ("/inspect\r");
    t.wait_paged("step 1", SLOW);
    t.typ("/tree\r");
    t.wait_for("↑↓ choose", SLOW);
    t.esc();
    t.quit();
}

#[test]
fn a_paste_is_one_edit_not_a_submit() {
    let (mut t, s) = Tui::start(vec![text("got it")], |_| {});
    t.wait_for("› ", SLOW);
    // Bracketed paste of two lines: the newline doesn't send.
    t.send(b"\x1b[200~line one\nline two\x1b[201~");
    t.wait_for("line two", SLOW);
    std::thread::sleep(Duration::from_millis(100));
    assert!(s.chat_requests().is_empty(), "the paste was sent");
    t.typ("\r");
    t.wait_for("got it", SLOW);
    let m = s.chat_requests()[0]["messages"].clone();
    assert_eq!(
        m.as_array().unwrap().last().unwrap()["content"],
        "line one\nline two"
    );
    t.quit();
}

#[test]
fn thinking_never_reaches_the_screen() {
    let (mut t, _s) = Tui::start(
        vec![Reply::Chat(ScriptedReply {
            reasoning: "SECRET-THOUGHT about the answer".into(),
            ..ScriptedReply::text("the visible answer")
        })],
        |_| {},
    );
    t.wait_for("› ", SLOW);
    t.typ("hi\r");
    t.wait_for("the visible answer", SLOW);
    assert!(!t.all().contains("SECRET-THOUGHT"), "{}", t.all());
    t.quit();
}

const UI_EXT: &str = r#"return { name = "uibits", version = "1", description = "ui pieces",
  setup = function(psi)
    psi.register_shortcut("ctrl+g", { description = "say hi", handler = function(ctx)
      psi.ui.notify("shortcut fired")
    end })
    psi.register_tool_renderer("bash", function(call, width)
      return { "PREFIX> " .. call.args.command }
    end)
    psi.on("agent_start", function(ev)
      psi.ui.status("uibits", "status-item")
      psi.ui.widget("uibits", { "widget-line" })
    end)
  end }"#;

#[test]
fn extension_shortcuts_renderers_status_widgets_and_reload() {
    let (mut t, _s) = Tui::start_with(
        vec![
            call("bash", json!({"command": "echo one"})),
            text("ok one"),
            call("bash", json!({"command": "echo two"})),
            text("ok two"),
        ],
        |p| std::fs::write(p.join("uibits.lua"), UI_EXT).unwrap(),
        &["--extension", "uibits.lua"],
    );
    t.wait_for("› ", SLOW);
    // A shortcut runs its handler.
    t.send(b"\x07");
    t.wait_for("shortcut fired", SLOW);
    // A run: the renderer draws the call, the status line and the widget
    // show what the extension set.
    t.typ("go\r");
    t.wait_for("ok one", SLOW);
    t.wait_for("PREFIX> echo one", SLOW);
    t.wait_screen("status-item", SLOW);
    t.wait_screen("widget-line", SLOW);
    // /reload picks up a changed extension.
    std::fs::write(
        t.project.join("uibits.lua"),
        UI_EXT.replace("PREFIX> ", "CHANGED> "),
    )
    .unwrap();
    t.typ("/reload\r");
    t.typ("again\r");
    t.wait_for("ok two", SLOW);
    t.wait_for("CHANGED> echo two", SLOW);
    t.quit();
}
