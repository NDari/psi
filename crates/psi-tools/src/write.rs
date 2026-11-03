//! write: create a file, or overwrite one read this session and unchanged
//! since (D-24). There is no override flag.

use psi_ai::{Param, ParamKind, ToolSchema};
use serde_json::{Map, Value, json};

use crate::files::Seen;
use crate::read;
use crate::text::lines;
use crate::{ToolCtx, ToolOutput, str_arg};

pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        name: "write",
        description: "Create a file, or replace a whole file. Read an existing file before replacing it. For small changes use edit.".into(),
        params: vec![
            Param::new("path", ParamKind::String, "File path, relative to the working directory or absolute").path(),
            // Not "content": some chat templates sort keys, and the path
            // must come before the long text or models drop it (H-05).
            Param::new("text", ParamKind::String, "The complete new content of the file"),
            Param::new("intent", ParamKind::String, "Optional: one line on why").optional(),
        ],
        example: json!({"path": "notes/todo.md", "text": "- first item\n"}),
    }
}

pub(crate) fn run(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let shown = str_arg(args, "path");
    let content = str_arg(args, "text");
    let path = ctx.resolve(shown);
    // The policy (psi-safety) refuses .psi and asks for gitignored paths
    // before this runs (D-27).
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
        // The refusal reads the file for the model (it counts as the read),
        // so the next write goes through: asking a small model to read first
        // made Gemma 4 retry the same write 14 times.
        let why = match seen {
            Seen::Current => None,
            Seen::Never => Some("already exists and you have not read it"),
            Seen::Changed => Some("changed since you last read it"),
        };
        if let Some(why) = why {
            let mut read_args = Map::new();
            read_args.insert("path".into(), Value::String(shown.into()));
            let current = read::run(&read_args, ctx);
            return ToolOutput::err(format!(
                "{shown} {why}, so it was not replaced. Its current content is below; call write again to replace it, or use edit for a small change.\n{}",
                current.text
            ));
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

    fn call(
        f: fn(&Map<String, Value>, &ToolCtx) -> ToolOutput,
        c: &ToolCtx,
        v: Value,
    ) -> ToolOutput {
        f(v.as_object().unwrap(), c)
    }

    /// Gemma 4's chat template lists schema keys alphabetically; with
    /// `content` before `path` it wrote whole files and dropped the path,
    /// 20 times in a row (psi eval, sh_logreport, 2026-10-31).
    #[test]
    fn path_comes_before_the_text_in_any_order() {
        let v = schema().to_openai();
        let props: Vec<&String> = v["function"]["parameters"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(props, ["path", "text", "intent"], "declared order");
        let mut sorted = props.clone();
        sorted.sort();
        let pos = |k: &str| sorted.iter().position(|p| *p == k).unwrap();
        assert!(pos("path") < pos("text"), "alphabetical order: {sorted:?}");
    }

    #[test]
    fn creates_with_parents() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 1000);
        let out = call(run, &c, json!({"path": "a/b/c.txt", "text": "x\ny\n"}));
        assert_eq!(out, ToolOutput::ok("Created a/b/c.txt (2 lines)."));
        assert_eq!(
            std::fs::read_to_string(d.path().join("a/b/c.txt")).unwrap(),
            "x\ny\n"
        );
    }

    #[test]
    fn tmpdir_in_a_path_means_the_sessions_tmpdir() {
        // Models write `$TMPDIR/...` in file paths as the prompt says to in
        // bash; that is the session's TMPDIR, not a folder named `$TMPDIR`.
        let d = tempfile::tempdir().unwrap();
        let (cwd, tmp) = (d.path().join("project"), d.path().join("tmp"));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        let c = ToolCtx::new(&cwd, &tmp, 1000);
        call(run, &c, json!({"path": "$TMPDIR/a/x.txt", "text": "1"}));
        call(run, &c, json!({"path": "${TMPDIR}/y.txt", "text": "2"}));
        assert_eq!(std::fs::read_to_string(tmp.join("a/x.txt")).unwrap(), "1");
        assert_eq!(std::fs::read_to_string(tmp.join("y.txt")).unwrap(), "2");
        assert!(!cwd.join("$TMPDIR").exists());
        // Only the variable on its own: `$TMPDIRS/z` is an ordinary name.
        call(run, &c, json!({"path": "$TMPDIRS/z.txt", "text": "3"}));
        assert!(cwd.join("$TMPDIRS/z.txt").exists());
    }

    #[test]
    fn overwrite_needs_a_current_read() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 1000);
        std::fs::write(d.path().join("f"), "old").unwrap();
        let out = call(run, &c, json!({"path": "f", "text": "new"}));
        assert!(
            out.is_error && out.text.contains("you have not read it"),
            "{out:?}"
        );
        assert!(
            out.text.ends_with("\nold"),
            "the refusal shows the content: {out:?}"
        );
        assert_eq!(std::fs::read_to_string(d.path().join("f")).unwrap(), "old");
        // The refusal counts as the read.
        assert_eq!(
            call(run, &c, json!({"path": "f", "text": "new"})),
            ToolOutput::ok("Replaced f (1 line).")
        );

        std::fs::write(d.path().join("f"), "changed outside").unwrap();
        let out = call(run, &c, json!({"path": "f", "text": "newer"}));
        assert!(
            out.is_error && out.text.contains("changed since you last read it"),
            "{out:?}"
        );
        assert!(out.text.ends_with("\nchanged outside"), "{out:?}");
        assert_eq!(
            std::fs::read_to_string(d.path().join("f")).unwrap(),
            "changed outside"
        );
        assert_eq!(
            call(run, &c, json!({"path": "f", "text": "newer"})),
            ToolOutput::ok("Replaced f (1 line).")
        );
        // Its own write counts as seen: writing again is fine.
        assert!(!call(run, &c, json!({"path": "f", "text": "newest"})).is_error);
    }
}
