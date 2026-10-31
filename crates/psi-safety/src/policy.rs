//! What runs, what asks and what is refused (psi-plan.md §8.1, D-11, D-27).
//!
//! Reversible work runs freely; the irreversible asks in every mode. Each
//! ask carries grant keys: a session grant (`s` on the card, or `--allow`
//! for list entries) covers later calls whose keys are all granted.

use std::path::{Path, PathBuf};

use crate::always_ask::{Rules, Verdict, classify};
use crate::blocked::resolve_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Asks only for the always-ask cases.
    Auto,
    /// Also asks before every write, edit and bash call.
    Strict,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Mode::Auto),
            "strict" => Some(Mode::Strict),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Strict => "strict",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Ask the user; `keys` are what a session grant covers.
    Ask {
        reason: String,
        keys: Vec<String>,
    },
    /// Not run, and nothing to ask: the text goes to the model.
    Refuse(String),
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub mode: Mode,
    pub rules: Rules,
    /// Why bash isn't isolated, when no backend works: every call asks.
    pub unisolated: Option<String>,
    /// The workspace root.
    pub root: PathBuf,
    /// Psi's own configuration (`~/.psi`, `<root>/.psi`): file tools refuse
    /// to write there (D-27).
    pub protected: Vec<PathBuf>,
    /// The session's TMPDIR, writable without asking.
    pub tmpdir: PathBuf,
}

/// What the file tools need to know about a target, from checkpoints.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileFacts {
    /// Gitignored, so checkpoints don't capture it.
    pub ignored: bool,
    /// The last capture failed: changes can't be undone right now.
    pub capture_failing: bool,
}

impl Policy {
    pub fn new(mode: Mode, rules: Rules, root: &Path, home: Option<&Path>, tmpdir: &Path) -> Self {
        let mut protected = vec![root.join(".psi")];
        if let Some(h) = home {
            protected.push(h.join(".psi"));
        }
        Self {
            mode,
            rules,
            unisolated: None,
            root: resolve_path(root),
            protected: protected.iter().map(|p| resolve_path(p)).collect(),
            tmpdir: resolve_path(tmpdir),
        }
    }

    /// A bash call.
    pub fn bash(&self, command: &str) -> Decision {
        let mut reasons = Vec::new();
        let mut keys = Vec::new();
        match classify(command, &self.rules) {
            Verdict::Clear => {}
            Verdict::Ask(ids) => {
                let list: Vec<String> = ids.iter().map(|i| format!("`{i}`")).collect();
                reasons.push(format!("{} is on the always-ask list", list.join(", ")));
                keys.extend(ids);
            }
            Verdict::Unparseable(why) => {
                reasons.push(format!(
                    "{why}, and bash would run the lines before the error"
                ));
                keys.push(format!("unparseable:{command}"));
            }
        }
        if let Some(why) = &self.unisolated {
            reasons.push(format!("bash is not isolated ({why})"));
            keys.push("unisolated".into());
        }
        if self.mode == Mode::Strict {
            reasons.push("strict mode asks before every command".into());
            keys.push("strict:bash".into());
        }
        ask_if(reasons, keys)
    }

    /// A write or edit of `path` (absolute); `shown` is the path as the
    /// model wrote it, for the message.
    pub fn file_write(&self, tool: &str, path: &Path, shown: &str, facts: FileFacts) -> Decision {
        let real = resolve_path(path);
        if let Some(p) = self.protected.iter().find(|p| real.starts_with(p)) {
            return Decision::Refuse(format!(
                "{shown} is inside {}, psi's own configuration, which file tools don't change. If it needs changing, tell the user what to change.",
                p.display()
            ));
        }
        let mut reasons = Vec::new();
        let mut keys = Vec::new();
        if !real.starts_with(&self.root) && !real.starts_with(&self.tmpdir) {
            reasons.push(format!(
                "{shown} is outside the workspace, so checkpoints can't undo it"
            ));
            keys.push(format!("outside:{}", real.display()));
        } else if facts.ignored {
            reasons.push(format!(
                "{shown} is gitignored, so checkpoints can't undo it"
            ));
            keys.push(format!("ignored:{}", real.display()));
        }
        if facts.capture_failing {
            reasons.push("checkpoints are failing, so the change can't be undone".into());
            keys.push("capture_failing".into());
        }
        if self.mode == Mode::Strict {
            reasons.push("strict mode asks before every change".into());
            keys.push(format!("strict:{tool}"));
        }
        ask_if(reasons, keys)
    }
}

impl Policy {
    /// An extension's tool: `always_ask` asks every time; a script tool runs
    /// inside the bash boundary, so it asks when there is none; strict mode
    /// asks for any tool that isn't read-only.
    pub fn ext_tool(
        &self,
        name: &str,
        read_only: bool,
        always_ask: bool,
        script: bool,
    ) -> Decision {
        let mut reasons = Vec::new();
        let mut keys = Vec::new();
        if always_ask {
            reasons.push(format!("the {name} tool always asks"));
            keys.push(format!("tool:{name}"));
        }
        if script && let Some(why) = &self.unisolated {
            reasons.push(format!("script tools are not isolated ({why})"));
            keys.push("unisolated".into());
        }
        if self.mode == Mode::Strict && !read_only {
            reasons.push("strict mode asks before every change".into());
            keys.push(format!("strict:{name}"));
        }
        ask_if(reasons, keys)
    }
}

fn ask_if(reasons: Vec<String>, keys: Vec<String>) -> Decision {
    if reasons.is_empty() {
        Decision::Allow
    } else {
        Decision::Ask {
            reason: reasons.join("; "),
            keys,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(mode: Mode) -> Policy {
        Policy::new(
            mode,
            Rules::shipped(),
            Path::new("/w/p"),
            Some(Path::new("/h")),
            Path::new("/h/.cache/psi/tmp/s"),
        )
    }

    fn keys(d: Decision) -> Vec<String> {
        match d {
            Decision::Ask { keys, .. } => keys,
            other => panic!("expected an ask: {other:?}"),
        }
    }

    #[test]
    fn bash_asks_for_list_entries_unparseable_and_strict() {
        let p = policy(Mode::Auto);
        assert_eq!(p.bash("cargo test && git status"), Decision::Allow);
        assert_eq!(keys(p.bash("make && git push")), ["git push"]);
        assert_eq!(keys(p.bash("echo \"x")), ["unparseable:echo \"x"]);
        let s = policy(Mode::Strict);
        assert_eq!(keys(s.bash("ls")), ["strict:bash"]);
        assert_eq!(keys(s.bash("git push")), ["git push", "strict:bash"]);
        let mut u = policy(Mode::Auto);
        u.unisolated = Some("bwrap missing".into());
        let Decision::Ask { reason, keys } = u.bash("ls") else {
            panic!()
        };
        assert_eq!(keys, ["unisolated"]);
        assert_eq!(reason, "bash is not isolated (bwrap missing)");
    }

    #[test]
    fn file_writes() {
        let p = policy(Mode::Auto);
        let f = FileFacts::default();
        assert_eq!(
            p.file_write("write", Path::new("/w/p/src/a.py"), "/w/p/src/a.py", f),
            Decision::Allow
        );
        assert_eq!(
            p.file_write(
                "write",
                Path::new("/h/.cache/psi/tmp/s/x"),
                "/h/.cache/psi/tmp/s/x",
                f
            ),
            Decision::Allow
        );
        assert_eq!(
            keys(p.file_write("write", Path::new("/etc/hosts"), "/etc/hosts", f)),
            ["outside:/etc/hosts"]
        );
        // `..` doesn't get out unnoticed.
        assert_eq!(
            keys(p.file_write("edit", Path::new("/w/p/../q/x"), "/w/p/../q/x", f)),
            ["outside:/w/q/x"]
        );
        let ignored = FileFacts { ignored: true, ..f };
        assert_eq!(
            keys(p.file_write("write", Path::new("/w/p/.env"), "/w/p/.env", ignored)),
            ["ignored:/w/p/.env"]
        );
        let failing = FileFacts {
            capture_failing: true,
            ..f
        };
        assert_eq!(
            keys(p.file_write("write", Path::new("/w/p/a"), "/w/p/a", failing)),
            ["capture_failing"]
        );
        assert_eq!(
            keys(policy(Mode::Strict).file_write("edit", Path::new("/w/p/a"), "/w/p/a", f)),
            ["strict:edit"]
        );
    }

    #[test]
    fn extension_tools() {
        let p = policy(Mode::Auto);
        assert_eq!(p.ext_tool("lint", true, false, false), Decision::Allow);
        assert_eq!(
            keys(p.ext_tool("deploy", false, true, false)),
            ["tool:deploy"]
        );
        let s = policy(Mode::Strict);
        assert_eq!(s.ext_tool("lint", true, false, true), Decision::Allow);
        assert_eq!(keys(s.ext_tool("fmt", false, false, false)), ["strict:fmt"]);
        let mut u = policy(Mode::Auto);
        u.unisolated = Some("no bwrap".into());
        assert_eq!(keys(u.ext_tool("wc", true, false, true)), ["unisolated"]);
        assert_eq!(u.ext_tool("lua_tool", true, false, false), Decision::Allow);
    }

    #[test]
    fn psi_config_is_refused() {
        let p = policy(Mode::Auto);
        for path in [
            "/w/p/.psi/settings.jsonc",
            "/h/.psi/models.jsonc",
            "/w/p/x/../.psi/a",
        ] {
            let d = p.file_write("write", Path::new(path), path, FileFacts::default());
            assert!(
                matches!(&d, Decision::Refuse(m) if m.contains("psi's own configuration")),
                "{path}: {d:?}"
            );
        }
    }
}
