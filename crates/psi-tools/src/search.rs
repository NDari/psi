//! grep, find and ls: .gitignore-aware helpers, off by default (D-17, H-10).

use std::path::Path;

use globset::Glob;
use ignore::WalkBuilder;
use psi_ai::{Param, ParamKind, ToolSchema};
use regex::RegexBuilder;
use serde_json::{Map, Value, json};

use crate::{ToolCtx, ToolOutput, str_arg};

const MAX_MATCHES: usize = 200;
const MAX_PATHS: usize = 500;
const MAX_MATCH_LINE: usize = 300;

pub(crate) fn grep_schema() -> ToolSchema {
    ToolSchema {
        name: "grep",
        description: format!(
            "Search file contents with a regular expression. Skips files in .gitignore. Shows up to {MAX_MATCHES} matches as path:line: text."
        ),
        params: vec![
            Param::new(
                "pattern",
                ParamKind::String,
                "Regular expression (Rust syntax)",
            ),
            Param::new(
                "path",
                ParamKind::String,
                "Directory or file to search (default: the working directory)",
            )
            .path()
            .optional(),
            Param::new(
                "glob",
                ParamKind::String,
                "Only files matching this glob, for example *.py",
            )
            .optional(),
            Param::new(
                "ignore_case",
                ParamKind::Boolean,
                "Match case-insensitively",
            )
            .optional(),
        ],
        example: json!({"pattern": "def parse_", "glob": "*.py"}),
        parameters: None,
    }
}

pub(crate) fn find_schema() -> ToolSchema {
    ToolSchema {
        name: "find",
        description: format!(
            "Find files whose path matches a glob, such as **/*.py. Skips files in .gitignore. Shows up to {MAX_PATHS} paths."
        ),
        params: vec![
            Param::new(
                "pattern",
                ParamKind::String,
                "Glob matched against paths relative to the search directory",
            ),
            Param::new(
                "path",
                ParamKind::String,
                "Directory to search (default: the working directory)",
            )
            .path()
            .optional(),
        ],
        example: json!({"pattern": "**/test_*.py"}),
        parameters: None,
    }
}

pub(crate) fn ls_schema() -> ToolSchema {
    ToolSchema {
        name: "ls",
        description: "List a directory. Directories end in /.".into(),
        params: vec![
            Param::new(
                "path",
                ParamKind::String,
                "Directory (default: the working directory)",
            )
            .path()
            .optional(),
        ],
        example: json!({"path": "src"}),
        parameters: None,
    }
}

fn base(args: &Map<String, Value>, ctx: &ToolCtx) -> std::path::PathBuf {
    match str_arg(args, "path") {
        "" => ctx.cwd.clone(),
        p => ctx.resolve(p),
    }
}

fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

pub(crate) fn grep(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let pattern = str_arg(args, "pattern");
    let re = match RegexBuilder::new(pattern)
        .case_insensitive(
            args.get("ignore_case")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .build()
    {
        Ok(r) => r,
        Err(e) => return ToolOutput::err(format!("Invalid regular expression: {e}")),
    };
    let glob = match str_arg(args, "glob") {
        "" => None,
        g => match Glob::new(g) {
            Ok(g) => Some(g.compile_matcher()),
            Err(e) => return ToolOutput::err(format!("Invalid glob: {e}")),
        },
    };
    let root = base(args, ctx);
    if !root.exists() {
        return ToolOutput::err(format!("Not found: {}", str_arg(args, "path")));
    }
    let mut out = Vec::new();
    let mut more = 0;
    for entry in WalkBuilder::new(&root).build().flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        if let Some(g) = &glob
            && !g.is_match(p.file_name().unwrap_or_default())
            && !g.is_match(p.strip_prefix(&root).unwrap_or(p))
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(p) else {
            continue;
        };
        if bytes.iter().take(8192).any(|&c| c == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                if out.len() >= MAX_MATCHES {
                    more += 1;
                    continue;
                }
                let line: String = line.chars().take(MAX_MATCH_LINE).collect();
                out.push(format!("{}:{}: {line}", rel(p, &ctx.cwd), i + 1));
            }
        }
    }
    if out.is_empty() {
        return ToolOutput::ok("No matches.");
    }
    let mut text = out.join("\n");
    if more > 0 {
        text.push_str(&format!(
            "\n[{more} more matches not shown; narrow the pattern or path]"
        ));
    }
    ToolOutput::ok(text)
}

pub(crate) fn find(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let g = match Glob::new(str_arg(args, "pattern")) {
        Ok(g) => g.compile_matcher(),
        Err(e) => return ToolOutput::err(format!("Invalid glob: {e}")),
    };
    let root = base(args, ctx);
    if !root.is_dir() {
        return ToolOutput::err(format!("Not a directory: {}", str_arg(args, "path")));
    }
    let mut paths: Vec<String> = WalkBuilder::new(&root)
        .build()
        .flatten()
        .filter(|e| e.depth() > 0)
        .filter(|e| g.is_match(e.path().strip_prefix(&root).unwrap_or(e.path())))
        .map(|e| rel(e.path(), &ctx.cwd))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return ToolOutput::ok("No paths match.");
    }
    let total = paths.len();
    paths.truncate(MAX_PATHS);
    let mut text = paths.join("\n");
    if total > MAX_PATHS {
        text.push_str(&format!("\n[{} more paths not shown]", total - MAX_PATHS));
    }
    ToolOutput::ok(text)
}

pub(crate) fn ls(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let root = base(args, ctx);
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) => return ToolOutput::err(format!("Cannot list {}: {e}", root.display())),
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| {
            let mut n = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                n.push('/');
            }
            n
        })
        .collect();
    names.sort();
    if names.is_empty() {
        return ToolOutput::ok("(empty directory)");
    }
    let total = names.len();
    names.truncate(MAX_PATHS);
    let mut text = names.join("\n");
    if total > MAX_PATHS {
        text.push_str(&format!("\n[{} more entries not shown]", total - MAX_PATHS));
    }
    ToolOutput::ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, ToolCtx) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src/sub")).unwrap();
        std::fs::write(d.path().join("src/a.py"), "def parse_x():\n    pass\n").unwrap();
        std::fs::write(d.path().join("src/sub/b.py"), "x = parse_y()\n").unwrap();
        std::fs::write(d.path().join("notes.md"), "parse_ here too\n").unwrap();
        std::fs::write(d.path().join(".gitignore"), "build/\n").unwrap();
        std::fs::create_dir_all(d.path().join("build")).unwrap();
        std::fs::write(d.path().join("build/gen.py"), "parse_z\n").unwrap();
        // ignore only reads .gitignore inside a git repository.
        std::fs::create_dir_all(d.path().join(".git")).unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 10_000);
        (d, c)
    }

    fn call(f: fn(&Map<String, Value>, &ToolCtx) -> ToolOutput, c: &ToolCtx, v: Value) -> String {
        f(v.as_object().unwrap(), c).text
    }

    #[test]
    fn grep_respects_gitignore_and_glob() {
        let (_d, c) = setup();
        assert_eq!(
            call(grep, &c, json!({"pattern": "parse_", "glob": "*.py"})),
            "src/a.py:1: def parse_x():\nsrc/sub/b.py:1: x = parse_y()"
        );
        assert!(call(grep, &c, json!({"pattern": "("})).starts_with("Invalid regular expression"));
    }

    #[test]
    fn find_and_ls() {
        let (_d, c) = setup();
        assert_eq!(
            call(find, &c, json!({"pattern": "**/*.py"})),
            "src/a.py\nsrc/sub/b.py"
        );
        assert_eq!(call(ls, &c, json!({"path": "src"})), "a.py\nsub/");
    }
}
