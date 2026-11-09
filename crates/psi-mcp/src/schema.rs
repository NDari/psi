//! An MCP tool's input schema, cleaned for llama-server (which turns tool
//! schemas into a grammar, and unusual ones break it or the model's calls)
//! and turned into Psi's tool schema (psi-plan.md §10.2).

use psi_ai::{Param, ParamKind, ToolSchema};
use serde_json::{Map, Value, json};

/// Schemas longer than this keep only their top-level properties.
pub const MAX_SCHEMA: usize = 4096;
/// Longer even then: the tool is skipped.
const MAX_REDUCED: usize = 8192;
/// Tool descriptions are cut here; every tool's text goes out with every request.
pub const MAX_DESCRIPTION: usize = 2000;

/// The schema with gaps filled (`type`, `properties`), `const` as a
/// one-value `enum` and boolean schemas as `{}`; reduced to its top-level
/// properties when long. An error when the input isn't an object.
pub fn clean(schema: &Value) -> Result<Value, String> {
    let mut s = match schema {
        Value::Object(_) => schema.clone(),
        Value::Bool(true) | Value::Null => json!({}),
        _ => return Err("its input schema isn't an object".into()),
    };
    if s.get("type").is_some_and(|t| t != "object") {
        return Err("its input isn't an object".into());
    }
    fix(&mut s);
    let o = s
        .as_object_mut()
        .ok_or("its input schema isn't an object")?;
    o.insert("type".into(), "object".into());
    if !o.get("properties").is_some_and(Value::is_object) {
        o.insert("properties".into(), json!({}));
    }
    if s.to_string().len() > MAX_SCHEMA {
        s = reduce(&s);
        if s.to_string().len() > MAX_REDUCED {
            return Err(format!(
                "its input schema is over {MAX_REDUCED} characters even with only its top-level properties"
            ));
        }
    }
    Ok(s)
}

fn fix(v: &mut Value) {
    match v {
        Value::Bool(_) => *v = json!({}),
        Value::Object(o) => {
            if let Some(c) = o.remove("const")
                && !o.contains_key("enum")
            {
                o.insert("enum".into(), json!([c]));
            }
            if !o.contains_key("type") {
                if o.contains_key("properties") {
                    o.insert("type".into(), "object".into());
                } else if o.contains_key("items") {
                    o.insert("type".into(), "array".into());
                }
            }
            for key in ["properties", "$defs", "definitions"] {
                if let Some(Value::Object(m)) = o.get_mut(key) {
                    m.values_mut().for_each(fix);
                }
            }
            for key in ["items", "additionalProperties"] {
                if let Some(x @ (Value::Object(_) | Value::Bool(_))) = o.get_mut(key) {
                    if key == "additionalProperties" && x.is_boolean() {
                        continue;
                    }
                    fix(x);
                }
            }
            for key in ["anyOf", "oneOf", "allOf", "prefixItems"] {
                if let Some(Value::Array(a)) = o.get_mut(key) {
                    a.iter_mut().for_each(fix);
                }
            }
        }
        _ => {}
    }
}

/// Each top-level property as its type, description and enum only.
fn reduce(s: &Value) -> Value {
    let mut props = Map::new();
    if let Some(Value::Object(p)) = s.get("properties") {
        for (k, v) in p {
            let mut r = Map::new();
            for key in ["type", "description", "enum"] {
                if let Some(x) = v.get(key) {
                    r.insert(key.into(), x.clone());
                }
            }
            props.insert(k.clone(), Value::Object(r));
        }
    }
    let mut out = json!({"type": "object", "properties": props});
    if let Some(r) = s.get("required") {
        out["required"] = r.clone();
    }
    out
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

/// Psi's schema for a tool: `name` as the model sees it, the server's
/// description (cut at [`MAX_DESCRIPTION`]) and a parameter per top-level
/// property; the whole cleaned schema is what is sent. Tools are listed
/// once per connection, so their strings are leaked, as extension tools'
/// are.
pub fn tool_schema(name: &str, description: &str, cleaned: &Value) -> ToolSchema {
    let required: Vec<&str> = cleaned
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut example = Map::new();
    let mut params = Vec::new();
    if let Some(Value::Object(props)) = cleaned.get("properties") {
        for (k, v) in props {
            let kind = kind_of(v);
            let is_required = required.contains(&k.as_str());
            if is_required {
                example.insert(k.clone(), sample(v, kind));
            }
            let desc = v.get("description").and_then(Value::as_str).unwrap_or("");
            let schema: &'static Value = Box::leak(Box::new(v.clone()));
            let p = Param::new(leak(k), kind, leak(desc)).with_schema(schema);
            params.push(if is_required { p } else { p.optional() });
        }
    }
    let description = match description.char_indices().nth(MAX_DESCRIPTION) {
        Some((i, _)) => format!("{}...", &description[..i]),
        None => description.to_owned(),
    };
    ToolSchema {
        name: leak(name),
        description,
        params,
        example: Value::Object(example),
        parameters: Some(Box::leak(Box::new(cleaned.clone()))),
    }
}

fn kind_of(v: &Value) -> ParamKind {
    match v.get("type") {
        Some(Value::String(t)) => ParamKind::parse(t).unwrap_or(ParamKind::Any),
        // ["string", "null"]: the one that isn't null.
        Some(Value::Array(ts)) => {
            let real: Vec<&str> = ts
                .iter()
                .filter_map(Value::as_str)
                .filter(|t| *t != "null")
                .collect();
            match real.as_slice() {
                [t] => ParamKind::parse(t).unwrap_or(ParamKind::Any),
                _ => ParamKind::Any,
            }
        }
        _ => ParamKind::Any,
    }
}

fn sample(v: &Value, kind: ParamKind) -> Value {
    if let Some(first) = v
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|e| e.first())
    {
        return first.clone();
    }
    match kind {
        ParamKind::Integer | ParamKind::Number => json!(1),
        ParamKind::Boolean => json!(true),
        ParamKind::Array => json!([]),
        ParamKind::Object => json!({}),
        ParamKind::String | ParamKind::Any => json!("..."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_are_filled_and_const_becomes_enum() {
        let s = clean(&json!({
            "properties": {
                "mode": {"const": "fast"},
                "opts": {"properties": {"deep": true}},
                "ids": {"items": {"type": "integer"}},
                "either": {"anyOf": [{"const": 1}, {"type": "string"}]}
            },
            "required": ["mode"]
        }))
        .unwrap();
        assert_eq!(s["type"], "object");
        assert_eq!(s["properties"]["mode"], json!({"enum": ["fast"]}));
        assert_eq!(s["properties"]["opts"]["type"], "object");
        assert_eq!(s["properties"]["opts"]["properties"]["deep"], json!({}));
        assert_eq!(s["properties"]["ids"]["type"], "array");
        assert_eq!(s["properties"]["either"]["anyOf"][0], json!({"enum": [1]}));
        assert_eq!(
            clean(&json!({})).unwrap(),
            json!({"type": "object", "properties": {}})
        );
        assert!(clean(&json!({"type": "string"})).is_err());
    }

    #[test]
    fn long_schemas_keep_their_top_level() {
        let deep = json!({"type": "object", "description": "d", "properties": {"x": {"type": "string", "description": "y".repeat(5000)}}});
        let s = clean(&json!({"properties": {"a": deep, "b": {"type": "string", "enum": ["p", "q"]}}, "required": ["b"]})).unwrap();
        assert_eq!(
            s["properties"]["a"],
            json!({"type": "object", "description": "d"})
        );
        assert_eq!(s["properties"]["b"]["enum"], json!(["p", "q"]));
        assert_eq!(s["required"], json!(["b"]));
    }

    #[test]
    fn params_follow_the_properties() {
        let s = clean(&json!({
            "properties": {
                "q": {"type": "string", "description": "query"},
                "n": {"type": ["integer", "null"]},
                "v": {"anyOf": [{"type": "string"}, {"type": "integer"}]},
                "kind": {"type": "string", "enum": ["a", "b"]}
            },
            "required": ["q", "kind"]
        }))
        .unwrap();
        let t = tool_schema("mcp__s__find", "Finds.", &s);
        let kinds: Vec<(&str, ParamKind, bool)> = t
            .params
            .iter()
            .map(|p| (p.name, p.kind, p.required))
            .collect();
        assert_eq!(
            kinds,
            [
                ("q", ParamKind::String, true),
                ("n", ParamKind::Integer, false),
                ("v", ParamKind::Any, false),
                ("kind", ParamKind::String, true),
            ]
        );
        assert_eq!(t.example, json!({"q": "...", "kind": "a"}));
        // Sent as the server wrote it.
        let sent = t.to_openai();
        assert_eq!(
            sent["function"]["parameters"]["properties"]["v"],
            s["properties"]["v"]
        );
        assert_eq!(
            sent["function"]["parameters"]["required"],
            json!(["q", "kind"])
        );
    }
}
