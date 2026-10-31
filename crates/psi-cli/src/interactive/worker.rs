//! The agent side of the interactive mode: owns the agent, runs prompts
//! and the session commands that need it, and reports back to the UI.

use std::path::PathBuf;
use std::sync::Arc;

use psi_agent::recovery;
use psi_agent::{Agent, AgentEvent, RunOutcome};
use psi_safety::checkpoint::Checkpoints;
use psi_safety::policy::Mode;
use psi_session::{EntryBody, SessionFile};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::ui::UiMsg;

/// What the UI asks of the worker.
#[derive(Debug)]
pub enum Cmd {
    Prompt(String),
    /// `!cmd` output to add to the context of the next prompt.
    Context(String),
    Undo,
    /// Move the conversation (and with `files`, the files) to an entry.
    Goto {
        id: String,
        files: bool,
    },
    Diff,
    Mode(Mode),
    Label {
        id: String,
        label: Option<String>,
    },
    New,
    Resume(PathBuf),
    /// The whole session, for /tree, /rewind, /stats and /inspect.
    Entries,
}

/// Opens the checkpoints for a session file.
pub type OpenCheckpoints = Box<dyn Fn(&SessionFile) -> Option<Arc<Checkpoints>> + Send>;

pub struct Worker {
    pub agent: Agent,
    pub sessions_root: PathBuf,
    pub cwd: PathBuf,
    /// Opens the checkpoints for a session file (a new one for /new and /resume).
    pub open_checkpoints: OpenCheckpoints,
}

impl Worker {
    pub async fn run(mut self, mut rx: UnboundedReceiver<Cmd>, ui: UnboundedSender<UiMsg>) {
        while let Some(cmd) = rx.recv().await {
            let reply = match cmd {
                Cmd::Prompt(p) => {
                    let out: RunOutcome = self.agent.run(&p).await;
                    UiMsg::RunDone(out)
                }
                Cmd::Context(text) => match self.agent.add_context("bash", &text) {
                    Ok(()) => continue,
                    Err(e) => UiMsg::Error(e),
                },
                Cmd::Undo => self.undo(),
                Cmd::Goto { id, files } => self.goto(&id, files),
                Cmd::Diff => self.diff(),
                Cmd::Mode(m) => {
                    self.agent.safety_mut().policy.mode = m;
                    UiMsg::Info(format!("mode: {}", m.as_str()))
                }
                Cmd::Label { id, label } => {
                    match self.agent.session_mut().append(EntryBody::Label {
                        target_id: id,
                        label,
                    }) {
                        Ok(_) => UiMsg::Info("labelled".into()),
                        Err(e) => UiMsg::Error(e.to_string()),
                    }
                }
                Cmd::New => match SessionFile::create(&self.sessions_root, &self.cwd) {
                    Ok(s) => self.switch(s, "new session"),
                    Err(e) => UiMsg::Error(format!("cannot create a session: {e}")),
                },
                Cmd::Resume(p) => match SessionFile::open(&p) {
                    Ok(s) => self.switch(s, "resumed"),
                    Err(e) => UiMsg::Error(format!("cannot open {}: {e}", p.display())),
                },
                Cmd::Entries => UiMsg::Entries {
                    entries: self.agent.session().entries().to_vec(),
                    leaf: self.agent.session().leaf().map(str::to_owned),
                },
            };
            if ui.send(reply).is_err() {
                break;
            }
        }
    }

    fn checkpoints(&self) -> Result<Arc<Checkpoints>, Box<UiMsg>> {
        self.agent.safety().checkpoints.clone().ok_or_else(|| {
            Box::new(UiMsg::Error(
                "checkpoints are off (git is missing), so there is nothing to restore".into(),
            ))
        })
    }

    fn undo(&mut self) -> UiMsg {
        let cp = match self.checkpoints() {
            Ok(c) => c,
            Err(m) => return *m,
        };
        match recovery::undo(self.agent.session_mut(), &cp) {
            Ok(Some(r)) => UiMsg::Restored {
                what: "undo".into(),
                written: r.written.len(),
                deleted: r.deleted.len(),
                left_alone: r
                    .left_alone
                    .iter()
                    .map(|(p, why)| format!("{} ({why})", p.display()))
                    .collect(),
            },
            Ok(None) => UiMsg::Info("nothing to undo: the files match every earlier prompt".into()),
            Err(e) => UiMsg::Error(format!("undo failed: {e}")),
        }
    }

    fn goto(&mut self, id: &str, files: bool) -> UiMsg {
        if !files {
            return match self.agent.session_mut().set_leaf(id) {
                Ok(()) => UiMsg::Moved(id.to_owned()),
                Err(e) => UiMsg::Error(e.to_string()),
            };
        }
        let cp = match self.checkpoints() {
            Ok(c) => c,
            Err(m) => return *m,
        };
        match recovery::rewind(self.agent.session_mut(), &cp, id) {
            Ok(Some(r)) => UiMsg::Restored {
                what: "rewind".into(),
                written: r.written.len(),
                deleted: r.deleted.len(),
                left_alone: r
                    .left_alone
                    .iter()
                    .map(|(p, why)| format!("{} ({why})", p.display()))
                    .collect(),
            },
            Ok(None) => UiMsg::Moved(id.to_owned()),
            Err(e) => UiMsg::Error(format!("rewind failed: {e}")),
        }
    }

    fn diff(&self) -> UiMsg {
        let cp = match self.checkpoints() {
            Ok(c) => c,
            Err(m) => return *m,
        };
        match recovery::session_changes(self.agent.session(), &cp) {
            Ok(changes) => UiMsg::Diff(changes),
            Err(e) => UiMsg::Error(format!("diff failed: {e}")),
        }
    }

    fn switch(&mut self, s: SessionFile, what: &str) -> UiMsg {
        let cp = (self.open_checkpoints)(&s);
        let path = s.path().to_owned();
        self.agent.set_session(s, cp);
        UiMsg::Switched {
            what: what.into(),
            path,
            entries: self.agent.session().entries().to_vec(),
        }
    }
}

/// Forwards agent events to the UI.
pub async fn forward(mut rx: UnboundedReceiver<AgentEvent>, ui: UnboundedSender<UiMsg>) {
    while let Some(ev) = rx.recv().await {
        if ui.send(UiMsg::Agent(ev)).is_err() {
            break;
        }
    }
}
