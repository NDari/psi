//! Layered JSONC settings (psi-plan.md §3.4, D-04).
//!
//! Layers, lowest first: built-in, user, project, command-line flags. A later
//! layer overrides scalars and arrays, and merges objects key by key. Rule
//! lists (`always_ask`, `always_ask_remove`) append instead (§8.3).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jsonc_parser::ParseOptions;
use serde_json::Value;

/// The built-in layer, embedded in the binary.
pub const BUILTIN_SETTINGS: &str = include_str!("../settings.default.jsonc");

/// Where the file layers live. A missing file is skipped.
#[derive(Debug, Clone, Default)]
pub struct SettingsPaths {
    pub user: Option<PathBuf>,
    pub project: Option<PathBuf>,
}

impl SettingsPaths {
    /// `~/.psi/settings.jsonc` and `<project>/.psi/settings.jsonc`.
    pub fn discover(home: Option<&Path>, project: &Path) -> Self {
        Self {
            user: home.map(|h| h.join(".psi").join("settings.jsonc")),
            // The caller drops this layer until the project is trusted (§9.3).
            project: Some(project.join(".psi").join("settings.jsonc")),
        }
    }
}

/// Reads `~/.psi/models.jsonc`; a missing file means no profiles.
pub fn load_models(home: Option<&Path>) -> Result<psi_ai::profile::ModelsFile> {
    let Some(path) = home.map(|h| h.join(".psi/models.jsonc")) else {
        return Ok(Default::default());
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let origin = path.display().to_string();
    serde_json::from_value(parse_jsonc(&text, &origin)?).with_context(|| origin.clone())
}

/// Parses JSONC: JSON plus comments and trailing commas, nothing looser.
pub fn parse_jsonc(text: &str, origin: &str) -> Result<Value> {
    let options = ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    };
    jsonc_parser::parse_to_serde_value(text, &options).map_err(|e| anyhow::anyhow!("{origin}: {e}"))
}

/// Top-level lists that every layer appends to.
const APPEND_KEYS: &[&str] = &["always_ask", "always_ask_remove"];

/// Merges `over` into `base`: objects merge by key, rule lists append,
/// everything else replaces.
pub fn merge(base: &mut Value, over: Value) {
    let (Value::Object(b), Value::Object(o)) = (&mut *base, &over) else {
        *base = over;
        return;
    };
    for (k, v) in o.clone() {
        match (b.get_mut(&k), v) {
            (Some(Value::Array(list)), Value::Array(more)) if APPEND_KEYS.contains(&k.as_str()) => {
                list.extend(more);
            }
            (Some(slot), v) => merge_value(slot, v),
            (None, v) => {
                b.insert(k, v);
            }
        }
    }
}

fn merge_value(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge_value(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, v) => *slot = v,
    }
}

/// Loads all layers and returns the merged settings object.
pub fn load(paths: &SettingsPaths, flags: Value) -> Result<Value> {
    let mut settings = parse_jsonc(BUILTIN_SETTINGS, "built-in settings")?;
    for path in [&paths.user, &paths.project].into_iter().flatten() {
        if let Some(layer) = read_layer(path)? {
            tracing::debug!(path = %path.display(), "loaded settings layer");
            merge(&mut settings, layer);
        }
    }
    merge(&mut settings, flags);
    Ok(settings)
}

fn read_layer(path: &Path) -> Result<Option<Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let origin = path.display().to_string();
    match parse_jsonc(&text, &origin)? {
        // An empty or comment-only file is an empty layer.
        Value::Null => Ok(None),
        v @ Value::Object(_) => Ok(Some(v)),
        _ => bail!("{origin}: settings must be a JSON object"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_parses_to_object() {
        let v = parse_jsonc(BUILTIN_SETTINGS, "built-in").unwrap();
        assert_eq!(v["mode"], "auto");
        assert_eq!(v["network"], "on");
    }

    #[test]
    fn jsonc_accepts_comments_and_trailing_commas() {
        let v = parse_jsonc("// c\n{ /* c */ \"a\": [1, 2,], }", "t").unwrap();
        assert_eq!(v, json!({"a": [1, 2]}));
    }

    #[test]
    fn jsonc_rejects_looser_syntax() {
        assert!(parse_jsonc("{ a: 1 }", "t").is_err());
        assert!(parse_jsonc("{ \"a\": 'x' }", "t").is_err());
        assert!(parse_jsonc("{ \"a\": 1 \"b\": 2 }", "t").is_err());
    }

    #[test]
    fn parse_error_names_origin() {
        let err = parse_jsonc("{", "/x/settings.jsonc").unwrap_err();
        assert!(err.to_string().starts_with("/x/settings.jsonc:"), "{err}");
    }

    #[test]
    fn merge_overrides_scalars_and_arrays_and_merges_objects() {
        let mut base = json!({"a": 1, "list": [1, 2], "o": {"x": 1, "y": 2}});
        merge(
            &mut base,
            json!({"a": 2, "list": [3], "o": {"y": 3, "z": 4}}),
        );
        assert_eq!(
            base,
            json!({"a": 2, "list": [3], "o": {"x": 1, "y": 3, "z": 4}})
        );
    }

    #[test]
    fn rule_lists_append_across_layers() {
        let mut base = json!({"always_ask": ["a"], "always_ask_remove": [], "cache_dirs": ["x"]});
        merge(
            &mut base,
            json!({"always_ask": ["b"], "always_ask_remove": ["git clean"], "cache_dirs": ["y"]}),
        );
        assert_eq!(
            base,
            json!({"always_ask": ["a", "b"], "always_ask_remove": ["git clean"], "cache_dirs": ["y"]})
        );
    }

    #[test]
    fn merge_replaces_object_with_scalar() {
        let mut base = json!({"o": {"x": 1}});
        merge(&mut base, json!({"o": false}));
        assert_eq!(base, json!({"o": false}));
    }

    #[test]
    fn load_applies_layers_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.jsonc");
        let project = dir.path().join("project.jsonc");
        std::fs::write(&user, "{ \"mode\": \"strict\", \"u\": 1, }").unwrap();
        std::fs::write(
            &project,
            "// only a comment\n{ \"network\": \"off\", \"u\": 2 }",
        )
        .unwrap();
        let paths = SettingsPaths {
            user: Some(user),
            project: Some(project),
        };
        let v = load(&paths, json!({"mode": "auto"})).unwrap();
        assert_eq!(v["mode"], "auto");
        assert_eq!(v["network"], "off");
        assert_eq!(v["u"], 2);
    }

    #[test]
    fn load_skips_missing_and_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.jsonc");
        std::fs::write(&empty, "// nothing\n").unwrap();
        let paths = SettingsPaths {
            user: Some(dir.path().join("missing.jsonc")),
            project: Some(empty),
        };
        let v = load(&paths, json!({})).unwrap();
        assert_eq!(v, parse_jsonc(BUILTIN_SETTINGS, "b").unwrap());
    }

    #[test]
    fn load_rejects_non_object_layer() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.jsonc");
        std::fs::write(&user, "[1]").unwrap();
        let paths = SettingsPaths {
            user: Some(user),
            project: None,
        };
        let err = load(&paths, json!({})).unwrap_err();
        assert!(err.to_string().contains("must be a JSON object"), "{err}");
    }
}
