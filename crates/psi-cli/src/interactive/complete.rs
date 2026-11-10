//! Completion under the input box: slash commands at the start of the
//! input, and `@` file references with fuzzy matching (§11.4).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

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

/// The project's files, walked in the background so typing never waits.
#[derive(Default)]
struct FileList {
    files: Vec<String>,
    walking: bool,
    /// A walk finished since the list was last used.
    fresh: bool,
}

pub struct Completer {
    root: PathBuf,
    files: Arc<Mutex<FileList>>,
    matcher: Matcher,
    items: Vec<Item>,
    kind: Option<Kind>,
    /// The box just took a selection; the next update keeps the list.
    hold: bool,
}

impl Completer {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            files: Arc::default(),
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
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
            let all = COMMANDS
                .iter()
                .map(|(c, d)| ((*c).to_owned(), (*d).to_owned()));
            self.items = all
                .filter(|(c, _)| c.starts_with(word) && c != word)
                .map(|(c, d)| Item {
                    insert: c,
                    detail: Some(d),
                })
                .collect();
        } else if let Some(q) = word.strip_prefix('@') {
            // Each time the list opens, walk again: files come and go
            // during a session. The previous list serves until then.
            if self.kind != Some(Kind::File) {
                self.walk();
            }
            self.kind = Some(Kind::File);
            let list = self.files.lock().map(|mut l| {
                l.fresh = false;
                l.files.clone()
            });
            let files = list.unwrap_or_default();
            let pattern = Pattern::parse(q, CaseMatching::Smart, Normalization::Smart);
            let mut scored = pattern.match_list(files.iter(), &mut self.matcher);
            // Shorter paths first among equal scores.
            scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.len().cmp(&b.0.len())));
            self.items = scored
                .into_iter()
                .take(MAX_ITEMS)
                .map(|(f, _)| Item {
                    insert: format!("@{f}"),
                    detail: None,
                })
                .collect();
        } else {
            self.close();
        }
    }
}

impl Completer {
    /// Starts a walk of the project in the background, unless one is going.
    fn walk(&self) {
        let Ok(mut l) = self.files.lock() else { return };
        if l.walking {
            return;
        }
        l.walking = true;
        let (files, root) = (self.files.clone(), self.root.clone());
        std::thread::spawn(move || {
            let list = list_files(&root);
            if let Ok(mut l) = files.lock() {
                l.files = list;
                l.walking = false;
                l.fresh = true;
            }
        });
    }

    /// A walk finished while the file list is open: the caller updates it.
    pub fn take_fresh(&mut self) -> bool {
        self.kind == Some(Kind::File)
            && self
                .files
                .lock()
                .is_ok_and(|mut l| std::mem::take(&mut l.fresh))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens the file list at `word` and waits for the background walk,
    /// as the UI does.
    fn open_files(c: &mut Completer, word: &str) {
        c.close();
        c.update(word, false);
        let start = std::time::Instant::now();
        while !c.take_fresh() {
            assert!(start.elapsed().as_secs() < 5, "the walk never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        c.update(word, false);
    }

    #[test]
    fn file_names_rank_above_scattered_matches() {
        let d = tempfile::tempdir().unwrap();
        for f in ["src/main.rs", "src/domain.rs", "docs/maintenance/notes.md"] {
            let p = d.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        let mut c = Completer::new(d.path());
        open_files(&mut c, "@main");
        assert_eq!(c.items()[0].insert, "@src/main.rs");
        open_files(&mut c, "@xyz");
        assert!(!c.active());
    }

    #[test]
    fn files_made_during_the_session_show_up_next_time() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("old.txt"), "").unwrap();
        let mut c = Completer::new(d.path());
        open_files(&mut c, "@txt");
        assert_eq!(c.items().len(), 1);
        std::fs::write(d.path().join("new.txt"), "").unwrap();
        open_files(&mut c, "@txt");
        let names: Vec<&str> = c.items().iter().map(|i| i.insert.as_str()).collect();
        assert_eq!(names, ["@new.txt", "@old.txt"]);
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
        open_files(&mut c, "@main");
        assert_eq!(c.items()[0].insert, "@src/main.rs");
        c.keep_open();
        c.update("@src/main.rs", false);
        assert!(c.active(), "a selection keeps the list open");
        c.update("hello", false);
        assert!(!c.active());
    }
}
