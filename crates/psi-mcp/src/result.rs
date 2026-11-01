//! A tool call's result as the text the model gets (psi-plan.md §10.2).
//! Read from the result's JSON (the MCP wire shape), not from the SDK's
//! types, which change between versions.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The model's text and whether the call failed. Text content is sent as
/// is; `structuredContent` as JSON when there is no text; images, audio and
/// binary resources become a one-line note. Past `max_chars` the text is
/// cut, the whole saved to a file in `spill_dir`, and the result names it.
pub fn render(result: &Value, max_chars: usize, spill_dir: &Path, tool: &str) -> (String, bool) {
    let is_error = result.get("isError").and_then(Value::as_bool) == Some(true);
    let mut parts: Vec<String> = Vec::new();
    let mut texts = 0;
    if let Some(Value::Array(content)) = result.get("content") {
        for c in content {
            let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("");
            match s("type") {
                "text" => {
                    texts += 1;
                    parts.push(s("text").to_owned());
                }
                kind @ ("image" | "audio") => parts.push(format!(
                    "[{kind}: {}, {} bytes, not shown]",
                    s("mimeType"),
                    s("data").len() / 4 * 3
                )),
                "resource" => {
                    let r = c.get("resource").unwrap_or(&Value::Null);
                    let uri = r.get("uri").and_then(Value::as_str).unwrap_or("");
                    match r.get("text").and_then(Value::as_str) {
                        Some(t) => {
                            texts += 1;
                            parts.push(format!("[resource {uri}]\n{t}"));
                        }
                        None => parts.push(format!("[resource {uri}: binary, not shown]")),
                    }
                }
                "resource_link" => {
                    parts.push(format!("[resource link: {} {}]", s("uri"), s("name")))
                }
                other => parts.push(format!("[{other} content, not shown]")),
            }
        }
    }
    if texts == 0
        && let Some(sc) = result.get("structuredContent").filter(|v| !v.is_null())
    {
        parts.push(sc.to_string());
    }
    let text = parts.join("\n");
    let text = if text.trim().is_empty() {
        "(no output)".to_owned()
    } else {
        text
    };
    (cut(text, max_chars, spill_dir, tool), is_error)
}

fn cut(text: String, max: usize, dir: &Path, tool: &str) -> String {
    let Some((end, _)) = text.char_indices().nth(max) else {
        return text;
    };
    let total = text.chars().count();
    let saved = spill(&text, dir, tool);
    let mut out = text[..end].to_owned();
    match saved {
        Some(p) => out.push_str(&format!(
            "\n[cut at {max} of {total} characters; the whole result is in {}: read parts of it or grep it]",
            p.display()
        )),
        None => out.push_str(&format!("\n[cut at {max} of {total} characters]")),
    }
    out
}

fn spill(text: &str, dir: &Path, tool: &str) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let f = tempfile::Builder::new()
        .prefix(&format!("{}-", crate::names::sanitize(tool)))
        .suffix(".txt")
        .tempfile_in(dir)
        .ok()?;
    std::fs::write(f.path(), text).ok()?;
    f.keep().ok().map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn r(v: Value) -> (String, bool) {
        render(&v, 200, Path::new("/nonexistent"), "t")
    }

    #[test]
    fn text_images_and_structured_content() {
        let v = json!({"content": [
            {"type": "text", "text": "found 2"},
            {"type": "image", "mimeType": "image/png", "data": "AAAA"},
            {"type": "resource", "resource": {"uri": "file:///a", "text": "body"}},
            {"type": "resource_link", "uri": "file:///b", "name": "b"}
        ], "structuredContent": {"n": 2}});
        assert_eq!(
            r(v).0,
            "found 2\n[image: image/png, 3 bytes, not shown]\n[resource file:///a]\nbody\n[resource link: file:///b b]"
        );
        assert_eq!(
            r(json!({"content": [], "structuredContent": {"n": 2}})).0,
            r#"{"n":2}"#
        );
        assert_eq!(r(json!({"content": []})), ("(no output)".into(), false));
        assert_eq!(
            r(json!({"content": [{"type": "text", "text": "no such repo"}], "isError": true})),
            ("no such repo".into(), true)
        );
    }

    #[test]
    fn long_results_are_cut_and_saved_whole() {
        let dir = tempfile::tempdir().unwrap();
        let long = "x".repeat(250);
        let (t, _) = render(
            &json!({"content": [{"type": "text", "text": long}]}),
            100,
            dir.path(),
            "mcp__s__get",
        );
        assert!(t.starts_with(&"x".repeat(100)));
        let path = t
            .split(" is in ")
            .nth(1)
            .unwrap()
            .split(':')
            .next()
            .unwrap();
        assert!(t.contains("[cut at 100 of 250 characters;"), "{t}");
        assert_eq!(std::fs::read_to_string(path).unwrap(), long);
        // Nowhere to save: just the cut.
        assert!(
            r(json!({"content": [{"type": "text", "text": long}]}))
                .0
                .ends_with("[cut at 200 of 250 characters]")
        );
    }
}
