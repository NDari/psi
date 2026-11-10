//! Layered JSONC settings (psi-plan.md §3.4, D-04).
//!
//! Layers, lowest first: built-in, user, project, command-line flags. A later
//! layer overrides scalars and arrays, and merges objects key by key. Rule
//! lists (`refuse`, `refuse_remove`) append instead.
//!
//! A project's file loads without a trust step, because it can't loosen
//! safety: the keys in [`USER_ONLY`] are dropped from it
//! (design-going-forward.md).

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
    let models: psi_ai::profile::ModelsFile =
        serde_json::from_value(parse_jsonc(&text, &origin)?).with_context(|| origin.clone())?;
    for m in &models.models {
        m.check_sampling()
            .map_err(|e| anyhow::anyhow!("{origin}: {e}"))?;
    }
    Ok(models)
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
const APPEND_KEYS: &[&str] = &["refuse", "refuse_remove"];

/// Keys only the user's own file and the command line may set: the
/// boundary, the network, what bash may write outside the workspace, and
/// dropping a refuse entry.
pub const USER_ONLY: &[&str] = &["isolation", "network", "cache_dirs", "refuse_remove"];

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

/// Loads all layers. Returns the merged settings object, and a note for
/// each key the project's file set that only the user may set.
pub fn load(paths: &SettingsPaths, flags: Value) -> Result<(Value, Vec<String>)> {
    let mut settings = parse_jsonc(BUILTIN_SETTINGS, "built-in settings")?;
    let mut notes = Vec::new();
    if let Some(path) = &paths.user
        && let Some(layer) = read_layer(path)?
    {
        tracing::debug!(path = %path.display(), "loaded settings layer");
        merge(&mut settings, layer);
    }
    if let Some(path) = &paths.project
        && let Some(mut layer) = read_layer(path)?
    {
        tracing::debug!(path = %path.display(), "loaded settings layer");
        if let Value::Object(o) = &mut layer {
            for key in USER_ONLY {
                if o.remove(*key).is_some() {
                    notes.push(format!(
                        "{} sets \"{key}\", which only ~/.psi/settings.jsonc may set; ignored",
                        path.display()
                    ));
                }
            }
        }
        merge(&mut settings, layer);
    }
    merge(&mut settings, flags);
    Ok((settings, notes))
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
        assert_eq!(v["isolation"], "on");
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
        let mut base = json!({"refuse": ["a"], "refuse_remove": [], "cache_dirs": ["x"]});
        merge(
            &mut base,
            json!({"refuse": ["b"], "refuse_remove": ["git push"], "cache_dirs": ["y"]}),
        );
        assert_eq!(
            base,
            json!({"refuse": ["a", "b"], "refuse_remove": ["git push"], "cache_dirs": ["y"]})
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
        std::fs::write(&user, "{ \"notify\": \"off\", \"u\": 1, \"p\": 1 }").unwrap();
        std::fs::write(&project, "// only a comment\n{ \"p\": 2, \"u\": 2 }").unwrap();
        let paths = SettingsPaths {
            user: Some(user),
            project: Some(project),
        };
        let (v, notes) = load(&paths, json!({"notify": "bell"})).unwrap();
        assert_eq!(v["notify"], "bell");
        assert_eq!(v["p"], 2);
        assert_eq!(v["u"], 2);
        assert!(notes.is_empty());
    }

    /// No trust step: a project's file can add to safety, not take away.
    #[test]
    fn a_project_file_cannot_loosen_safety() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.jsonc");
        let project = dir.path().join("project.jsonc");
        std::fs::write(
            &user,
            r#"{ "network": "off", "refuse_remove": ["kubectl"] }"#,
        )
        .unwrap();
        std::fs::write(
            &project,
            r#"{ "isolation": "off", "network": "on", "cache_dirs": ["/"],
                 "refuse_remove": ["git push"], "refuse": ["make deploy"],
                 "post_edit_check": "cargo check -q" }"#,
        )
        .unwrap();
        let paths = SettingsPaths {
            user: Some(user),
            project: Some(project.clone()),
        };
        let (v, notes) = load(&paths, json!({})).unwrap();
        assert_eq!(v["isolation"], "on");
        assert_eq!(v["network"], "off");
        assert_eq!(v["refuse_remove"], json!(["kubectl"]));
        assert_ne!(v["cache_dirs"], json!(["/"]));
        // What tightens or only configures still loads.
        assert_eq!(v["refuse"], json!(["make deploy"]));
        assert_eq!(v["post_edit_check"], "cargo check -q");
        assert_eq!(notes.len(), 4, "{notes:?}");
        assert!(
            notes[0].starts_with(&format!("{} sets \"isolation\"", project.display())),
            "{notes:?}"
        );
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
        let (v, _) = load(&paths, json!({})).unwrap();
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
