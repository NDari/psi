//! The UI thread: input and drawing never wait on the agent (§3.3). Agent
//! events, approvals and command results arrive as messages.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use psi_agent::approval::{Approval, ApprovalRequest, BlockedChoice, BlockedRequest};
use psi_agent::plan::PlanState;
use psi_agent::recovery::Change;
use psi_agent::{AgentEvent, Queues, RunOutcome, Stop};
use psi_ext::{ExtManager, Request as ExtRequest};
use psi_safety::policy::Mode;
use psi_session::{Entry, EntryBody, Role, text_of};
use psi_tui::theme::{self, Token};
use psi_tui::{Action, Editor, EditorEvent, Keymap, Line, MdStream, Renderer, Style, render_block};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::complete::Completer;
use super::view::{
    self, Status, approval_card, call_block, call_header, dim, git_diff_lines, list, status_line,
};
use super::worker::Cmd;

/// What reaches the UI thread.
pub enum UiMsg {
    Agent(AgentEvent),
    Approve(ApprovalRequest, oneshot::Sender<Approval>),
    Blocked(BlockedRequest, oneshot::Sender<BlockedChoice>),
    Pause(String, oneshot::Sender<bool>),
    RunDone(RunOutcome),
    Info(String),
    Error(String),
    /// The `input` event's answer: the text to send on, or None when a
    /// handler consumed it.
    Input {
        text: Option<String>,
        follow_up: bool,
    },
    Restored {
        what: String,
        written: usize,
        deleted: usize,
        left_alone: Vec<String>,
    },
    Moved(String),
    Diff(Vec<Change>),
    Entries {
        entries: Vec<Entry>,
        leaf: Option<String>,
    },
    Switched {
        what: String,
        path: PathBuf,
        entries: Vec<Entry>,
        /// Text for the editor (/fork puts the prompt back).
        prefill: Option<String>,
    },
    /// `!cmd` finished.
    UserBash {
        command: String,
        output: String,
        to_context: bool,
    },
    /// An extension's request (§9.7).
    Ext(String, ExtRequest),
}

/// An extension's dialog under the input box.
enum ExtModal {
    Confirm {
        ext: String,
        title: String,
        message: String,
        reply: oneshot::Sender<bool>,
    },
    Input {
        ext: String,
        title: String,
        text: String,
        reply: oneshot::Sender<Option<String>>,
    },
    /// A custom component, drawn by the extension each frame under a
    /// deadline; the last frame stays when it is late.
    Custom {
        ext: String,
        id: u64,
        reply: oneshot::Sender<Option<Value>>,
        last: Vec<Line>,
    },
}

/// How long a frame waits for an extension's component or renderer.
const EXT_FRAME: Duration = Duration::from_millis(8);

/// The slash commands, for completion and /help.
pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "list the commands and keys"),
    (
        "/undo",
        "restore the files to before the last prompt that changed them",
    ),
    (
        "/rewind",
        "go back to an earlier prompt: conversation and files",
    ),
    (
        "/tree",
        "move to another point in the conversation (files stay)",
    ),
    ("/diff", "the session's changes, by source"),
    ("/show", "a tool call in full: /show <n>"),
    ("/stats", "tokens, timing, guards and asks this session"),
    ("/todos", "the todo list on this branch"),
    ("/mode", "auto or strict: /mode strict"),
    ("/plan", "plan mode on or off: read-only, ends with a plan"),
    ("/theme", "list the themes, or switch: /theme <name>"),
    ("/label", "label the current point: /label <text>"),
    ("/new", "start a new session"),
    ("/resume", "continue an earlier session in this directory"),
    (
        "/fork",
        "a new session from before an earlier prompt, to edit and resend it",
    ),
    ("/inspect", "the session as an inspection timeline"),
    ("/reload", "load the extensions again"),
    ("/quit", "leave psi"),
];

struct Running {
    started: Instant,
    tools: Vec<(String, String, Value, Instant)>,
}

struct Card {
    req: ApprovalRequest,
    tx: oneshot::Sender<Approval>,
    typing: Option<String>,
}

enum Picker {
    /// An extension's ui.select.
    Select {
        title: String,
        items: Vec<String>,
        sel: usize,
        reply: oneshot::Sender<Option<String>>,
    },
    /// Pick an entry; `files` restores files too (/rewind).
    Goto {
        items: Vec<(String, String)>,
        sel: usize,
        files: bool,
    },
    Resume {
        items: Vec<PathBuf>,
        sel: usize,
    },
    /// Pick a prompt to fork from (/fork).
    Fork {
        items: Vec<(String, String)>,
        sel: usize,
    },
    /// What to do with a proposed plan.
    Plan {
        sel: usize,
    },
}

const PLAN_CHOICES: [&str; 3] = ["Carry out the plan", "Stay in plan mode", "Refine the plan"];

/// What a committed tool call was, for /show.
struct Shown {
    name: String,
    args: Value,
    result: String,
    is_error: bool,
}

pub struct App<W: Write> {
    r: Renderer<W>,
    editor: Editor,
    md: MdStream,
    status: Status,
    cmds: UnboundedSender<Cmd>,
    queues: Queues,
    cancel: std::sync::Arc<std::sync::Mutex<CancellationToken>>,
    running: Option<Running>,
    card: Option<Card>,
    blocked: Option<(BlockedRequest, oneshot::Sender<BlockedChoice>)>,
    pause: Option<(String, oneshot::Sender<bool>)>,
    picker: Option<Picker>,
    pending_entries: Option<&'static str>,
    completer: Completer,
    drop_sel: usize,
    shown: Vec<Shown>,
    calls_by_id: Vec<(String, String, Value)>,
    queued: VecDeque<(String, bool)>,
    focused: bool,
    last_ctrl_c: Option<Instant>,
    quit: bool,
    sessions_dir: PathBuf,
    label_text: Option<String>,
    debug: bool,
    ext: Arc<ExtManager>,
    rt: tokio::runtime::Handle,
    ui_tx: UnboundedSender<UiMsg>,
    ext_modal: Option<ExtModal>,
    ext_status: BTreeMap<String, String>,
    widgets: BTreeMap<String, Vec<Line>>,
    resources: Arc<psi_cli::resources::Resources>,
    /// Plan mode as the agent last recorded it.
    plan: PlanState,
    /// A plan was proposed during this run: offer it when the run ends.
    plan_proposed: bool,
    keymap: Keymap,
}

pub struct AppParts<W: Write> {
    pub out: W,
    pub width: u16,
    pub height: u16,
    pub status: Status,
    pub cmds: UnboundedSender<Cmd>,
    pub queues: Queues,
    pub cancel: std::sync::Arc<std::sync::Mutex<CancellationToken>>,
    pub completer: Completer,
    pub sessions_dir: PathBuf,
    pub history: Vec<String>,
    pub ext: Arc<ExtManager>,
    pub rt: tokio::runtime::Handle,
    pub ui_tx: UnboundedSender<UiMsg>,
    pub resources: Arc<psi_cli::resources::Resources>,
    pub keymap: Keymap,
}

impl<W: Write> App<W> {
    pub fn new(p: AppParts<W>) -> Self {
        let mut editor = Editor::new();
        editor.set_history(p.history);
        editor.set_keymap(p.keymap.clone());
        let mut completer = p.completer;
        completer.extra = p
            .ext
            .commands()
            .into_iter()
            .map(|(n, d, _)| (format!("/{n}"), d))
            .collect();
        completer.extra.extend(p.resources.commands());
        Self {
            r: Renderer::new(p.out, p.width, p.height),
            editor,
            md: MdStream::new(),
            status: p.status,
            cmds: p.cmds,
            queues: p.queues,
            cancel: p.cancel,
            running: None,
            card: None,
            blocked: None,
            pause: None,
            picker: None,
            pending_entries: None,
            completer,
            drop_sel: 0,
            shown: Vec::new(),
            calls_by_id: Vec::new(),
            queued: VecDeque::new(),
            focused: true,
            last_ctrl_c: None,
            quit: false,
            sessions_dir: p.sessions_dir,
            label_text: None,
            ext: p.ext,
            rt: p.rt,
            ui_tx: p.ui_tx,
            ext_modal: None,
            ext_status: BTreeMap::new(),
            widgets: BTreeMap::new(),
            resources: p.resources,
            plan: PlanState::default(),
            plan_proposed: false,
            keymap: p.keymap,
            debug: std::env::var_os("PSI_DEBUG_RENDER").is_some(),
        }
    }

    pub fn history(&self) -> Vec<String> {
        self.editor.history().to_vec()
    }

    pub fn start(&mut self, banner: Vec<Line>) -> std::io::Result<()> {
        self.r.start(true)?;
        self.r.commit(&banner)?;
        self.draw()
    }

    pub fn finish(&mut self) -> std::io::Result<()> {
        self.r.finish(false)
    }

    fn width(&self) -> usize {
        self.r.width() as usize
    }

    fn commit(&mut self, lines: Vec<Line>) {
        let _ = self.r.commit(&lines);
    }

    fn info(&mut self, s: &str) {
        self.commit(vec![Line::styled(s, dim())]);
    }

    fn error(&mut self, s: &str) {
        self.commit(vec![Line::styled(s, theme::style(Token::Error))]);
    }

    fn bell(&mut self) {
        if !self.focused {
            let out = self.r.get_mut();
            let _ = out.write_all(b"\x07").and_then(|()| out.flush());
        }
    }

    fn send(&mut self, c: Cmd) {
        if self.cmds.send(c).is_err() {
            self.error("the agent stopped; restart psi");
        }
    }

    /// The main loop: until /quit or Ctrl+D.
    pub fn run(&mut self, rx: &mut UnboundedReceiver<UiMsg>) -> std::io::Result<()> {
        while !self.quit {
            let mut dirty = false;
            if event::poll(Duration::from_millis(16))? {
                // Take every waiting event before drawing, so a paste or
                // fast typing costs one frame.
                loop {
                    self.on_event(event::read()?);
                    dirty = true;
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            while let Ok(m) = rx.try_recv() {
                self.on_msg(m);
                dirty = true;
            }
            if self.running.is_some() {
                dirty = true; // the elapsed time on running calls
            }
            if dirty {
                self.draw()?;
            }
        }
        Ok(())
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Paste(s) => {
                if let Some(c) = &mut self.card
                    && let Some(t) = &mut c.typing
                {
                    t.push_str(&s.replace('\n', " "));
                } else {
                    self.editor.insert_str(&s);
                    self.refresh_completion();
                }
            }
            Event::Resize(w, h) => self.r.resize(w, h),
            Event::FocusGained => self.focused = true,
            Event::FocusLost => self.focused = false,
            _ => {}
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if self.card.is_some() {
            return self.card_key(k);
        }
        if self.ext_modal.is_some() {
            return self.modal_key(k);
        }
        if let Some(name) = key_name(&k)
            && self.ext.shortcuts().iter().any(|(s, _)| *s == name)
        {
            let (ext, tx) = (self.ext.clone(), self.ui_tx.clone());
            self.rt.spawn(async move {
                if let Err(e) = ext.run_shortcut(&name).await {
                    let _ = tx.send(UiMsg::Error(e));
                }
            });
            return;
        }
        if let Some((req, _)) = &self.blocked {
            let choice = match k.code {
                KeyCode::Char('r') => Some(BlockedChoice::RerunOutside),
                KeyCode::Char('a') => Some(BlockedChoice::AddDir),
                KeyCode::Char('n') | KeyCode::Esc => Some(BlockedChoice::No),
                _ => None,
            };
            if let Some(c) = choice {
                let dir = req.dir.display().to_string();
                if let Some((_, tx)) = self.blocked.take() {
                    let _ = tx.send(c);
                }
                let what = match c {
                    BlockedChoice::RerunOutside => {
                        "rerunning it once outside the sandbox".to_owned()
                    }
                    BlockedChoice::AddDir => format!("{dir} is writable for this session"),
                    BlockedChoice::No => "left blocked".to_owned(),
                };
                self.info(&format!("  {what}"));
            }
            return;
        }
        if self.pause.is_some() {
            let go = match k.code {
                KeyCode::Char('c') | KeyCode::Enter => Some(true),
                KeyCode::Char('s') | KeyCode::Esc => Some(false),
                _ => None,
            };
            if let (Some(go), Some((_, tx))) = (go, self.pause.take()) {
                let _ = tx.send(go);
            }
            return;
        }
        if self.picker.is_some() {
            return self.picker_key(k);
        }
        if self.keymap.is(Action::PlanToggle, &k) {
            if self.running.is_some() {
                return self.error("/plan waits until the run ends (Esc aborts it)");
            }
            return self.send(Cmd::Plan);
        }
        if self.keymap.is(Action::Interrupt, &k) {
            if self.completer.active() {
                self.completer.close();
            } else if self.running.is_some() {
                self.abort();
            }
            return;
        }
        if self.keymap.is(Action::Clear, &k) {
            {
                if !self.editor.is_empty() {
                    self.editor.clear();
                    self.completer.close();
                } else if self.running.is_some() {
                    self.abort();
                } else if self
                    .last_ctrl_c
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                {
                    self.quit = true;
                } else {
                    self.last_ctrl_c = Some(Instant::now());
                    let key = self.keymap.first(Action::Clear).unwrap_or_default();
                    self.info(&format!("press {key} again to quit"));
                }
                return;
            }
        }
        if self.keymap.is(Action::Exit, &k) && self.editor.is_empty() {
            self.quit = true;
            return;
        }
        if self.keymap.is(Action::FollowUp, &k) {
            let text = self.editor.text();
            if !text.trim().is_empty() {
                self.editor.push_history(&text);
                self.editor.clear();
                self.submit(text, true);
            }
            return;
        }
        match k.code {
            KeyCode::Up | KeyCode::Down if self.completer.active() => {
                let n = self.completer.items().len();
                if n > 0 {
                    self.drop_sel = if k.code == KeyCode::Up {
                        (self.drop_sel + n - 1) % n
                    } else {
                        (self.drop_sel + 1) % n
                    };
                    // What the box shows is what Enter runs.
                    let pick = self.completer.items()[self.drop_sel].insert.clone();
                    self.editor.replace_current_word(&pick);
                    self.completer.keep_open();
                }
                return;
            }
            KeyCode::Tab if self.completer.active() => {
                if let Some(it) = self.completer.items().get(self.drop_sel) {
                    let pick = it.insert.clone();
                    let dir = pick.ends_with('/');
                    self.editor.replace_current_word(&pick);
                    if !dir {
                        self.editor.insert_str(" ");
                    }
                }
                self.completer.close();
                self.refresh_completion();
                return;
            }
            _ => {}
        }
        match self.editor.handle(k) {
            EditorEvent::Submit(text) => {
                self.completer.close();
                self.submit(text, false);
            }
            EditorEvent::Changed => self.refresh_completion(),
            EditorEvent::None => {}
        }
    }

    fn refresh_completion(&mut self) {
        let (col, word) = self.editor.current_word();
        let first = self.editor.cursor().0 == 0 && col == 0;
        self.completer.update(&word, first);
        self.drop_sel = 0;
    }

    fn card_key(&mut self, k: KeyEvent) {
        let Some(card) = &mut self.card else { return };
        if let Some(t) = &mut card.typing {
            match k.code {
                KeyCode::Enter => {
                    let msg = std::mem::take(t);
                    if let Some(c) = self.card.take() {
                        let _ = c.tx.send(Approval::DenyWith(msg.clone()));
                        self.info(&format!("  denied: {msg}"));
                    }
                }
                KeyCode::Esc => card.typing = None,
                KeyCode::Backspace => {
                    t.pop();
                }
                KeyCode::Char(ch) => t.push(ch),
                _ => {}
            }
            return;
        }
        let answer = match k.code {
            KeyCode::Char('y') => Some(Approval::Once),
            KeyCode::Char('s') => Some(Approval::Session),
            KeyCode::Char('n') | KeyCode::Esc => Some(Approval::Deny),
            KeyCode::Char('e') => {
                card.typing = Some(String::new());
                None
            }
            _ => None,
        };
        if let Some(a) = answer
            && let Some(c) = self.card.take()
        {
            let what = match &a {
                Approval::Once => "allowed once",
                Approval::Session => "allowed for this session",
                _ => "denied",
            };
            let _ = c.tx.send(a);
            self.info(&format!("  {what}"));
        }
    }

    fn picker_key(&mut self, k: KeyEvent) {
        let Some(p) = &mut self.picker else { return };
        let (n, sel) = match p {
            Picker::Goto { items, sel, .. } | Picker::Fork { items, sel } => (items.len(), sel),
            Picker::Resume { items, sel } => (items.len(), sel),
            Picker::Select { items, sel, .. } => (items.len(), sel),
            Picker::Plan { sel } => (PLAN_CHOICES.len(), sel),
        };
        match k.code {
            KeyCode::Up if n > 0 => *sel = (*sel + n - 1) % n,
            KeyCode::Down if n > 0 => *sel = (*sel + 1) % n,
            KeyCode::Esc => self.close_picker(),
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.close_picker()
            }
            KeyCode::Enter => match self.picker.take() {
                Some(Picker::Goto { items, sel, files }) => {
                    if let Some((id, label)) = items.get(sel) {
                        self.info(&format!("  going back to: {label}"));
                        self.send(Cmd::Goto {
                            id: id.clone(),
                            files,
                        });
                    }
                }
                Some(Picker::Resume { items, sel }) => {
                    if let Some(p) = items.get(sel) {
                        self.send(Cmd::Resume(p.clone()));
                    }
                }
                Some(Picker::Fork { items, sel }) => {
                    if let Some((id, _)) = items.get(sel) {
                        self.send(Cmd::Fork(id.clone()));
                    }
                }
                Some(Picker::Select {
                    items, sel, reply, ..
                }) => {
                    let _ = reply.send(items.get(sel).cloned());
                }
                Some(Picker::Plan { sel }) => match sel {
                    0 => {
                        self.commit_user("Carry out the plan");
                        self.running = Some(Running {
                            started: Instant::now(),
                            tools: Vec::new(),
                        });
                        self.send(Cmd::ExecutePlan);
                    }
                    1 => self.info("  still in plan mode"),
                    _ => self.info("  say what to change; plan mode stays on"),
                },
                None => {}
            },
            _ => {}
        }
    }

    /// Esc on a picker: an extension's select answers nil.
    fn close_picker(&mut self) {
        if let Some(Picker::Select { reply, .. }) = self.picker.take() {
            let _ = reply.send(None);
        }
    }

    fn modal_key(&mut self, k: KeyEvent) {
        let Some(m) = &mut self.ext_modal else { return };
        match m {
            ExtModal::Confirm { .. } => {
                let yes = match k.code {
                    KeyCode::Char('y') | KeyCode::Enter => true,
                    KeyCode::Char('n') | KeyCode::Esc => false,
                    _ => return,
                };
                if let Some(ExtModal::Confirm { reply, .. }) = self.ext_modal.take() {
                    let _ = reply.send(yes);
                }
            }
            ExtModal::Input { text, .. } => match k.code {
                KeyCode::Enter | KeyCode::Esc => {
                    if let Some(ExtModal::Input { text, reply, .. }) = self.ext_modal.take() {
                        let _ = reply.send((k.code == KeyCode::Enter).then_some(text));
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char(c) => text.push(c),
                _ => {}
            },
            ExtModal::Custom { ext, id, .. } => {
                let name = key_name(&k).or_else(|| match k.code {
                    KeyCode::Char(c) => Some(c.to_string()),
                    _ => None,
                });
                let Some(name) = name else { return };
                let out = self
                    .ext
                    .component_input(ext, *id, &name, Duration::from_secs(1));
                // A nil answer keeps it open; Esc with no answer closes it.
                let close = match out {
                    Some(v) => Some(Some(v)),
                    None if k.code == KeyCode::Esc => Some(None),
                    None => None,
                };
                if let Some(v) = close
                    && let Some(ExtModal::Custom { reply, .. }) = self.ext_modal.take()
                {
                    let _ = reply.send(v);
                }
            }
        }
    }

    fn abort(&mut self) {
        if let Ok(c) = self.cancel.lock() {
            c.cancel();
        }
        self.info("  aborting…");
    }

    fn submit(&mut self, text: String, follow_up: bool) {
        self.submit_from(text, follow_up, "interactive");
    }

    /// Commands and `!bash` first; then the `input` event (pi parity),
    /// whose handlers may change or consume the text; then skills,
    /// prompt templates and the agent (`after_input`).
    fn submit_from(&mut self, text: String, follow_up: bool, source: &'static str) {
        let t = text.trim().to_owned();
        if t.is_empty() {
            return;
        }
        if let Some(cmd) = t.strip_prefix('/')
            && !cmd.split_whitespace().next().unwrap_or("").contains('/')
        {
            let name = cmd.split_whitespace().next().unwrap_or("");
            let builtin = COMMANDS.iter().any(|(c, _)| &c[1..] == name)
                || ["?", "exit", "q"].contains(&name)
                || self.ext.commands().iter().any(|(n, _, _)| n == name);
            if builtin {
                return self.slash(cmd);
            }
        }
        if let Some(c) = t.strip_prefix('!') {
            let (command, to_context) = match c.strip_prefix('!') {
                Some(rest) => (rest.trim().to_owned(), false),
                None => (c.trim().to_owned(), true),
            };
            return self.user_bash(command, to_context);
        }
        if self.ext.listens("input") {
            let (ext, tx) = (self.ext.clone(), self.ui_tx.clone());
            self.rt.spawn(async move {
                let out = ext
                    .chain("input", serde_json::json!({"text": t, "source": source}))
                    .await;
                let text = if out["handled"] == serde_json::Value::Bool(true) {
                    None
                } else {
                    Some(out["text"].as_str().unwrap_or(&t).to_owned())
                };
                let _ = tx.send(UiMsg::Input { text, follow_up });
            });
            return;
        }
        self.after_input(t, follow_up);
    }

    fn after_input(&mut self, t: String, follow_up: bool) {
        if let Some(cmd) = t.strip_prefix('/')
            && !cmd.split_whitespace().next().unwrap_or("").contains('/')
        {
            // Skills and prompt templates (D-35); built-ins won above.
            match self.resources.expand(&t) {
                Ok(Some(e)) => {
                    let text = e.text.clone();
                    self.info(&format!("  {}", e.note));
                    return self.send_prompt(text, follow_up);
                }
                Ok(None) => return self.slash(cmd),
                Err(e) => return self.error(&e),
            }
        }
        self.send_prompt(t, follow_up);
    }

    /// Sends text to the agent: a new run, or while one runs, a steering
    /// or follow-up message.
    fn send_prompt(&mut self, t: String, follow_up: bool) {
        if self.running.is_some() {
            // Steering goes in before the next turn; a follow-up waits until
            // the run would stop (pi parity).
            let q = if follow_up {
                &self.queues.follow_up
            } else {
                &self.queues.steering
            };
            if let Ok(mut q) = q.lock() {
                q.push_back(t.clone());
            }
            self.queued.push_back((t, follow_up));
            return;
        }
        self.commit_user(&t);
        self.running = Some(Running {
            started: Instant::now(),
            tools: Vec::new(),
        });
        self.send(Cmd::Prompt(t));
    }

    fn commit_user(&mut self, t: &str) {
        let t = &shown_user_text(t);
        let mut lines = vec![Line::default()];
        for (i, l) in t.lines().enumerate() {
            let mut line = Line::default();
            line.push(
                if i == 0 { "› " } else { "  " },
                theme::style(Token::UserMessageText).bold(),
            );
            line.push(l, Style::default().bold());
            lines.push(line);
        }
        self.commit(lines);
    }

    fn user_bash(&mut self, command: String, to_context: bool) {
        if command.is_empty() {
            return;
        }
        let mut l = Line::default();
        l.push("! ", theme::style(Token::BashMode).bold());
        l.push(&command, Style::default().bold());
        self.commit(vec![Line::default(), l]);
        // Runs as the user, outside the boundary, without approval (§6).
        let out = std::process::Command::new("bash")
            .arg("-c")
            .arg(&command)
            .stdin(std::process::Stdio::null())
            .output();
        let output = match out {
            Ok(o) => {
                let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                s.push_str(&String::from_utf8_lossy(&o.stderr));
                if let Some(c) = o.status.code().filter(|c| *c != 0) {
                    s.push_str(&format!("\n[exit code {c}]"));
                }
                s
            }
            Err(e) => format!("cannot run bash: {e}"),
        };
        self.on_msg(UiMsg::UserBash {
            command,
            output,
            to_context,
        });
    }

    fn slash(&mut self, line: &str) {
        let (name, arg) = line
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((line, ""));
        let busy = self.running.is_some();
        let needs_idle = matches!(
            name,
            "undo" | "rewind" | "tree" | "new" | "resume" | "mode" | "label" | "plan"
        );
        if busy && needs_idle {
            return self.error(&format!("/{name} waits until the run ends (Esc aborts it)"));
        }
        match name {
            "help" | "?" => {
                let mut lines = vec![Line::default()];
                for (c, d) in COMMANDS {
                    let mut l = Line::default();
                    l.push(format!("  {c:<10}"), theme::style(Token::Accent));
                    l.push(d, dim());
                    lines.push(l);
                }
                for (c, d, ext) in self.ext.commands() {
                    let mut l = Line::default();
                    l.push(format!("  /{c:<9}"), theme::style(Token::Accent));
                    l.push(format!("{d} ({ext})"), dim());
                    lines.push(l);
                }
                for (k, d) in self.ext.shortcuts() {
                    let mut l = Line::default();
                    l.push(format!("  {k:<12}"), theme::style(Token::Accent));
                    l.push(d, dim());
                    lines.push(l);
                }
                let keys = self.keymap.describe();
                let keys = keys.iter().map(|(k, d)| (k.as_str(), d.as_str()));
                for (k, d) in keys.chain([
                    ("@path", "add a file reference (Tab completes)"),
                    (
                        "!cmd",
                        "run a command yourself; its output goes to the model (!!cmd: it doesn't)",
                    ),
                ]) {
                    let mut l = Line::default();
                    l.push(format!("  {k:<12}"), theme::style(Token::Accent));
                    l.push(d, dim());
                    lines.push(l);
                }
                self.commit(lines);
            }
            "quit" | "exit" | "q" => self.quit = true,
            "undo" => self.send(Cmd::Undo),
            "diff" => self.send(Cmd::Diff),
            "rewind" | "tree" | "fork" | "stats" | "inspect" | "todos" => {
                self.pending_entries = Some(match name {
                    "rewind" => "rewind",
                    "tree" => "tree",
                    "fork" => "fork",
                    "stats" => "stats",
                    "todos" => "todos",
                    _ => "inspect",
                });
                self.send(Cmd::Entries);
            }
            "mode" => {
                let m = match arg {
                    "" => {
                        if self.status.mode == "auto" {
                            Mode::Strict
                        } else {
                            Mode::Auto
                        }
                    }
                    a => match Mode::parse(a) {
                        Some(m) => m,
                        None => return self.error("/mode auto or /mode strict"),
                    },
                };
                self.status.mode = m.as_str().into();
                self.send(Cmd::Mode(m));
            }
            "show" => {
                let n: usize = match arg.trim_start_matches('#').parse() {
                    Ok(n) => n,
                    Err(_) => return self.error("/show <n>: the number after # on a tool call"),
                };
                let Some(s) = self.shown.get(n.wrapping_sub(1)) else {
                    return self.error(&format!("no tool call #{n}"));
                };
                let mut lines = vec![
                    Line::default(),
                    call_header(n, &s.name, &s.args, self.width(), None),
                ];
                let args = serde_json::to_string_pretty(&s.args).unwrap_or_default();
                lines.extend(args.lines().map(|l| Line::styled(format!("  {l}"), dim())));
                let style = if s.is_error {
                    theme::style(Token::Error)
                } else {
                    Style::default()
                };
                lines.extend(
                    s.result
                        .lines()
                        .map(|l| Line::styled(format!("  {l}"), style)),
                );
                self.commit(lines);
            }
            "label" => {
                if arg.is_empty() {
                    return self.error("/label <text>");
                }
                self.pending_entries = Some("label");
                self.label_text = Some(arg.to_owned());
                self.send(Cmd::Entries);
            }
            "new" => self.send(Cmd::New),
            "reload" => self.send(Cmd::Reload),
            "plan" => self.send(Cmd::Plan),
            "theme" => {
                if arg.is_empty() {
                    let mut names = vec!["default".to_owned()];
                    names.extend(self.resources.themes.iter().map(|t| t.name.clone()));
                    let now = theme::current_name();
                    let list: Vec<String> = names
                        .iter()
                        .map(|n| {
                            if *n == now {
                                format!("{n} (now)")
                            } else {
                                n.clone()
                            }
                        })
                        .collect();
                    return self.info(&format!("  themes: {}", list.join(", ")));
                }
                match self.resources.theme(arg) {
                    Some(t) => {
                        theme::set(t);
                        self.info(&format!(
                            "  theme {arg} for this session; set \"theme\" in settings.jsonc to keep it"
                        ));
                    }
                    None => self.error(&format!("no theme named '{arg}'; /theme lists them")),
                }
            }
            "resume" => {
                let mut files = psi_session::SessionFile::list(&self.sessions_dir);
                files.reverse();
                if files.is_empty() {
                    return self.info("no earlier sessions in this directory");
                }
                self.picker = Some(Picker::Resume {
                    items: files,
                    sel: 0,
                });
            }
            other if self.ext.commands().iter().any(|(n, _, _)| n == other) => {
                let (ext, tx) = (self.ext.clone(), self.ui_tx.clone());
                let (name, args) = (other.to_owned(), arg.to_owned());
                self.rt.spawn(async move {
                    if let Err(e) = ext.run_command(&name, &args).await {
                        let _ = tx.send(UiMsg::Error(format!("/{name}: {e}")));
                    }
                });
            }
            other => self.error(&format!("no command /{other}; /help lists them")),
        }
    }

    fn on_msg(&mut self, m: UiMsg) {
        match m {
            UiMsg::Agent(ev) => self.on_agent(ev),
            UiMsg::Approve(req, tx) => {
                self.flush_text();
                self.card = Some(Card {
                    req,
                    tx,
                    typing: None,
                });
                self.bell();
            }
            UiMsg::Blocked(req, tx) => {
                self.flush_text();
                self.blocked = Some((req, tx));
                self.bell();
            }
            UiMsg::Pause(reason, tx) => {
                self.flush_text();
                self.pause = Some((reason, tx));
                self.bell();
            }
            UiMsg::RunDone(out) => {
                self.flush_text();
                self.running = None;
                self.status.busy = None;
                self.queued.clear();
                match &out.stop {
                    Stop::Done => {}
                    Stop::Aborted => self.info("  aborted"),
                    Stop::Budget(r) | Stop::Guard(r) => self.commit(vec![Line::styled(
                        format!("  stopped: {r}"),
                        theme::style(Token::Warning),
                    )]),
                    Stop::Failed(r) => self.error(&format!("  failed: {r}")),
                }
                self.offer_plan();
                self.bell();
            }
            UiMsg::Info(s) => self.info(&format!("  {s}")),
            UiMsg::Error(s) => self.error(&format!("  {s}")),
            UiMsg::Input { text, follow_up } => {
                if let Some(t) = text {
                    self.after_input(t, follow_up);
                }
            }
            UiMsg::Restored {
                what,
                written,
                deleted,
                left_alone,
            } => {
                let mut s = format!("  {what}: {written} files written, {deleted} deleted");
                if !left_alone.is_empty() {
                    s.push_str(&format!("; left alone: {}", left_alone.join(", ")));
                }
                self.info(&s);
            }
            UiMsg::Moved(id) => self.info(&format!("  the conversation continues from {id}")),
            UiMsg::Diff(changes) => {
                if changes.is_empty() {
                    return self.info("  no changes in this session");
                }
                let mut lines = Vec::new();
                for c in changes {
                    lines.push(Line::default());
                    lines.push(Line::styled(
                        format!("▌ {}", c.source),
                        theme::style(Token::Accent).bold(),
                    ));
                    lines.extend(git_diff_lines(&c.diff));
                }
                self.commit(lines);
            }
            UiMsg::Entries { entries, leaf } => self.with_entries(entries, leaf),
            UiMsg::Switched {
                what,
                path,
                entries,
                prefill,
            } => {
                self.shown.clear();
                self.info(&format!("  {what}: {}", path.display()));
                // The agent takes plan mode from the new session; so does the screen.
                self.set_plan(PlanState::default());
                let _ = self.replay(&entries);
                if let Some(t) = prefill {
                    self.editor.set_text(&t);
                }
            }
            UiMsg::Ext(ext, req) => self.on_ext(ext, req),
            UiMsg::UserBash {
                command,
                output,
                to_context,
            } => {
                let lines: Vec<&str> = output.lines().collect();
                let mut out: Vec<Line> = lines
                    .iter()
                    .take(20)
                    .map(|l| Line::styled(format!("  {l}"), dim()))
                    .collect();
                if lines.len() > 20 {
                    out.push(Line::styled(
                        format!("  … {} more lines", lines.len() - 20),
                        dim(),
                    ));
                }
                self.commit(out);
                if to_context {
                    let text = format!("I ran `{command}`:\n{}", output.trim_end());
                    self.send(Cmd::Context(text));
                }
            }
        }
    }

    fn on_ext(&mut self, ext: String, req: ExtRequest) {
        match req {
            ExtRequest::Notify { message, level } => {
                let style = match level.as_str() {
                    "error" => theme::style(Token::Error),
                    "warning" => theme::style(Token::Warning),
                    _ => dim(),
                };
                self.commit(vec![Line::styled(format!("  [{ext}] {message}"), style)]);
            }
            ExtRequest::Confirm {
                title,
                message,
                reply,
            } => {
                self.flush_text();
                self.ext_modal = Some(ExtModal::Confirm {
                    ext,
                    title,
                    message,
                    reply,
                });
                self.bell();
            }
            ExtRequest::Input { title, reply, .. } => {
                self.flush_text();
                self.ext_modal = Some(ExtModal::Input {
                    ext,
                    title,
                    text: String::new(),
                    reply,
                });
                self.bell();
            }
            ExtRequest::Select {
                title,
                options,
                reply,
            } => {
                self.flush_text();
                self.picker = Some(Picker::Select {
                    title,
                    items: options,
                    sel: 0,
                    reply,
                });
                self.bell();
            }
            ExtRequest::Custom { id, reply } => {
                self.flush_text();
                self.ext_modal = Some(ExtModal::Custom {
                    ext,
                    id,
                    reply,
                    last: Vec::new(),
                });
            }
            ExtRequest::Status { key, text } => match text {
                Some(t) => {
                    self.ext_status.insert(key, t);
                }
                None => {
                    self.ext_status.remove(&key);
                }
            },
            ExtRequest::Widget { slot, lines } => match lines {
                Some(l) => {
                    self.widgets
                        .insert(slot, l.iter().map(|x| Line::from_ansi(x)).collect());
                }
                None => {
                    self.widgets.remove(&slot);
                }
            },
            ExtRequest::SendUserMessage { text, follow_up } => {
                if self.running.is_some() {
                    let q = if follow_up {
                        &self.queues.follow_up
                    } else {
                        &self.queues.steering
                    };
                    if let Ok(mut q) = q.lock() {
                        q.push_back(text.clone());
                    }
                    self.queued.push_back((text, follow_up));
                } else {
                    self.submit_from(text, false, "extension");
                }
            }
            _ => {}
        }
    }

    /// A resumed session: its messages, committed again.
    pub fn replay(&mut self, entries: &[Entry]) -> std::io::Result<()> {
        let leaf = entries.last().map(|e| e.id.clone());
        let mut plan = None;
        for e in psi_session::branch(entries, leaf.as_deref()) {
            if let EntryBody::Custom { custom_type, data } = &e.body
                && custom_type == "plan"
            {
                plan = Some(PlanState::from_json(data));
            }
            if let EntryBody::Message { role, content, .. } = &e.body {
                let t = text_of(content);
                match role {
                    Role::User => self.commit_user(&t),
                    Role::Assistant if !t.is_empty() => {
                        let w = self.width();
                        let mut md = MdStream::new();
                        let mut blocks = md.push(&t);
                        blocks.extend(md.finish());
                        let lines: Vec<Line> =
                            blocks.iter().flat_map(|b| render_block(b, w)).collect();
                        self.commit(lines);
                    }
                    _ => {}
                }
            }
        }
        if let Some(p) = plan {
            self.set_plan(p);
        }
        self.draw()
    }

    fn with_entries(&mut self, entries: Vec<Entry>, leaf: Option<String>) {
        let Some(what) = self.pending_entries.take() else {
            return;
        };
        let branch = psi_session::branch(&entries, leaf.as_deref());
        match what {
            "rewind" | "tree" => {
                let source: Vec<&Entry> = if what == "rewind" {
                    branch.clone()
                } else {
                    entries.iter().collect()
                };
                // /tree lists every prompt; those off the current branch
                // are marked, so a picked one starts from another branch.
                let items: Vec<(String, String)> = source
                    .iter()
                    .filter_map(|e| match &e.body {
                        EntryBody::Message {
                            role: Role::User,
                            content,
                            ..
                        } => {
                            let on_branch = branch.iter().any(|b| b.id == e.id);
                            let mark = if on_branch { "" } else { "↳ " };
                            let text = view::cut(&text_of(content).replace('\n', " "), 100);
                            Some((e.id.clone(), format!("{mark}{text}")))
                        }
                        _ => None,
                    })
                    .collect();
                if items.is_empty() {
                    return self.info("  no earlier prompts");
                }
                let sel = items.len() - 1;
                self.picker = Some(Picker::Goto {
                    items,
                    sel,
                    files: what == "rewind",
                });
            }
            "fork" => {
                let items: Vec<(String, String)> = branch
                    .iter()
                    .filter_map(|e| match &e.body {
                        EntryBody::Message {
                            role: Role::User,
                            content,
                            ..
                        } => Some((
                            e.id.clone(),
                            view::cut(&text_of(content).replace('\n', " "), 100),
                        )),
                        _ => None,
                    })
                    .collect();
                if items.is_empty() {
                    return self.info("  no earlier prompts to fork from");
                }
                let sel = items.len() - 1;
                self.picker = Some(Picker::Fork { items, sel });
            }
            "label" => {
                if let (Some(id), Some(t)) = (leaf, self.label_text.take()) {
                    self.send(Cmd::Label { id, label: Some(t) });
                }
            }
            "todos" => {
                let list = branch
                    .iter()
                    .rev()
                    .find_map(|e| match &e.body {
                        EntryBody::Custom { custom_type, data } if custom_type == "todo" => {
                            Some(psi_tools::TodoList::from_json(data))
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                let done = list.items.iter().filter(|t| t.done).count();
                let mut lines = vec![Line::default()];
                if list.items.is_empty() {
                    lines.push(Line::styled("  no todos on this branch", dim()));
                } else {
                    lines.push(Line::styled(
                        format!("  todos: {done}/{} done", list.items.len()),
                        dim(),
                    ));
                    for t in &list.items {
                        let mut l = Line::default();
                        if t.done {
                            l.push("  ✓ ", theme::style(Token::Success));
                            l.push(format!("#{} {}", t.id, t.text), dim());
                        } else {
                            l.push("  ○ ", dim());
                            l.push(format!("#{} {}", t.id, t.text), Style::default());
                        }
                        lines.push(l);
                    }
                }
                self.commit(lines);
            }
            "stats" => {
                let lines = stats(&branch);
                self.commit(
                    lines
                        .into_iter()
                        .map(|l| Line::styled(format!("  {l}"), dim()))
                        .collect(),
                );
            }
            _ => {
                let rec = psi_lab::inspect::Record {
                    entries: branch.into_iter().cloned().collect(),
                    result: None,
                };
                let t = psi_lab::inspect::timeline(&rec);
                let text = psi_lab::inspect::render_text(&t, false);
                self.commit(text.lines().map(Line::raw).collect());
            }
        }
    }

    /// Commits whatever streamed text is pending.
    fn flush_text(&mut self) {
        let w = self.width();
        let blocks = self.md.finish();
        let lines: Vec<Line> = blocks.iter().flat_map(|b| render_block(b, w)).collect();
        if !lines.is_empty() {
            self.commit(lines);
        }
    }

    fn on_agent(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::TextDelta { text } => {
                let w = self.width();
                let first = self.md.pending().is_empty();
                let done = self.md.push(&text);
                let mut lines = Vec::new();
                if first && !done.is_empty() {
                    lines.push(Line::default());
                }
                lines.extend(done.iter().flat_map(|b| render_block(b, w)));
                if !lines.is_empty() {
                    self.commit(lines);
                }
            }
            AgentEvent::ToolStart { id, name, args } => {
                self.flush_text();
                if let Some(r) = &mut self.running {
                    r.tools
                        .push((id.clone(), name.clone(), args.clone(), Instant::now()));
                }
                self.calls_by_id.push((id, name, args));
            }
            AgentEvent::Entry { entry } => self.on_entry(&entry),
            AgentEvent::TurnStart { .. } => {
                if self.md.pending().is_empty() {
                    self.commit(vec![Line::default()]);
                }
            }
            AgentEvent::TurnEnd { .. } => self.flush_text(),
            AgentEvent::Compacting { tokens } => {
                self.flush_text();
                self.commit(vec![Line::styled(
                    format!("  summarizing the earlier conversation (about {tokens} tokens)..."),
                    dim(),
                )]);
            }
            _ => {}
        }
    }

    fn on_entry(&mut self, e: &Entry) {
        let w = self.width();
        match &e.body {
            EntryBody::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
            } => {
                let args = self
                    .calls_by_id
                    .iter()
                    .rev()
                    .find(|(id, _, _)| id == tool_call_id)
                    .map(|(_, _, a)| a.clone())
                    .unwrap_or(Value::Null);
                if let Some(r) = &mut self.running
                    && let Some(i) = r.tools.iter().position(|(id, ..)| id == tool_call_id)
                {
                    r.tools.remove(i);
                }
                let result = text_of(content);
                self.shown.push(Shown {
                    name: tool_name.clone(),
                    args: args.clone(),
                    result: result.clone(),
                    is_error: *is_error,
                });
                let n = self.shown.len();
                let call = serde_json::json!({"name": tool_name, "args": args,
                                              "result": result, "is_error": is_error});
                match self.ext.render_tool(tool_name, call, w, EXT_FRAME) {
                    Some(lines) => self.commit(lines.iter().map(|l| Line::from_ansi(l)).collect()),
                    None => self.commit(call_block(n, tool_name, &args, &result, *is_error, w)),
                }
            }
            EntryBody::Message {
                role: Role::User,
                content,
                ..
            } => {
                // A steering or follow-up message the agent just took.
                let t = text_of(content);
                if let Some(i) = self.queued.iter().position(|(q, _)| *q == t) {
                    self.queued.remove(i);
                    self.flush_text();
                    self.commit_user(&t);
                }
            }
            EntryBody::Message {
                role: Role::Assistant,
                usage: Some(u),
                ..
            } => {
                self.status.prompt_tokens = u.prompt_total() + u.output;
            }
            EntryBody::Custom { custom_type, data } if custom_type == "timing" => {
                self.status.ttft_ms = data["first_token_ms"].as_u64();
                self.status.tok_s = data["timings"]["predicted_per_second"].as_f64();
            }
            EntryBody::Custom { custom_type, data } if custom_type == "plan" => {
                let p = PlanState::from_json(data);
                self.plan_proposed = p.enabled && !p.steps.is_empty();
                self.set_plan(p);
                // The run's end and this entry come by different channels.
                if self.running.is_none() {
                    self.offer_plan();
                }
            }
            EntryBody::Checkpoint { .. } => self.status.checkpoints += 1,
            EntryBody::GuardEvent { guard, message, .. } => {
                self.commit(vec![Line::styled(
                    format!("  ⚑ {guard}: {message}"),
                    theme::style(Token::Warning),
                )]);
            }
            EntryBody::ClearedResults {
                entry_ids,
                tokens_freed,
                ..
            } => {
                self.commit(vec![Line::styled(
                    format!(
                        "  cleared {} old tool results to save context (about {tokens_freed} tokens)",
                        entry_ids.len()
                    ),
                    dim(),
                )]);
            }
            EntryBody::Compaction {
                summary, replaces, ..
            } if !summary.is_empty() => {
                let what = if replaces.is_empty() {
                    "  summarized the earlier conversation; recent turns are kept as they are"
                } else {
                    "  merged the two oldest summaries"
                };
                self.commit(vec![Line::styled(what, dim())]);
            }
            EntryBody::PolicyDecision {
                decision, reason, ..
            } if decision == "refused" || decision == "granted" => {
                self.commit(vec![Line::styled(format!("  {decision}: {reason}"), dim())]);
            }
            _ => {}
        }
    }

    /// After a run in plan mode that proposed a plan: show it and ask what next.
    fn offer_plan(&mut self) {
        if !std::mem::take(&mut self.plan_proposed) || !self.plan.enabled {
            return;
        }
        let mut lines = vec![
            Line::default(),
            Line::styled(
                format!("  plan: {} steps", self.plan.steps.len()),
                theme::style(Token::Accent).bold(),
            ),
        ];
        for (i, st) in self.plan.steps.iter().enumerate() {
            lines.push(Line::raw(format!("  {}. {}", i + 1, st.text)));
        }
        self.commit(lines);
        self.picker = Some(Picker::Plan { sel: 0 });
    }

    /// The status label and the steps widget follow the plan state.
    fn set_plan(&mut self, p: PlanState) {
        self.status.plan = if p.enabled {
            Some("plan".into())
        } else if p.executing {
            Some(format!("plan {}/{}", p.done_count(), p.steps.len()))
        } else {
            None
        };
        if p.executing {
            let lines = p
                .steps
                .iter()
                .map(|st| {
                    let mut l = Line::default();
                    if st.done {
                        l.push("  ✓ ", theme::style(Token::Success));
                        l.push(&st.text, dim());
                    } else {
                        l.push("  ○ ", dim());
                        l.push(&st.text, Style::default());
                    }
                    l
                })
                .collect();
            self.widgets.insert("~plan".into(), lines);
        } else {
            self.widgets.remove("~plan");
        }
        self.plan = p;
    }

    /// The live region: streaming text, running calls, queued messages,
    /// the status line, the input box, and below it a card or a list.
    fn draw(&mut self) -> std::io::Result<()> {
        let w = self.width();
        let mut live: Vec<Line> = Vec::new();
        let pending = self.md.pending();
        if !pending.is_empty() {
            let lines = render_block(pending, w);
            let keep = lines.len().saturating_sub(6);
            live.extend(lines.into_iter().skip(keep));
        }
        if let Some(r) = &self.running {
            for (i, (_, name, args, started)) in r.tools.iter().enumerate() {
                live.push(call_header(
                    self.shown.len() + i + 1,
                    name,
                    args,
                    w,
                    Some(started.elapsed().as_secs_f64()),
                ));
            }
            if r.tools.is_empty() && pending.is_empty() && self.card.is_none() {
                let spin = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                let i = (r.started.elapsed().as_millis() / 80) as usize % spin.len();
                live.push(Line::styled(
                    format!(
                        "{} waiting for the model… ({:.0}s)",
                        spin[i],
                        r.started.elapsed().as_secs_f64()
                    ),
                    dim(),
                ));
            }
        }
        for (q, follow) in &self.queued {
            let tag = if *follow { "follow-up" } else { "steering" };
            live.push(Line::styled(
                format!("  ({tag}) {}", view::cut(q, w.saturating_sub(16))),
                dim(),
            ));
        }
        for lines in self.widgets.values() {
            live.extend(lines.iter().map(|l| l.truncate(w)));
        }
        let mut status = status_line(&self.status, w);
        for (k, v) in &self.ext_status {
            status.push(format!("  {k}: {v}"), dim());
        }
        live.push(status.truncate(w));
        live.push(Line::styled(
            "─".repeat(w),
            theme::style(Token::BorderMuted),
        ));
        let top = live.len();
        let (rows, (cr, cc)) = self.editor.render(w, "› ", Style::default());
        live.extend(rows);
        let mut cursor = Some((top + cr, cc));
        if let Some(c) = &self.card {
            live.extend(approval_card(
                &c.req.tool,
                &c.req.args,
                c.req.intent.as_deref(),
                &c.req.preceding_text,
                &c.req.reason,
                c.typing.as_deref(),
                w,
            ));
            cursor = None;
        } else if let Some((req, _)) = &self.blocked {
            let y = theme::style(Token::Warning);
            live.push(Line::styled(
                format!(
                    "┌ blocked: bash tried to write {}, outside the workspace",
                    req.path.display()
                ),
                y,
            ));
            live.push(Line::styled(
                format!("│ {}", view::cut(&req.command, w.saturating_sub(4))),
                Style::default().bold(),
            ));
            live.push(Line::styled(format!("└ r rerun once outside the sandbox · a make {} writable for the session and rerun · n leave it", req.dir.display()), y));
            cursor = None;
        } else if let Some((reason, _)) = &self.pause {
            let y = theme::style(Token::Warning);
            live.push(Line::styled(format!("┌ paused: {reason}"), y));
            live.push(Line::styled("└ c continue · s stop the run", y));
            cursor = None;
        } else if let Some(m) = &mut self.ext_modal {
            let y = theme::style(Token::Warning);
            match m {
                ExtModal::Confirm {
                    ext,
                    title,
                    message,
                    ..
                } => {
                    live.push(Line::styled(format!("┌ {ext}: {title}"), y.bold()));
                    for l in message.lines().take(6) {
                        live.push(Line::styled(format!("│ {l}"), Style::default()));
                    }
                    live.push(Line::styled("└ y yes · n no", y));
                }
                ExtModal::Input {
                    ext, title, text, ..
                } => {
                    live.push(Line::styled(format!("┌ {ext}: {title}"), y.bold()));
                    live.push(Line::styled(format!("│ {text}▏"), Style::default()));
                    live.push(Line::styled("└ Enter send · Esc cancel", y));
                }
                ExtModal::Custom { ext, id, last, .. } => {
                    if let Some(lines) = self.ext.component_render(ext, *id, w, EXT_FRAME) {
                        *last = lines.iter().map(|l| Line::from_ansi(l)).collect();
                    }
                    live.extend(last.iter().map(|l| l.truncate(w)));
                }
            }
            cursor = None;
        } else if let Some(p) = &self.picker {
            let (items, sel): (Vec<String>, usize) = match p {
                Picker::Goto { items, sel, .. } | Picker::Fork { items, sel } => {
                    (items.iter().map(|(_, l)| l.clone()).collect(), *sel)
                }
                Picker::Resume { items, sel } => (
                    items
                        .iter()
                        .map(|p| {
                            p.file_name()
                                .map(|f| f.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        })
                        .collect(),
                    *sel,
                ),
                Picker::Select {
                    title, items, sel, ..
                } => {
                    live.push(Line::styled(format!("  {title}"), Style::default().bold()));
                    (items.clone(), *sel)
                }
                Picker::Plan { sel } => {
                    live.push(Line::styled("  what next?", Style::default().bold()));
                    (PLAN_CHOICES.iter().map(|c| (*c).to_owned()).collect(), *sel)
                }
            };
            live.extend(list(&items, sel, 10, w));
            live.push(Line::styled("  ↑↓ choose · Enter go · Esc cancel", dim()));
        } else if self.completer.active() {
            let items: Vec<String> = self
                .completer
                .items()
                .iter()
                .map(|i| match &i.detail {
                    Some(d) => format!("{}  {d}", i.insert),
                    None => i.insert.clone(),
                })
                .collect();
            live.extend(list(&items, self.drop_sel, 8, w));
        }
        if self.debug {
            live.push(Line::styled(
                format!("draw {:?}", self.r.last_draw()),
                dim(),
            ));
        }
        self.r.draw(&live, cursor)
    }
}

/// A key as extensions name it: "ctrl+t", "alt+x", "f5", "enter", "up".
/// A plain character is typing, not a shortcut, so it has no name here.
/// A `/skill:name` message is long; the screen shows its name and what
/// followed it.
fn shown_user_text(t: &str) -> String {
    let Some(rest) = t.strip_prefix("<skill name=\"") else {
        return t.to_owned();
    };
    let Some((name, _)) = rest.split_once('"') else {
        return t.to_owned();
    };
    match rest.split_once("\n</skill>") {
        Some((_, after)) if !after.trim().is_empty() => {
            format!("[skill {name}] {}", after.trim())
        }
        Some(_) => format!("[skill {name}]"),
        None => t.to_owned(),
    }
}

pub fn key_name(k: &KeyEvent) -> Option<String> {
    let base = match k.code {
        KeyCode::Char(c) => c.to_ascii_lowercase().to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "escape".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Backspace => "backspace".into(),
        _ => return None,
    };
    let mut name = String::new();
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        name.push_str("ctrl+");
    }
    if k.modifiers.contains(KeyModifiers::ALT) {
        name.push_str("alt+");
    }
    if name.is_empty() && matches!(k.code, KeyCode::Char(_)) {
        return None;
    }
    name.push_str(&base);
    Some(name)
}

/// /stats: tokens, timing, guards and asks on the branch.
fn stats(branch: &[&Entry]) -> Vec<String> {
    let (mut input, mut output, mut cached, mut steps) = (0, 0, 0, 0);
    let (mut guards, mut asks, mut denied, mut blocked, mut calls, mut errors) = (0, 0, 0, 0, 0, 0);
    let mut ttft = Vec::new();
    for e in branch {
        match &e.body {
            EntryBody::Message {
                role: Role::Assistant,
                usage,
                ..
            } => {
                steps += 1;
                if let Some(u) = usage {
                    input += u.input;
                    output += u.output;
                    cached += u.cache_read;
                }
            }
            EntryBody::ToolResult { is_error, .. } => {
                calls += 1;
                if *is_error {
                    errors += 1;
                }
            }
            EntryBody::GuardEvent { .. } => guards += 1,
            EntryBody::PolicyDecision { decision, .. } => match decision.as_str() {
                "denied" => {
                    asks += 1;
                    denied += 1
                }
                "blocked" | "rerun_outside" | "dir_added" => blocked += 1,
                "refused" | "granted" => {}
                _ => asks += 1,
            },
            EntryBody::Custom { custom_type, data } if custom_type == "timing" => {
                if let Some(t) = data["first_token_ms"].as_u64() {
                    ttft.push(t);
                }
            }
            _ => {}
        }
    }
    let mean = |v: &[u64]| {
        if v.is_empty() {
            0
        } else {
            v.iter().sum::<u64>() / v.len() as u64
        }
    };
    vec![
        String::new(),
        format!("model requests {steps}, tool calls {calls} ({errors} failed)"),
        format!("tokens in {input} (+{cached} cached), out {output}"),
        format!(
            "time to first token: mean {}ms over {} requests",
            mean(&ttft),
            ttft.len()
        ),
        format!("asks {asks} ({denied} denied), blocked writes {blocked}, guard events {guards}"),
    ]
}
