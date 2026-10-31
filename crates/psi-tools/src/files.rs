//! Which files were read this session, so write can refuse to overwrite a
//! file the model hasn't seen or that changed since (D-24).

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct FileTracker {
    seen: HashMap<PathBuf, u64>,
}

pub(crate) fn hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

pub(crate) enum Seen {
    Never,
    Changed,
    Current,
}

impl FileTracker {
    pub fn record(&mut self, path: &Path, bytes: &[u8]) {
        self.seen.insert(path.to_owned(), hash(bytes));
    }

    pub fn was_read(&self, path: &Path) -> bool {
        self.seen.contains_key(path)
    }

    pub(crate) fn check(&self, path: &Path, current: &[u8]) -> Seen {
        match self.seen.get(path) {
            None => Seen::Never,
            Some(h) if *h == hash(current) => Seen::Current,
            Some(_) => Seen::Changed,
        }
    }
}
