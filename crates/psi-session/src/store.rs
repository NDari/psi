//! Session files on disk (psi-plan.md §7.1):
//! `~/.psi/sessions/<cwd-slug>/<UTC time>_<id>.jsonl`, appended as a run goes,
//! with the wire log beside it as `<same name>.wire.jsonl`.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use crate::time::{now_ms, utc_stamp};
use crate::{Entry, EntryBody, IdGen, SessionError, read_entries_lossy};

#[derive(Debug)]
pub struct SessionFile {
    path: PathBuf,
    file: File,
    entries: Vec<Entry>,
    ids: IdGen,
    /// The entry new ones attach to.
    leaf: Option<String>,
    /// Lines that couldn't be read when the file was opened.
    skipped: Vec<String>,
}

/// The directory name for a working directory: its path with `/` as `-`.
pub fn cwd_slug(cwd: &Path) -> String {
    let s = cwd.display().to_string();
    let s = s.trim_matches('/').replace('/', "-");
    format!("-{s}-")
}

impl SessionFile {
    /// A new, empty session for `cwd` under `root` (`~/.psi/sessions`).
    pub fn create(root: &Path, cwd: &Path) -> Result<Self, SessionError> {
        let dir = root.join(cwd_slug(cwd));
        std::fs::create_dir_all(&dir)?;
        let u = uuid::Uuid::now_v7().simple().to_string();
        let name = format!("{}_{}.jsonl", utc_stamp(now_ms()), &u[u.len() - 8..]);
        Self::create_at(&dir.join(name))
    }

    /// A new, empty session at `path` (a sub-agent's, next to its
    /// parent's), creating its directory.
    pub fn create_at(path: &Path) -> Result<Self, SessionError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            path: path.to_owned(),
            file,
            entries: Vec::new(),
            ids: IdGen::random_after(None),
            leaf: None,
            skipped: Vec::new(),
        })
    }

    /// A new session holding a copy of `history` (a branch, root first),
    /// for /fork: new entries continue from its last entry.
    pub fn fork(root: &Path, cwd: &Path, history: &[Entry]) -> Result<Self, SessionError> {
        let mut s = Self::create(root, cwd)?;
        for e in history {
            let mut line = serde_json::to_vec(e).map_err(std::io::Error::other)?;
            line.push(b'\n');
            s.file.write_all(&line)?;
        }
        s.file.flush()?;
        s.entries = history.to_vec();
        s.leaf = history.last().map(|e| e.id.clone());
        s.ids = IdGen::random_after(s.leaf.clone());
        Ok(s)
    }

    /// An existing session; new entries continue from its last one. Lines
    /// that can't be read (a crash while saving) are left out and listed in
    /// [`Self::skipped`]; a last line cut short gets its newline first, so
    /// the next entry starts on a line of its own.
    pub fn open(path: &Path) -> Result<Self, SessionError> {
        let (entries, skipped) = read_entries_lossy(BufReader::new(File::open(path)?))?;
        let mut file = OpenOptions::new().append(true).open(path)?;
        let ends_cut = std::fs::read(path)?.last().is_some_and(|b| *b != b'\n');
        if ends_cut {
            file.write_all(b"\n")?;
            file.flush()?;
        }
        let leaf = entries.last().map(|e| e.id.clone());
        Ok(Self {
            path: path.to_owned(),
            file,
            ids: IdGen::random_after(leaf.clone()),
            entries,
            leaf,
            skipped,
        })
    }

    /// Lines that couldn't be read when the session was opened.
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// The session files in one directory (`<root>/<cwd-slug>`), oldest first.
    pub fn list(dir: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "jsonl")
                    && !p.to_string_lossy().ends_with(".wire.jsonl")
            })
            .collect();
        files.sort();
        files
    }

    /// The most recent session file for `cwd`, if any.
    pub fn latest(root: &Path, cwd: &Path) -> Option<PathBuf> {
        Self::list(&root.join(cwd_slug(cwd))).pop()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `<session>.wire.jsonl`.
    pub fn wire_path(&self) -> PathBuf {
        let s = self.path.to_string_lossy();
        PathBuf::from(format!("{}.wire.jsonl", s.trim_end_matches(".jsonl")))
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Appends an entry under the current leaf and writes it out at once.
    pub fn append(&mut self, body: EntryBody) -> Result<&Entry, SessionError> {
        let mut e = self.ids.push(now_ms(), body);
        e.parent_id = self.leaf.clone();
        let mut line = serde_json::to_vec(&e).map_err(std::io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.flush()?;
        self.leaf = Some(e.id.clone());
        self.entries.push(e);
        Ok(self
            .entries
            .last()
            .unwrap_or_else(|| unreachable!("just pushed")))
    }

    /// The entry new ones attach to.
    pub fn leaf(&self) -> Option<&str> {
        self.leaf.as_deref()
    }

    /// Moves the leaf to an existing entry (/rewind, /tree): the next entry
    /// starts a new branch there. Persisted only once an entry is appended.
    pub fn set_leaf(&mut self, id: &str) -> Result<(), SessionError> {
        if !self.entries.iter().any(|e| e.id == id) {
            return Err(SessionError::NoEntry(id.to_owned()));
        }
        self.leaf = Some(id.to_owned());
        Ok(())
    }

    /// The current branch, from the root to the leaf.
    pub fn branch(&self) -> Vec<&Entry> {
        branch(&self.entries, self.leaf.as_deref())
    }
}

/// The path from the root to `leaf`, following `parent_id`.
pub fn branch<'a>(entries: &'a [Entry], leaf: Option<&str>) -> Vec<&'a Entry> {
    let mut out = Vec::new();
    let mut cur = leaf;
    while let Some(id) = cur {
        let Some(e) = entries.iter().rev().find(|e| e.id == id) else {
            break;
        };
        out.push(e);
        cur = e.parent_id.as_deref();
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(c: &str) -> EntryBody {
        EntryBody::checkpoint(c)
    }

    #[test]
    fn create_append_reopen_continue() {
        let root = tempfile::tempdir().unwrap();
        let cwd = Path::new("/home/u/proj");
        let mut s = SessionFile::create(root.path(), cwd).unwrap();
        assert!(s.path().starts_with(root.path().join("-home-u-proj-")));
        assert!(s.wire_path().to_string_lossy().ends_with(".wire.jsonl"));
        let a = s.append(cp("a")).unwrap().id.clone();
        s.append(cp("b")).unwrap();
        assert_eq!(
            SessionFile::latest(root.path(), cwd).as_deref(),
            Some(s.path())
        );

        let mut again = SessionFile::open(s.path()).unwrap();
        assert_eq!(again.entries().len(), 2);
        let c = again.append(cp("c")).unwrap().clone();
        assert_eq!(c.parent_id.as_deref(), Some(again.entries()[1].id.as_str()));
        let ids: Vec<&str> = again.branch().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], a);
        assert_eq!(ids[0].len(), 8);
    }

    #[test]
    fn a_line_cut_by_a_crash_is_skipped_and_the_next_one_is_whole() {
        let root = tempfile::tempdir().unwrap();
        let mut s = SessionFile::create(root.path(), Path::new("/p")).unwrap();
        s.append(cp("a")).unwrap();
        s.append(cp("b")).unwrap();
        let path = s.path().to_owned();
        drop(s);
        // A crash mid-append, and an entry type this version doesn't know.
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"type\":\"from_the_future\",\"id\":\"x\"}\n{\"type\":\"checkp")
            .unwrap();
        drop(f);
        let mut again = SessionFile::open(&path).unwrap();
        assert_eq!(again.entries().len(), 2);
        assert_eq!(again.skipped().len(), 2, "{:?}", again.skipped());
        assert!(
            again.skipped()[1].starts_with("line 4:"),
            "{:?}",
            again.skipped()
        );
        let c = again.append(cp("c")).unwrap().clone();
        assert_eq!(c.parent_id.as_deref(), Some(again.entries()[1].id.as_str()));
        drop(again);
        let third = SessionFile::open(&path).unwrap();
        assert_eq!(third.entries().len(), 3);
        assert_eq!(third.branch().len(), 3);
        assert_eq!(third.skipped().len(), 2);
    }

    #[test]
    fn fork_copies_a_branch_into_a_new_file() {
        let root = tempfile::tempdir().unwrap();
        let cwd = Path::new("/home/u/proj");
        let mut s = SessionFile::create(root.path(), cwd).unwrap();
        s.append(cp("a")).unwrap();
        s.append(cp("b")).unwrap();
        let history: Vec<Entry> = s.branch().into_iter().cloned().collect();
        let mut f = SessionFile::fork(root.path(), cwd, &history).unwrap();
        assert_ne!(f.path(), s.path());
        let c = f.append(cp("c")).unwrap().clone();
        assert_eq!(c.parent_id.as_deref(), Some(history[1].id.as_str()));
        let back = SessionFile::open(f.path()).unwrap();
        assert_eq!(back.entries().len(), 3);
        assert_eq!(back.branch().len(), 3);
        // The original is untouched.
        assert_eq!(SessionFile::open(s.path()).unwrap().entries().len(), 2);
    }

    #[test]
    fn branch_follows_parents_not_file_order() {
        let mut ids = IdGen::new();
        let a = ids.push(0, cp("a"));
        let b = ids.push(0, cp("b"));
        let mut c = ids.push(0, cp("c"));
        c.parent_id = Some(a.id.clone()); // a fork from a
        let all = vec![a.clone(), b, c.clone()];
        let got: Vec<&str> = branch(&all, Some(&c.id))
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(got, [a.id.as_str(), c.id.as_str()]);
    }
}
