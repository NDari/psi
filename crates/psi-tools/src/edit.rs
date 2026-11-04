//! edit: replace exact pieces of a file (D-10): one with `old_text` and
//! `new_text`, or several with `edits`, each matched against the file as
//! it was before the call, all or none.
//!
//! 1. An exact, unique match applies.
//! 2. A unique match that differs only in trailing whitespace or line
//!    endings applies, and the result says so.
//! 3. Line numbers copied from read's output are ignored.
//! 4. Anything else, indentation included, does not apply: the result shows
//!    the closest region and names the difference.

use std::sync::LazyLock;

use psi_ai::{Param, ParamKind, ToolSchema};
use serde_json::{Map, Value, json};

use crate::text::lines;
use crate::{ToolCtx, ToolOutput, str_arg};

/// `edits`: a list of `{old_text, new_text}` (pi has the same, as `edits[]`).
static EDITS_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {"old_text": {"type": "string"}, "new_text": {"type": "string"}},
            "required": ["old_text", "new_text"],
        },
    })
});

pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        name: "edit",
        description: "Replace old_text with new_text in a file, or make several changes with edits [{old_text, new_text}, ...]. Each old_text must match the file as it was before the call, exactly (including indentation) and only once; include a few surrounding lines to make it unique.".into(),
        params: vec![
            Param::new("path", ParamKind::String, "File path, relative to the working directory or absolute").path(),
            // Required unless edits is given; the shape fixes check (fix_call).
            Param::new("old_text", ParamKind::String, "The exact text to replace").optional(),
            Param::new("new_text", ParamKind::String, "The text to put in its place").optional(),
            Param::new("edits", ParamKind::Array, "").optional().with_schema(&EDITS_SCHEMA),
            Param::new("intent", ParamKind::String, "Optional: one line on why").optional(),
        ],
        example: json!({"path": "app.py", "old_text": "    return a - b", "new_text": "    return a + b"}),
    }
}

/// The replacements asked for: `edits`, or `old_text` and `new_text`.
fn pairs(args: &Map<String, Value>) -> Result<Vec<(&str, &str)>, String> {
    let Some(Value::Array(list)) = args.get("edits") else {
        return Ok(vec![(str_arg(args, "old_text"), str_arg(args, "new_text"))]);
    };
    list.iter()
        .enumerate()
        .map(|(i, e)| {
            let field = |k: &str| e.get(k).and_then(Value::as_str);
            match (field("old_text"), field("new_text")) {
                (Some(o), Some(n)) => Ok((o, n)),
                _ => Err(format!(
                    "edits[{i}] needs old_text and new_text, both strings."
                )),
            }
        })
        .collect()
}

pub(crate) fn run(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let shown = str_arg(args, "path");
    let path = ctx.resolve(shown);
    let pairs = match pairs(args) {
        Ok(p) if p.is_empty() => {
            return ToolOutput::err("edits is empty: give at least one {old_text, new_text}.");
        }
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e),
    };
    let many = pairs.len() > 1;
    for (i, (old, new)) in pairs.iter().enumerate() {
        let which = if many {
            format!("edits[{i}]: ")
        } else {
            String::new()
        };
        if old.is_empty() {
            return ToolOutput::err(format!(
                "{which}old_text is empty. To create a file or replace all of it, use write."
            ));
        }
        if old == new {
            return ToolOutput::err(format!(
                "{which}old_text and new_text are the same, so nothing would change."
            ));
        }
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return ToolOutput::err(format!("File not found: {shown}. To create it, use write."));
        }
        Err(e) => return ToolOutput::err(format!("Cannot read {shown}: {e}")),
    };
    let Ok(content) = String::from_utf8(bytes) else {
        return ToolOutput::err(format!("{shown} is not a text file."));
    };
    let (text, applied) = match apply_all(&content, &pairs) {
        Ok(r) => r,
        Err(msg) => return ToolOutput::err(msg.replace("{path}", shown)),
    };
    if let Err(e) = std::fs::write(&path, &text) {
        return ToolOutput::err(format!("Cannot write {shown}: {e}"));
    }
    if let Ok(mut f) = ctx.files.lock()
        && f.was_read(&path)
    {
        f.record(&path, text.as_bytes());
    }
    let mut msg = match applied.as_slice() {
        [a] => format!(
            "Edited {shown}: replaced {} at line {} with {}.",
            lines(a.old_lines),
            a.line,
            lines(a.new_lines)
        ),
        all => {
            let at: Vec<String> = all.iter().map(|a| a.line.to_string()).collect();
            format!(
                "Edited {shown}: {} replacements, at lines {} of the file as it was.",
                all.len(),
                at.join(", ")
            )
        }
    };
    let mut notes: Vec<&str> = applied.iter().filter_map(|a| a.note).collect();
    notes.dedup();
    if !notes.is_empty() {
        msg.push_str(&format!(" ({})", notes.join("; ")));
    }
    ToolOutput::ok(msg)
}

/// Every replacement matched against `content` as it is, then made
/// together: none overlap, and none is made unless all match.
pub(crate) fn apply_all(
    content: &str,
    pairs: &[(&str, &str)],
) -> Result<(String, Vec<Applied>), String> {
    let many = pairs.len() > 1;
    let mut found = Vec::new();
    for (i, (old, new)) in pairs.iter().enumerate() {
        match locate(content, old, new) {
            Ok((mut a, note)) => {
                a.note = note;
                found.push((i, a));
            }
            Err(e) if many => return Err(format!("edits[{i}]: {e} Nothing was changed.")),
            Err(e) => return Err(e),
        }
    }
    let mut order: Vec<usize> = (0..found.len()).collect();
    order.sort_by_key(|&k| found[k].1.start);
    for w in order.windows(2) {
        let (a, b) = (&found[w[0]], &found[w[1]]);
        if b.1.start < a.1.end {
            return Err(format!(
                "edits[{}] and edits[{}] overlap (lines {} and {}). Merge them into one edit. Nothing was changed.",
                a.0, b.0, a.1.line, b.1.line
            ));
        }
    }
    let mut text = content.to_owned();
    for &k in order.iter().rev() {
        let a = &found[k].1;
        text.replace_range(a.start..a.end, &a.replacement);
    }
    Ok((text, found.into_iter().map(|f| f.1).collect()))
}

#[derive(Debug, PartialEq)]
pub(crate) struct Applied {
    /// The bytes of `content` replaced, and what goes in their place.
    pub start: usize,
    pub end: usize,
    pub replacement: String,
    /// 1-based line where the replacement starts.
    pub line: usize,
    pub old_lines: usize,
    pub new_lines: usize,
    /// How it matched, when not exactly.
    pub note: Option<&'static str>,
}

/// One replacement made, for the tests.
#[cfg(test)]
pub(crate) fn apply(
    content: &str,
    old: &str,
    new: &str,
) -> Result<(String, Option<&'static str>), String> {
    let (a, note) = locate(content, old, new)?;
    Ok((
        format!(
            "{}{}{}",
            &content[..a.start],
            a.replacement,
            &content[a.end..]
        ),
        note,
    ))
}

/// The matching rules, on strings. Errors use `{path}` for the file name.
fn locate(content: &str, old: &str, new: &str) -> Result<(Applied, Option<&'static str>), String> {
    if let Some(r) = try_match(content, old, new)? {
        return Ok(r);
    }
    if let Some((old2, new2)) = strip_line_numbers(old, new)
        && let Some((applied, note)) = try_match(content, &old2, &new2)?
    {
        let n = match note {
            Some(_) => {
                "ignored line numbers copied from read, and trailing whitespace or line endings"
            }
            None => "ignored line numbers copied from read",
        };
        return Ok((applied, Some(n)));
    }
    Err(not_found_hint(content, old))
}

/// Tiers 1 and 2. `Ok(None)` means no match at all.
fn try_match(
    content: &str,
    old: &str,
    new: &str,
) -> Result<Option<(Applied, Option<&'static str>)>, String> {
    let exact: Vec<usize> = content.match_indices(old).map(|(i, _)| i).collect();
    if exact.len() == 1 {
        let at = exact[0];
        return Ok(Some((
            Applied {
                start: at,
                end: at + old.len(),
                replacement: new.to_owned(),
                line: line_of(content, at),
                old_lines: old.lines().count().max(1),
                new_lines: new.lines().count(),
                note: None,
            },
            None,
        )));
    }
    if exact.len() > 1 {
        let at: Vec<String> = exact
            .iter()
            .take(5)
            .map(|&i| line_of(content, i).to_string())
            .collect();
        return Err(format!(
            "old_text matches {} places in {{path}} (lines {}). Include more surrounding lines so it matches only once.",
            exact.len(),
            at.join(", ")
        ));
    }
    // Tier 2: whole lines, ignoring trailing whitespace and \r.
    let file_lines: Vec<&str> = content.split('\n').collect();
    let norm = |l: &str| l.trim_end().to_owned();
    let old_norm: Vec<String> = old.trim_end_matches('\n').split('\n').map(norm).collect();
    let file_norm: Vec<String> = file_lines.iter().map(|l| norm(l)).collect();
    if old_norm.iter().all(|l| l.is_empty()) || old_norm.len() > file_norm.len() {
        return Ok(None);
    }
    let hits: Vec<usize> = (0..=file_norm.len() - old_norm.len())
        .filter(|&i| file_norm[i..i + old_norm.len()] == old_norm[..])
        .collect();
    match hits.len() {
        0 => Ok(None),
        1 => {
            let i = hits[0];
            let crlf = file_lines[i].ends_with('\r');
            let mut replacement = new.trim_end_matches('\n').to_owned();
            if crlf {
                replacement = replacement.replace('\n', "\r\n") + "\r";
            }
            // The bytes of lines i to i + len - 1, without the last one's '\n'.
            let start: usize = file_lines[..i].iter().map(|l| l.len() + 1).sum();
            let end = start
                + file_lines[i..i + old_norm.len()]
                    .iter()
                    .map(|l| l.len() + 1)
                    .sum::<usize>()
                - 1;
            Ok(Some((
                Applied {
                    start,
                    end,
                    replacement,
                    line: i + 1,
                    old_lines: old_norm.len(),
                    new_lines: new.lines().count(),
                    note: None,
                },
                Some("matched after ignoring trailing whitespace and line endings"),
            )))
        }
        n => Err(format!(
            "old_text matches {n} places in {{path}} once trailing whitespace is ignored. Include more surrounding lines so it matches only once."
        )),
    }
}

fn line_of(content: &str, byte: usize) -> usize {
    content[..byte].matches('\n').count() + 1
}

/// `12\tfoo` lines, as read printed them until 2026-11-03 (models still copy
/// them from older context or habit). Both texts must carry them (or new
/// may be plain) for this to count as a copy from read.
fn strip_line_numbers(old: &str, new: &str) -> Option<(String, String)> {
    fn strip(s: &str) -> Option<String> {
        let mut out = Vec::new();
        for l in s.split('\n') {
            if l.is_empty() {
                out.push(String::new());
                continue;
            }
            let (num, rest) = l.split_once('\t')?;
            if num.trim().is_empty() || !num.trim().chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            out.push(rest.to_owned());
        }
        Some(out.join("\n"))
    }
    let o = strip(old)?;
    let n = strip(new).unwrap_or_else(|| new.to_owned());
    Some((o, n))
}

/// The closest region of the file and what differs, or advice to re-read.
fn not_found_hint(content: &str, old: &str) -> String {
    let file: Vec<&str> = content.lines().collect();
    let want: Vec<&str> = old.trim_end_matches('\n').lines().collect();
    let base = "old_text was not found in {path}.";
    if file.is_empty() || want.is_empty() {
        return format!("{base} Read the file to see its current content.");
    }
    let k = want.len().min(file.len());
    let want_joined = want.join("\n");
    let mut best = (0.0, 0);
    for i in 0..=file.len() - k {
        let window = file[i..i + k].join("\n");
        let s = strsim::normalized_levenshtein(&window, &want_joined);
        if s > best.0 {
            best = (s, i);
        }
    }
    let (score, at) = best;
    if score < 0.5 {
        return format!(
            "{base} Nothing in the file is close to it. Read the file to see its current content."
        );
    }
    let region = &file[at..at + k];
    let kind = difference(region, &want);
    let shown: Vec<String> = region
        .iter()
        .take(30)
        .enumerate()
        .map(|(j, l)| format!("{}\t{l}", at + j + 1))
        .collect();
    format!(
        "{base} The closest match is lines {}-{}, which differ {kind}:\n{}\nCopy old_text exactly as it appears in the file.",
        at + 1,
        at + k,
        shown.join("\n")
    )
}

fn difference(region: &[&str], want: &[&str]) -> &'static str {
    let same_by = |f: &dyn Fn(&str) -> String| {
        region.len() == want.len() && region.iter().zip(want).all(|(a, b)| f(a) == f(b))
    };
    if same_by(&|s: &str| s.trim_start().to_owned()) {
        return "in indentation";
    }
    let plain = |s: &str| -> String {
        s.chars()
            .map(|c| match c {
                '\u{2018}' | '\u{2019}' => '\'',
                '\u{201C}' | '\u{201D}' => '"',
                '\u{2013}' | '\u{2014}' => '-',
                '\u{00A0}' => ' ',
                c => c,
            })
            .collect()
    };
    if same_by(&|s: &str| plain(s)) {
        return "in quote or dash characters (curly quotes or long dashes instead of plain ones)";
    }
    if same_by(&|s: &str| plain(s).split_whitespace().collect::<Vec<_>>().join(" ")) {
        return "in whitespace";
    }
    "in their text"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(content: &str, old: &str, new: &str) -> (String, Option<&'static str>) {
        apply(content, old, new).unwrap()
    }

    #[test]
    fn exact_unique_match_applies() {
        assert_eq!(ok("a\nb\nc\n", "b\n", "B\n"), ("a\nB\nc\n".into(), None));
    }

    #[test]
    fn several_matches_are_refused_with_lines() {
        let e = apply("x\ny\nx\n", "x", "z").unwrap_err();
        assert_eq!(
            e,
            "old_text matches 2 places in {path} (lines 1, 3). Include more surrounding lines so it matches only once."
        );
    }

    #[test]
    fn trailing_whitespace_and_crlf_apply_and_say_so() {
        let (t, n) = ok(
            "def f():  \n    return 1\n",
            "def f():\n    return 1\n",
            "def f():\n    return 2\n",
        );
        assert_eq!(t, "def f():\n    return 2\n");
        assert!(n.unwrap().contains("trailing whitespace"));
        let (t, _) = ok("a\r\nb\r\nc\r\n", "a\nb\n", "A\nB\n");
        assert_eq!(t, "A\r\nB\r\nc\r\n");
    }

    #[test]
    fn indentation_differences_do_not_apply() {
        let content = "def f():\n    if x:\n        return 1\n";
        let e = apply(content, "if x:\n    return 1", "if x:\n    return 2").unwrap_err();
        assert!(
            e.contains("closest match is lines 2-3, which differ in indentation"),
            "{e}"
        );
        assert!(e.contains("2\t    if x:\n3\t        return 1"), "{e}");
        assert_eq!(std::str::from_utf8(content.as_bytes()).unwrap(), content);
    }

    #[test]
    fn python_and_yaml_indentation_cases_never_apply() {
        // Indentation is meaning in Python and YAML (D-10).
        let py = "class A:\n    def f(self):\n        pass\n";
        assert!(apply(py, "def f(self):\n    pass", "def f(self):\n    return 1").is_err());
        let yaml = "a:\n  b: 1\n  c:\n    d: 2\n";
        assert!(apply(yaml, "c:\n  d: 2", "c:\n  d: 3").is_err());
    }

    #[test]
    fn curly_quotes_are_named() {
        let e = apply(
            "print(\"hi\")\n",
            "print(\u{201C}hi\u{201D})",
            "print(\"bye\")",
        )
        .unwrap_err();
        assert!(e.contains("quote or dash characters"), "{e}");
    }

    #[test]
    fn line_numbers_copied_from_read_are_ignored() {
        let (t, n) = ok("a\nb\nc\n", "2\tb\n", "2\tB\n");
        assert_eq!(t, "a\nB\nc\n");
        assert_eq!(n, Some("ignored line numbers copied from read"));
    }

    #[test]
    fn far_off_text_says_to_reread() {
        let e = apply("alpha\nbeta\n", "something else entirely here", "x").unwrap_err();
        assert!(e.contains("Nothing in the file is close"), "{e}");
    }

    #[test]
    fn run_reports_lines_and_keeps_read_state() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 1000);
        std::fs::write(d.path().join("m.py"), "a = 1\nb = 2\n").unwrap();
        let args = json!({"path": "m.py", "old_text": "b = 2", "new_text": "b = 3\nc = 4"});
        let out = run(args.as_object().unwrap(), &c);
        assert_eq!(
            out,
            ToolOutput::ok("Edited m.py: replaced 1 line at line 2 with 2 lines.")
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join("m.py")).unwrap(),
            "a = 1\nb = 3\nc = 4\n"
        );
        let same = json!({"path": "m.py", "old_text": "x", "new_text": "x"});
        assert!(
            run(same.as_object().unwrap(), &c)
                .text
                .contains("nothing would change")
        );
        let missing = json!({"path": "n.py", "old_text": "x", "new_text": "y"});
        assert!(
            run(missing.as_object().unwrap(), &c)
                .text
                .contains("To create it, use write")
        );
    }

    #[test]
    fn several_edits_match_the_file_as_it_was() {
        let content = "a = 1\nb = 2\nc = 3\n";
        // The second's old_text is the first's new_text: matched against
        // the original, it is the original "b = 2".
        let (t, applied) = apply_all(
            content,
            &[("a = 1", "b = 2"), ("b = 2", "B = 2"), ("c = 3", "c = 4")],
        )
        .unwrap();
        assert_eq!(t, "b = 2\nB = 2\nc = 4\n");
        assert_eq!(
            applied.iter().map(|a| a.line).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        // Overlapping edits are refused; a failing one changes nothing.
        let e = apply_all(content, &[("a = 1\nb", "x"), ("b = 2", "y")]).unwrap_err();
        assert!(
            e.starts_with("edits[0] and edits[1] overlap (lines 1 and 2)"),
            "{e}"
        );
        let e = apply_all(content, &[("a = 1", "x"), ("zzz qqq", "y")]).unwrap_err();
        assert!(e.starts_with("edits[1]: old_text was not found"), "{e}");
        assert!(e.ends_with("Nothing was changed."), "{e}");
        // Trailing whitespace still matches, per edit.
        let (t, applied) = apply_all(
            "x = 1  \ny = 2\n",
            &[("x = 1\n", "x = 9\n"), ("y = 2", "y = 8")],
        )
        .unwrap();
        assert_eq!(t, "x = 9\ny = 8\n");
        assert!(applied[0].note.is_some() && applied[1].note.is_none());
    }

    #[test]
    fn run_takes_several_edits_all_or_none() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::new(d.path(), d.path(), 1000);
        let f = d.path().join("m.py");
        std::fs::write(&f, "a = 1\nb = 2\nc = 3\n").unwrap();
        let args = json!({"path": "m.py", "edits": [
            {"old_text": "a = 1", "new_text": "a = 10"},
            {"old_text": "c = 3", "new_text": "c = 30"},
        ]});
        let out = run(args.as_object().unwrap(), &c);
        assert_eq!(
            out,
            ToolOutput::ok("Edited m.py: 2 replacements, at lines 1, 3 of the file as it was.")
        );
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "a = 10\nb = 2\nc = 30\n"
        );
        let bad = json!({"path": "m.py", "edits": [
            {"old_text": "b = 2", "new_text": "b = 20"},
            {"old_text": "nope nope", "new_text": "x"},
        ]});
        let out = run(bad.as_object().unwrap(), &c);
        assert!(
            out.is_error
                && out
                    .text
                    .starts_with("edits[1]: old_text was not found in m.py"),
            "{}",
            out.text
        );
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "a = 10\nb = 2\nc = 30\n"
        );
        let same = json!({"path": "m.py", "edits": [
            {"old_text": "b = 2", "new_text": "b = 20"},
            {"old_text": "c = 30", "new_text": "c = 30"},
        ]});
        assert!(
            run(same.as_object().unwrap(), &c)
                .text
                .starts_with("edits[1]: old_text and new_text are the same")
        );
        let half = json!({"path": "m.py", "edits": [{"old_text": "b = 2"}]});
        assert_eq!(
            run(half.as_object().unwrap(), &c).text,
            "edits[0] needs old_text and new_text, both strings."
        );
    }
}
