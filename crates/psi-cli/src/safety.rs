//! Turns the settings into the session's safety (design-going-forward.md,
//! goal 1): the refuse list, checkpoints and the bash boundary.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use psi_agent::Safety;
use psi_safety::Sandbox;
use psi_safety::checkpoint::Checkpoints;
use psi_safety::git::{git_available, git_dirs_outside, project_root};
use psi_safety::isolation::{Isolation, SandboxConfig, choose, default_cache_dirs};
use psi_safety::policy::Policy;
use psi_safety::refuse::Rules;
use serde_json::{Value, json};

pub struct SetupPaths<'a> {
    pub cwd: &'a Path,
    pub home: Option<&'a Path>,
    pub tmpdir: &'a Path,
    /// The session's id, for its checkpoint index and ref.
    pub session: &'a str,
}

pub struct SafetySetup {
    pub safety: Safety,
    /// What bash runs inside, when isolated.
    pub sandbox: Option<Arc<Sandbox>>,
    /// Said once at startup: what is off and why (never a silent fallback).
    pub warnings: Vec<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn expand_home(p: &str, home: Option<&Path>) -> PathBuf {
    match (p.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ => PathBuf::from(p),
    }
}

pub fn setup(settings: &Value, paths: &SetupPaths) -> Result<SafetySetup> {
    let mut warnings = Vec::new();
    let rules = Rules::layered(
        &strings(&settings["refuse"]),
        &strings(&settings["refuse_remove"]),
    )?;

    let root = project_root(paths.cwd);
    let checkpoints = if !git_available() {
        warnings.push(
            "git is not installed, so checkpoints are off and changes can't be undone".into(),
        );
        None
    } else {
        let store = paths.home.map(|h| h.join(".psi/checkpoints"));
        match store.map(|s| Checkpoints::open(&s, &root, paths.session)) {
            Some(Ok(c)) => Some(Arc::new(c)),
            Some(Err(e)) => {
                warnings.push(format!("checkpoints are off: {e}"));
                None
            }
            None => {
                warnings.push("no home directory, so checkpoints are off".into());
                None
            }
        }
    };

    let network = settings["network"].as_str() != Some("off");
    let isolate = settings["isolation"].as_str() != Some("off");
    let cache_dirs = match settings.get("cache_dirs") {
        Some(v) => strings(v)
            .iter()
            .map(|p| expand_home(p, paths.home))
            .collect(),
        None => paths.home.map(default_cache_dirs).unwrap_or_default(),
    };
    // Credentials, and the runtime directory where the agents holding them
    // listen: out of bash's reach inside the boundary.
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let hidden = paths
        .home
        .map(|h| psi_safety::secrets::hidden_paths(h, runtime.as_deref()))
        .unwrap_or_default();
    let cfg = SandboxConfig {
        root: root.clone(),
        git_dirs: git_dirs_outside(&root),
        cache_dirs,
        tmpdir: paths.tmpdir.to_owned(),
        network,
        hidden,
    };
    let isolation = choose(isolate, cfg, paths.home);
    let mut policy = Policy::new(rules, &root, paths.home, paths.tmpdir);
    match &isolation {
        Isolation::Unavailable(why) => {
            warnings.push(format!(
                "bash can't be isolated ({why}), so bash calls are refused"
            ));
            policy.unisolated = Some(why.clone());
        }
        Isolation::Off if !network => {
            warnings.push(
                "network off needs isolation; with isolation off, bash keeps the network".into(),
            );
        }
        _ => {}
    }
    let isolation_name = match &isolation {
        Isolation::On(sb) => sb.backend().name(),
        Isolation::Off => "off",
        Isolation::Unavailable(_) => "unavailable",
    };
    let info = json!({
        "isolation": isolation_name,
        "network": if network { "on" } else { "off" },
        "checkpoints": checkpoints.is_some(),
        "root": root,
    });
    Ok(SafetySetup {
        sandbox: isolation.sandbox().cloned(),
        safety: Safety {
            policy,
            checkpoints,
            info,
        },
        warnings,
    })
}
