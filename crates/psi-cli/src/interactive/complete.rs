//! Completion under the input box: slash commands at the start of the
//! input, and `@` file references with fuzzy matching (§11.4).

use std::path::{Path, PathBuf};

use super::ui::COMMANDS;

/// Files listed for `@` completion, at most.
const MAX_FILES: usize = 20_000;
const MAX_ITEMS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What goes in the box.
    pub insert: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Command,
    File,
}

pub struct Completer {
    root: PathBuf,
    files: Option<Vec<String>>,
    items: Vec<Item>,
    kind: Option<Kind>,
    /// The box just took a selection; the next update keeps the list.
    hold: bool,
}

impl Completer {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            files: None,
            items: Vec::new(),
            kind: None,
            hold: false,
        }
    }

    pub fn active(&self) -> bool {
        self.kind.is_some() && !self.items.is_empty()
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn close(&mut self) {
        self.kind = None;
        self.items.clear();
        self.hold = false;
    }

    pub fn keep_open(&mut self) {
        self.hold = true;
    }

    /// The word before the cursor changed.
    pub fn update(&mut self, word: &str, at_start: bool) {
        if std::mem::take(&mut self.hold) {
            return;
        }
        if at_start && word.starts_with('/') {
            self.kind = Some(Kind::Command);
            self.items = COMMANDS
                .iter()
                .filter(|(c, _)| c.starts_with(word) && *c != word)
                .map(|(c, d)| Item {
                    insert: (*c).to_owned(),
                    detail: Some((*d).to_owned()),
                })
                .collect();
        } else if let Some(q) = word.strip_prefix('@') {
            self.kind = Some(Kind::File);
            let files = self.files.get_or_insert_with(|| list_files(&self.root));
            let mut scored: Vec<(i64, &String)> = files
                .iter()
                .filter_map(|f| fuzzy(q, f).map(|s| (s, f)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
            self.items = scored
                .into_iter()
                .take(MAX_ITEMS)
                .map(|(_, f)| Item {
                    insert: format!("@{f}"),
                    detail: None,
                })
                .collect();
        } else {
            self.close();
        }
    }
}

/// The project's files, .gitignore-aware, relative to `root`.
fn list_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in ignore::WalkBuilder::new(root)
        .hidden(true)
        .build()
        .flatten()
    {
        if out.len() >= MAX_FILES {
            break;
        }
        if e.file_type().is_some_and(|t| t.is_file())
            && let Ok(rel) = e.path().strip_prefix(root)
        {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

/// A subsequence match score: higher is better, `None` when `q`'s
/// characters don't all appear in order. Matches at the start of a path
/// part and runs of adjacent matches score more.
pub fn fuzzy(q: &str, s: &str) -> Option<i64> {
    if q.is_empty() {
        return Some(0);
    }
    let sc: Vec<char> = s.chars().collect();
    let mut score = 0i64;
    let mut last: Option<usize> = None;
    let mut i = 0;
    for qc in q.chars() {
        let qc = qc.to_ascii_lowercase();
        let pos = (i..sc.len()).find(|&k| sc[k].to_ascii_lowercase() == qc)?;
        score += 1;
        if pos == 0 || matches!(sc[pos - 1], '/' | '_' | '-' | '.') {
            score += 8;
        }
        if last.is_some_and(|l| l + 1 == pos) {
            score += 5;
        }
        last = Some(pos);
        i = pos + 1;
    }
    // Prefer matches in the file name.
    if let Some(slash) = s.rfind('/')
        && last.is_some_and(|l| l > slash)
    {
        score += 3;
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_starts_and_runs() {
        assert!(fuzzy("mn", "src/main.rs").is_some());
        assert!(fuzzy("xyz", "src/main.rs").is_none());
        let a = fuzzy("main", "src/main.rs").unwrap();
        let b = fuzzy("main", "src/domain.rs").unwrap();
        assert!(a > b);
    }

    #[test]
    fn commands_at_the_start_and_files_after_at() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/main.rs"), "").unwrap();
        std::fs::write(d.path().join("README.md"), "").unwrap();
        let mut c = Completer::new(d.path());
        c.update("/un", true);
        assert_eq!(c.items()[0].insert, "/undo");
        c.update("/un", false);
        assert!(!c.active());
        c.update("@main", false);
        assert_eq!(c.items()[0].insert, "@src/main.rs");
        c.keep_open();
        c.update("@src/main.rs", false);
        assert!(c.active(), "a selection keeps the list open");
        c.update("hello", false);
        assert!(!c.active());
    }
}
