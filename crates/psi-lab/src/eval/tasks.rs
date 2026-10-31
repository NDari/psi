//! The coding tasks under `evals/code/` and the quick set (`SET`).

use std::path::{Path, PathBuf};

use crate::error::other;
use crate::{Result, fsutil};

/// One coding task: `prompt.md`, `repo/`, `reference/` and `hidden/`.
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
    /// `quick`, `all`, or task names separated by commas or spaces.
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

const REQUIRED: [&str; 5] = [
    "prompt.md",
    "repo",
    "reference",
    "hidden/check.sh",
    "hidden/total",
];

/// Loads the selected tasks from `code_dir`, checking each has its files.
pub fn load_tasks(code_dir: &Path, sel: &Selection) -> Result<Vec<Task>> {
    let names = match sel {
        Selection::Quick => read_set(&code_dir.join("SET"))?,
        Selection::Names(n) => n.clone(),
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

/// Reads `SET`: one name per line; blank lines and `#` comments are skipped.
fn read_set(path: &Path) -> Result<Vec<String>> {
    Ok(fsutil::read_to_string(path)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect())
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
        assert_eq!(all.len(), 40);
        let quick = load_tasks(&evals_code(), &Selection::Quick).unwrap();
        assert_eq!(quick.len(), 11);
        assert!(quick.iter().all(|t| t.total > 0));
    }

    #[test]
    fn missing_file_is_named() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("t/repo")).unwrap();
        let err = load_tasks(dir.path(), &Selection::Names(vec!["t".into()])).unwrap_err();
        assert_eq!(err.to_string(), "task t has no prompt.md");
    }
}
