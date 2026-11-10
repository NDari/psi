//! The coding tasks under `evals/code/` and its sets: the quick set
//! (`SET`), and any other file with an upper-case name (`TRIAL` is
//! `--tasks trial`).

use std::path::{Path, PathBuf};

use crate::error::other;
use crate::{Result, fsutil};

/// One coding task: `prompt.md`, the starting project (`repo/`, or
/// `source` naming a local git repository and a ref), `hidden/`, and for
/// the tasks whose graders are checked a `reference/` solution.
#[derive(Debug, Clone)]
pub struct Task {
    pub name: String,
    pub dir: PathBuf,
    /// How many check lines `hidden/check.sh` prints.
    pub total: u32,
}

impl Task {
    pub fn prompt(&self) -> Result<String> {
        fsutil::read_to_string(&self.dir.join("prompt.md"))
    }

    /// Whether the task has a reference solution to check its grader with.
    pub fn has_reference(&self) -> bool {
        self.dir.join("reference").is_dir()
    }

    /// Puts the task's starting project at `dest`: a copy of `repo/`, or
    /// the files of the ref `source` names (a task from a real project),
    /// then the task's `setup.sh` run there on the host, if it has one
    /// (trials have no network: it fetches what the project needs).
    pub fn materialize(&self, dest: &Path) -> Result<()> {
        let source = self.dir.join("source");
        if source.is_file() {
            let (repo, git_ref) = read_source(&source)?;
            fsutil::create_dir_all(dest)?;
            let script = "git -C \"$1\" archive --format=tar \"$2\" | tar -x -C \"$3\"";
            let done = std::process::Command::new("bash")
                .args(["-o", "pipefail", "-c", script, "archive"])
                .arg(&repo)
                .arg(&git_ref)
                .arg(dest)
                .output()
                .map_err(|e| other(format!("task {}: running git archive: {e}", self.name)))?;
            if !done.status.success() {
                return Err(other(format!(
                    "task {}: cannot take {git_ref} from {}: {}",
                    self.name,
                    repo.display(),
                    String::from_utf8_lossy(&done.stderr).trim()
                )));
            }
        } else {
            fsutil::copy_dir(&self.dir.join("repo"), dest, &[])?;
        }
        // Absolute: it runs with the project as its working directory.
        let setup = self.dir.join("setup.sh");
        let setup = std::path::absolute(&setup).unwrap_or(setup);
        if setup.is_file() {
            let done = std::process::Command::new("bash")
                .arg(&setup)
                .current_dir(dest)
                .output()
                .map_err(|e| other(format!("task {}: running setup.sh: {e}", self.name)))?;
            if !done.status.success() {
                return Err(other(format!(
                    "task {}: setup.sh failed: {}",
                    self.name,
                    String::from_utf8_lossy(&done.stderr).trim()
                )));
            }
        }
        Ok(())
    }
}

/// A `source` file: one line, a path to a local git repository (`~/` is
/// the home directory) and a ref; `#` lines are comments.
fn read_source(path: &Path) -> Result<(PathBuf, String)> {
    let text = fsutil::read_to_string(path)?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("");
    let mut parts = line.split_whitespace();
    let (Some(repo), Some(git_ref), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(other(format!(
            "{}: write it as `<path to a git repository> <ref>`",
            path.display()
        )));
    };
    let repo = match (repo.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(repo),
    };
    Ok((repo, git_ref.to_owned()))
}

/// Which tasks to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// The names in `SET`.
    Quick,
    /// Every task directory.
    All,
    Names(Vec<String>),
}

impl Selection {
    /// `quick`, `all`, or names of tasks and sets separated by commas or
    /// spaces.
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "" | "quick" => Selection::Quick,
            "all" => Selection::All,
            list => Selection::Names(
                list.split([',', ' '])
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ),
        }
    }
}

const REQUIRED: [&str; 3] = ["prompt.md", "hidden/check.sh", "hidden/total"];

/// Loads the selected tasks from `code_dir`, checking each has its files.
pub fn load_tasks(code_dir: &Path, sel: &Selection) -> Result<Vec<Task>> {
    let names = match sel {
        Selection::Quick => read_set(&code_dir.join("SET"))?,
        Selection::Names(n) => {
            let mut names = Vec::new();
            for name in n {
                match set_file(code_dir, name) {
                    Some(set) => names.extend(read_set(&set)?),
                    None => names.push(name.clone()),
                }
            }
            names
        }
        Selection::All => {
            let mut names = Vec::new();
            let entries = std::fs::read_dir(code_dir)
                .map_err(|e| other(format!("reading {}: {e}", code_dir.display())))?;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // `_lib` holds the graders' helpers.
                if entry.path().is_dir() && !name.starts_with('_') {
                    names.push(name);
                }
            }
            names.sort();
            names
        }
    };
    if names.is_empty() {
        return Err(other("no tasks selected"));
    }
    names.iter().map(|n| load_task(code_dir, n)).collect()
}

fn load_task(code_dir: &Path, name: &str) -> Result<Task> {
    let dir = code_dir.join(name);
    for f in REQUIRED {
        if !dir.join(f).exists() {
            return Err(other(format!("task {name} has no {f}")));
        }
    }
    if !dir.join("repo").is_dir() && !dir.join("source").is_file() {
        return Err(other(format!(
            "task {name} has no repo/ and no source file"
        )));
    }
    let total_text = fsutil::read_to_string(&dir.join("hidden/total"))?;
    let total = total_text
        .trim()
        .parse()
        .map_err(|_| other(format!("task {name}: hidden/total is not a number")))?;
    Ok(Task {
        name: name.to_owned(),
        dir,
        total,
    })
}

/// The set file a name stands for: `trial` is `TRIAL`, when there is such
/// a file and no task by that name.
fn set_file(code_dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    let set = code_dir.join(name.to_uppercase());
    (!code_dir.join(name).is_dir() && set.is_file()).then_some(set)
}

/// Reads a set: one task name per line; blank lines, `#` comments and
/// `@` options are skipped.
fn read_set(path: &Path) -> Result<Vec<String>> {
    Ok(fsutil::read_to_string(path)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(['#', '@']))
        .map(str::to_owned)
        .collect())
}

/// The model requests a set asks for per trial (a line `@steps 120`),
/// when the selection is that one set.
pub fn set_steps(code_dir: &Path, sel: &Selection) -> Option<u32> {
    let path = match sel {
        Selection::Quick => code_dir.join("SET"),
        Selection::Names(n) if n.len() == 1 => set_file(code_dir, &n[0])?,
        _ => return None,
    };
    fsutil::read_to_string(&path)
        .ok()?
        .lines()
        .find_map(|l| l.trim().strip_prefix("@steps ")?.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evals_code() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/code")
    }

    #[test]
    fn selection_parses() {
        assert_eq!(Selection::parse("quick"), Selection::Quick);
        assert_eq!(Selection::parse("all"), Selection::All);
        assert_eq!(
            Selection::parse("a,b c"),
            Selection::Names(vec!["a".into(), "b".into(), "c".into()])
        );
    }

    #[test]
    fn shipped_tasks_load() {
        let all = load_tasks(&evals_code(), &Selection::All).unwrap();
        assert_eq!(all.len(), 43);
        // All but the task from a real project carry a reference solution.
        let without: Vec<&str> = all
            .iter()
            .filter(|t| !t.has_reference())
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(without, ["cog_bare_words", "cog_bare_words_plan"]);
        let quick = load_tasks(&evals_code(), &Selection::Quick).unwrap();
        assert_eq!(quick.len(), 10);
        assert!(quick.iter().all(|t| t.total > 0));
        // A set by its name, alone or with tasks; its own request limit.
        let trial = Selection::parse("trial");
        let tasks = load_tasks(&evals_code(), &trial).unwrap();
        assert_eq!(tasks.len(), 10);
        assert_eq!(set_steps(&evals_code(), &trial), Some(120));
        assert_eq!(set_steps(&evals_code(), &Selection::Quick), None);
        let mixed = Selection::parse("trial,fix_pagination");
        assert_eq!(load_tasks(&evals_code(), &mixed).unwrap().len(), 11);
        assert_eq!(set_steps(&evals_code(), &mixed), None);
    }

    /// A task from a real project: the files of a ref in a local git
    /// repository, then the task's own setup, with nothing of `.git`.
    #[test]
    fn a_source_task_takes_a_ref_from_a_local_repository() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("proj");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(&repo)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(repo.join("a.txt"), "pinned\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "one"]);
        git(&["branch", "start"]);
        // Later work in the repository is not part of the task.
        std::fs::write(repo.join("a.txt"), "later\n").unwrap();
        git(&["commit", "-q", "-am", "two"]);

        let code = dir.path().join("code");
        let task = code.join("real");
        std::fs::create_dir_all(task.join("hidden")).unwrap();
        std::fs::write(task.join("prompt.md"), "do it").unwrap();
        std::fs::write(task.join("hidden/check.sh"), "").unwrap();
        std::fs::write(task.join("hidden/total"), "1").unwrap();
        let err = load_tasks(&code, &Selection::parse("real")).unwrap_err();
        assert_eq!(err.to_string(), "task real has no repo/ and no source file");
        std::fs::write(
            task.join("source"),
            format!("# where it starts\n{} start\n", repo.display()),
        )
        .unwrap();
        std::fs::write(task.join("setup.sh"), "echo fetched > deps.txt\n").unwrap();
        let t = load_tasks(&code, &Selection::parse("real"))
            .unwrap()
            .remove(0);
        assert!(!t.has_reference());
        let dest = dir.path().join("out/project");
        t.materialize(&dest).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("a.txt")).unwrap(),
            "pinned\n"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("deps.txt")).unwrap(),
            "fetched\n"
        );
        assert!(!dest.join(".git").exists());
        // A ref that isn't there says so.
        std::fs::write(task.join("source"), format!("{} nope\n", repo.display())).unwrap();
        let err = t
            .materialize(&dir.path().join("out2"))
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("task real: cannot take nope from "),
            "{err}"
        );
    }

    #[test]
    fn missing_file_is_named() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("t/repo")).unwrap();
        let err = load_tasks(dir.path(), &Selection::Names(vec!["t".into()])).unwrap_err();
        assert_eq!(err.to_string(), "task t has no prompt.md");
    }
}
