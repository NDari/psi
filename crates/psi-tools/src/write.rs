//! write: create a file, or overwrite one read this session and unchanged
//! since (D-24). There is no override flag.

use psi_ai::{Param, ParamKind, ToolSchema};
use serde_json::{Map, Value, json};

use crate::files::Seen;
use crate::text::lines;
use crate::{ToolCtx, ToolOutput, str_arg};

pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        name: "write",
        description: "Create a file, or replace a whole file. Read an existing file before replacing it. For small changes use edit.".into(),
        params: vec![
            Param::new("path", ParamKind::String, "File path, relative to the working directory or absolute").path(),
            Param::new("content", ParamKind::String, "The complete new content"),
            Param::new("intent", ParamKind::String, "Optional: one line on why").optional(),
        ],
        example: json!({"path": "notes/todo.md", "content": "- first item\n"}),
    }
}

pub(crate) fn run(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let shown = str_arg(args, "path");
    let content = str_arg(args, "content");
    let path = ctx.resolve(shown);
    // TODO(phase-3): refuse writes to ~/.psi and the workspace's .psi (D-27);
    // ask before writing gitignored paths.
    if path.is_dir() {
        return ToolOutput::err(format!("{shown} is a directory; give a file path."));
    }
    let existed = path.exists();
    if existed {
        let current = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return ToolOutput::err(format!("Cannot read {shown} to check it: {e}")),
        };
        let seen = match ctx.files.lock() {
            Ok(f) => f.check(&path, &current),
            Err(_) => Seen::Never,
        };
        match seen {
            Seen::Current => {}
            Seen::Never => {
                return ToolOutput::err(format!(
                    "{shown} already exists and you have not read it. Read it first, then write; for a small change use edit."
                ));
            }
            Seen::Changed => {
                return ToolOutput::err(format!(
                    "{shown} changed since you last read it. Read it again before replacing it."
                ));
            }
        }
    }
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return ToolOutput::err(format!("Cannot create the directory for {shown}: {e}"));
    }
    if let Err(e) = std::fs::write(&path, content) {
        return ToolOutput::err(format!("Cannot write {shown}: {e}"));
    }
    if let Ok(mut f) = ctx.files.lock() {
        f.record(&path, content.as_bytes());
    }
    let verb = if existed { "Replaced" } else { "Created" };
    ToolOutput::ok(format!(
        "{verb} {shown} ({}).",
        lines(content.lines().count())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read;

    fn call(
        f: fn(&Map<String, Value>, &ToolCtx) -> ToolOutput,
        c: &ToolCtx,
        v: Value,
    ) -> ToolOutput {
        f(v.as_object().unwrap(), c)
    }

    #[test]
    fn creates_with_parents() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 1000);
        let out = call(run, &c, json!({"path": "a/b/c.txt", "content": "x\ny\n"}));
        assert_eq!(out, ToolOutput::ok("Created a/b/c.txt (2 lines)."));
        assert_eq!(
            std::fs::read_to_string(d.path().join("a/b/c.txt")).unwrap(),
            "x\ny\n"
        );
    }

    #[test]
    fn overwrite_needs_a_current_read() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 1000);
        std::fs::write(d.path().join("f"), "old").unwrap();
        let out = call(run, &c, json!({"path": "f", "content": "new"}));
        assert!(
            out.is_error && out.text.contains("you have not read it"),
            "{out:?}"
        );

        call(read::run, &c, json!({"path": "f"}));
        std::fs::write(d.path().join("f"), "changed outside").unwrap();
        let out = call(run, &c, json!({"path": "f", "content": "new"}));
        assert!(
            out.is_error && out.text.contains("changed since you last read it"),
            "{out:?}"
        );

        call(read::run, &c, json!({"path": "f"}));
        assert_eq!(
            call(run, &c, json!({"path": "f", "content": "new"})),
            ToolOutput::ok("Replaced f (1 line).")
        );
        // Its own write counts as seen: writing again is fine.
        assert!(!call(run, &c, json!({"path": "f", "content": "newer"})).is_error);
    }
}
