//! read: file contents as they are, cut relative to the context window. No
//! line numbers, as in pi: with Qwen's one-token digits they cost about a
//! third more tokens for the same file.

use std::path::Path;

use psi_ai::{Param, ParamKind, ToolSchema};
use serde_json::{Map, Value, json};

use crate::text::cut_line;
use crate::{ToolCtx, ToolOutput, int_arg, str_arg};

/// The most lines one read returns.
pub(crate) const MAX_LINES: usize = 2000;

pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        name: "read",
        description: format!(
            "Read a text file. Long files are shown in parts (up to {MAX_LINES} lines); the result says which offset to read next."
        ),
        params: vec![
            Param::new(
                "path",
                ParamKind::String,
                "File path, relative to the working directory or absolute",
            )
            .path(),
            Param::new("offset", ParamKind::Integer, "First line to show, from 1").optional(),
            Param::new("limit", ParamKind::Integer, "How many lines to show").optional(),
        ],
        example: json!({"path": "src/main.py"}),
    }
}

pub(crate) fn run(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let shown = str_arg(args, "path");
    let path = ctx.resolve(shown);
    if path.is_dir() {
        return ToolOutput::err(format!(
            "{shown} is a directory. List it with bash, for example: ls {shown}"
        ));
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return ToolOutput::err(not_found(shown, &path));
        }
        Err(e) => return ToolOutput::err(format!("Cannot read {shown}: {e}")),
    };
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return ToolOutput::err(format!(
            "{shown} is a binary file ({} bytes); read shows text files only.",
            bytes.len()
        ));
    }
    if let Ok(mut f) = ctx.files.lock() {
        f.record(&path, &bytes);
    }
    let text = String::from_utf8_lossy(&bytes);
    if text.is_empty() {
        return ToolOutput::ok(format!("{shown} is empty."));
    }
    let lines: Vec<&str> = text
        .strip_suffix('\n')
        .unwrap_or(&text)
        .split('\n')
        .collect();
    let total = lines.len();
    let offset = int_arg(args, "offset").unwrap_or(1).max(1) as usize;
    if offset > total {
        return ToolOutput::err(format!(
            "offset {offset} is past the end of {shown}, which has {total} lines."
        ));
    }
    let limit = int_arg(args, "limit")
        .map(|l| l.max(1) as usize)
        .unwrap_or(usize::MAX)
        .min(MAX_LINES);
    let mut out = String::new();
    let mut last = offset - 1;
    for (i, line) in lines.iter().enumerate().skip(offset - 1).take(limit) {
        let shown = format!("{}\n", cut_line(line.strip_suffix('\r').unwrap_or(line)));
        if out.len() + shown.len() > ctx.max_result_chars && last >= offset {
            break;
        }
        out.push_str(&shown);
        last = i + 1;
    }
    if last < total {
        out.push_str(&format!(
            "[Showing lines {offset}-{last} of {total}. Call read with offset={} to continue.]",
            last + 1
        ));
    }
    ToolOutput::ok(out.trim_end_matches('\n').to_owned())
}

/// Names the missing piece and offers close names from the same directory.
fn not_found(shown: &str, path: &Path) -> String {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.is_dir() {
            break;
        }
        dir = d.parent();
    }
    let Some(dir) = dir else {
        return format!("File not found: {shown}");
    };
    if Some(dir) != path.parent() {
        return format!(
            "File not found: {shown} (the directory {} does not exist).",
            path.parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        );
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut close: Vec<(f64, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .map(|n| (strsim::jaro_winkler(&n, &name), n))
        .filter(|(s, _)| *s > 0.8)
        .collect();
    close.sort_by(|a, b| b.0.total_cmp(&a.0));
    if close.is_empty() {
        return format!("File not found: {shown}");
    }
    let names: Vec<String> = close.into_iter().take(3).map(|(_, n)| n).collect();
    format!(
        "File not found: {shown}. Similar names in that directory: {}",
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &Path) -> ToolCtx {
        ToolCtx::new(dir, dir, 100_000)
    }

    fn read(c: &ToolCtx, args: Value) -> ToolOutput {
        run(args.as_object().unwrap(), c)
    }

    #[test]
    fn shows_lines_as_they_are_and_records_the_read() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.py"), "x = 1\r\ny = 2\n").unwrap();
        let c = ctx(d.path());
        let out = read(&c, json!({"path": "a.py"}));
        assert_eq!(out, ToolOutput::ok("x = 1\ny = 2"));
        assert!(c.files.lock().unwrap().was_read(&d.path().join("a.py")));
    }

    #[test]
    fn long_files_say_how_to_continue() {
        let d = tempfile::tempdir().unwrap();
        let body: String = (1..=50).map(|i| format!("line {i}\n")).collect();
        std::fs::write(d.path().join("big.txt"), body).unwrap();
        let out = read(
            &ctx(d.path()),
            json!({"path": "big.txt", "offset": 10, "limit": 5}),
        );
        assert_eq!(
            out.text,
            "line 10\nline 11\nline 12\nline 13\nline 14\n[Showing lines 10-14 of 50. Call read with offset=15 to continue.]"
        );
        let small = ToolCtx::new(d.path(), d.path(), 40);
        let out = read(&small, json!({"path": "big.txt"}));
        assert!(
            out.text.ends_with("Call read with offset=6 to continue.]"),
            "{}",
            out.text
        );
    }

    #[test]
    fn errors_say_what_to_do() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("parser.py"), "").unwrap();
        std::fs::write(d.path().join("bin"), [0u8, 1, 2]).unwrap();
        let c = ctx(d.path());
        assert_eq!(
            read(&c, json!({"path": "parse.py"})).text,
            "File not found: parse.py. Similar names in that directory: parser.py"
        );
        assert!(
            read(&c, json!({"path": "nope/x.py"}))
                .text
                .contains("does not exist")
        );
        assert!(
            read(&c, json!({"path": "bin"}))
                .text
                .contains("binary file")
        );
        assert!(
            read(&c, json!({"path": "."}))
                .text
                .contains("is a directory")
        );
        assert_eq!(
            read(&c, json!({"path": "parser.py"})).text,
            "parser.py is empty."
        );
        let e = read(&c, json!({"path": "parser.py", "offset": 3}));
        assert!(!e.is_error, "an empty file is fine");
    }

    #[test]
    fn offset_past_end_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a"), "1\n2\n").unwrap();
        let out = read(&ctx(d.path()), json!({"path": "a", "offset": 5}));
        assert_eq!(
            out,
            ToolOutput::err("offset 5 is past the end of a, which has 2 lines.")
        );
    }
}
