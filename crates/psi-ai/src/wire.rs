//! The wire log (psi-plan.md §4.6): every request in the exact bytes sent and
//! every response in the bytes received, one JSON object per line.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireEntry {
    /// Pairs a request with its response.
    pub seq: u64,
    pub ts: u64,
    pub kind: WireKind,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// The body as text (lossy only if the bytes were not UTF-8).
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireKind {
    Request,
    Response,
}

#[derive(Debug)]
pub struct WireLog {
    path: PathBuf,
    file: Mutex<File>,
    seq: Mutex<u64>,
}

impl WireLog {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let seq = read_wire(path)
            .map(|e| e.last().map_or(0, |l| l.seq))
            .unwrap_or(0);
        Ok(Self {
            path: path.to_owned(),
            file: Mutex::new(file),
            seq: Mutex::new(seq),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn next_seq(&self) -> u64 {
        let mut s = self.seq.lock().unwrap_or_else(|p| p.into_inner());
        *s += 1;
        *s
    }

    pub(crate) fn write(
        &self,
        seq: u64,
        kind: WireKind,
        url: &str,
        status: Option<u16>,
        body: &[u8],
    ) {
        let entry = WireEntry {
            seq,
            ts: now_ms(),
            kind,
            url: url.to_owned(),
            status,
            body: String::from_utf8_lossy(body).into_owned(),
        };
        let Ok(mut line) = serde_json::to_vec(&entry) else {
            return;
        };
        line.push(b'\n');
        let mut f = self.file.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(e) = f.write_all(&line).and_then(|()| f.flush()) {
            tracing::warn!(path = %self.path.display(), "wire log write failed: {e}");
        }
    }
}

/// Reads a wire log.
pub fn read_wire(path: &Path) -> std::io::Result<Vec<WireEntry>> {
    let f = BufReader::new(File::open(path)?);
    let mut out = Vec::new();
    for line in f.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(e) => out.push(e),
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        }
    }
    Ok(out)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
