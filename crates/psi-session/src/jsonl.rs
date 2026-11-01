//! Reading and writing session JSONL files.

use std::io::{BufRead, Write};

use crate::Entry;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("line {line}: {source}")]
    Parse {
        line: usize,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("no entry {0} in this session")]
    NoEntry(String),
}

/// Reads entries, one JSON object per line. Blank lines are skipped.
pub fn read_entries(r: impl BufRead) -> Result<Vec<Entry>, SessionError> {
    let mut out = Vec::new();
    for (i, line) in r.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let e = serde_json::from_str(&line).map_err(|source| SessionError::Parse {
            line: i + 1,
            source,
        })?;
        out.push(e);
    }
    Ok(out)
}

/// Reads the entries that can be read, and says which lines couldn't: a
/// line cut by a crash while saving, or an entry type from a newer psi.
/// One bad line no longer makes a whole session unreadable.
pub fn read_entries_lossy(r: impl BufRead) -> Result<(Vec<Entry>, Vec<String>), SessionError> {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for (i, line) in r.split(b'\n').enumerate() {
        let line = line?;
        let line = String::from_utf8_lossy(&line);
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(e) => out.push(e),
            Err(e) => skipped.push(format!("line {}: {e}", i + 1)),
        }
    }
    Ok((out, skipped))
}

/// Writes entries, one JSON object per line.
pub fn write_entries<'a>(
    mut w: impl Write,
    entries: impl IntoIterator<Item = &'a Entry>,
) -> Result<(), SessionError> {
    for e in entries {
        // Entries are plain data; serializing them can't fail.
        serde_json::to_writer(&mut w, e).map_err(std::io::Error::other)?;
        w.write_all(b"\n")?;
    }
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntryBody, IdGen};

    #[test]
    fn write_then_read() {
        let mut ids = IdGen::new();
        let entries = vec![
            ids.push(1, EntryBody::checkpoint("a")),
            ids.push(2, EntryBody::checkpoint("b")),
        ];
        let mut buf = Vec::new();
        write_entries(&mut buf, &entries).unwrap();
        let back = read_entries(&buf[..]).unwrap();
        assert_eq!(back, entries);
    }

    #[test]
    fn parse_error_names_the_line() {
        let err = read_entries("\n{not json}\n".as_bytes()).unwrap_err();
        assert!(err.to_string().starts_with("line 2:"), "{err}");
    }
}
