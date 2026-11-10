//! What runs and what is refused (design-going-forward.md, "Refuse, never
//! ask"). Inside a run an action is undoable or impossible: work inside the
//! workspace runs freely and checkpoints undo it; the rest is refused, with
//! what to do instead. Nothing asks.

use std::path::{Path, PathBuf};

use crate::blocked::resolve_path;
use crate::refuse::{Rules, Verdict, classify};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Not run: the text goes to the model.
    Refuse(String),
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub rules: Rules,
    /// Why bash isn't isolated, when no backend works: bash is refused.
    pub unisolated: Option<String>,
    /// The workspace root.
    pub root: PathBuf,
    /// The session's TMPDIR, writable like the workspace.
    pub tmpdir: PathBuf,
    /// Credential files and directories ([`crate::secrets`]): never read.
    pub hidden: Vec<PathBuf>,
}

impl Policy {
    pub fn new(rules: Rules, root: &Path, home: Option<&Path>, tmpdir: &Path) -> Self {
        let hidden = home
            .map(|h| crate::secrets::hidden_paths(h, None))
            .unwrap_or_default();
        Self {
            rules,
            unisolated: None,
            root: resolve_path(root),
            tmpdir: resolve_path(tmpdir),
            hidden: hidden.iter().map(|p| resolve_path(p)).collect(),
        }
    }

    /// A bash call.
    pub fn bash(&self, command: &str) -> Decision {
        if let Some(why) = &self.unisolated {
            return Decision::Refuse(format!(
                "Not run: bash is off, because it can't be isolated here ({why}). Use read, write and edit, and say in your answer what you needed to run."
            ));
        }
        match classify(command, &self.rules) {
            // What doesn't parse runs: bash names the syntax error.
            Verdict::Clear | Verdict::Unparseable(_) => Decision::Allow,
            Verdict::Refuse(ids) => {
                let list: Vec<String> = ids.iter().map(|i| format!("`{i}`")).collect();
                Decision::Refuse(format!(
                    "Not run: psi refuses {}, and the command contains it. Leave it to the user: do the rest without it, and say in your answer that it still has to be done.",
                    list.join(" and ")
                ))
            }
        }
    }

    /// A read of `path` (absolute); `shown` is the path as the model wrote it.
    pub fn file_read(&self, path: &Path, shown: &str) -> Decision {
        let real = resolve_path(path);
        if self.hidden.iter().any(|h| real.starts_with(h)) {
            return Decision::Refuse(format!(
                "Not read: {shown} holds credentials, which psi keeps out of reach. Do without it."
            ));
        }
        Decision::Allow
    }

    /// A write or edit of `path` (absolute).
    pub fn file_write(&self, path: &Path, shown: &str) -> Decision {
        let real = resolve_path(path);
        if real.starts_with(&self.root) || real.starts_with(&self.tmpdir) {
            return Decision::Allow;
        }
        Decision::Refuse(format!(
            "Not written: {shown} is outside the workspace ({}), and psi only changes files inside it. If the task needs it, say so in your answer.",
            self.root.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy::new(
            Rules::shipped(),
            Path::new("/w/p"),
            Some(Path::new("/h")),
            Path::new("/h/.cache/psi/tmp/s"),
        )
    }

    fn refusal(d: Decision) -> String {
        match d {
            Decision::Refuse(m) => m,
            Decision::Allow => panic!("expected a refusal"),
        }
    }

    #[test]
    fn bash_refuses_list_entries_and_runs_the_rest() {
        let p = policy();
        assert_eq!(p.bash("cargo test && git status"), Decision::Allow);
        // History rewrites run: checkpoints record the branch tips.
        assert_eq!(
            p.bash("git checkout -b x && git commit -am x"),
            Decision::Allow
        );
        let m = refusal(p.bash("make && git push"));
        assert!(m.starts_with("Not run: psi refuses `git push`"), "{m}");
        let m = refusal(p.bash("sudo kubectl apply"));
        assert!(m.contains("`sudo` and `kubectl`"), "{m}");
        // bash reports a syntax error better than a refusal would.
        assert_eq!(p.bash("echo \"x"), Decision::Allow);
        let mut u = policy();
        u.unisolated = Some("bwrap missing".into());
        let m = refusal(u.bash("ls"));
        assert!(
            m.contains("bash is off") && m.contains("bwrap missing"),
            "{m}"
        );
    }

    #[test]
    fn writes_stay_inside_the_workspace() {
        let p = policy();
        for inside in ["/w/p/src/a.py", "/w/p/.env", "/w/p/.psi/settings.jsonc"] {
            assert_eq!(p.file_write(Path::new(inside), inside), Decision::Allow);
        }
        assert_eq!(
            p.file_write(Path::new("/h/.cache/psi/tmp/s/x"), "x"),
            Decision::Allow
        );
        // Not /etc: on macOS it resolves to /private/etc.
        let m = refusal(p.file_write(Path::new("/usr/x"), "/usr/x"));
        assert!(
            m.starts_with("Not written: /usr/x is outside the workspace (/w/p)"),
            "{m}"
        );
        // `..` doesn't get out unnoticed, and psi's own settings are outside.
        for outside in ["/w/p/../q/x", "/h/.psi/settings.jsonc"] {
            refusal(p.file_write(Path::new(outside), outside));
        }
    }

    #[test]
    fn credentials_are_not_read() {
        let p = policy();
        for path in ["/h/.ssh/id_ed25519", "/h/.config/gh/hosts.yml", "/h/.netrc"] {
            let m = refusal(p.file_read(Path::new(path), path));
            assert!(m.contains("holds credentials"), "{m}");
        }
        for path in ["/h/.gitconfig", "/w/p/src/a.py", "/usr/include/stdio.h"] {
            assert_eq!(p.file_read(Path::new(path), path), Decision::Allow);
        }
    }
}
