//! Turns the settings into the session's safety (psi-plan.md §8): the
//! mode, the always-ask rules, checkpoints and the bash boundary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use psi_agent::Safety;
use psi_agent::approval::{Approver, Headless};
use psi_safety::Sandbox;
use psi_safety::always_ask::Rules;
use psi_safety::checkpoint::Checkpoints;
use psi_safety::git::{git_available, git_dirs_outside, project_root};
use psi_safety::isolation::{Isolation, SandboxConfig, choose, default_cache_dirs};
use psi_safety::policy::{Mode, Policy};
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

/// Headless runs: nothing prompts (D-18).
pub fn setup(
    settings: &Value,
    allow: &[String],
    mode_flag: bool,
    paths: &SetupPaths,
) -> Result<SafetySetup> {
    setup_with(settings, allow, mode_flag, paths, Arc::new(Headless))
}

pub fn setup_with(
    settings: &Value,
    allow: &[String],
    mode_flag: bool,
    paths: &SetupPaths,
    approver: Arc<dyn Approver>,
) -> Result<SafetySetup> {
    let mut warnings = Vec::new();
    let mode_s = settings["mode"].as_str().unwrap_or("auto");
    let Some(mut mode) = Mode::parse(mode_s) else {
        bail!("mode is \"auto\" or \"strict\", not \"{mode_s}\"");
    };
    let rules = Rules::layered(
        &strings(&settings["always_ask"]),
        &strings(&settings["always_ask_remove"]),
    )?;
    let mut grants = HashSet::new();
    for a in allow {
        let ids: Vec<&str> = rules.ids().filter(|id| Rules::names(id, a)).collect();
        if ids.is_empty() {
            let all: Vec<&str> = rules.ids().collect();
            bail!(
                "--allow {a}: no always-ask entry by that name (entries: {})",
                all.join(", ")
            );
        }
        grants.extend(ids.into_iter().map(str::to_owned));
    }

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
    if checkpoints.is_none() && mode == Mode::Auto && !mode_flag {
        mode = Mode::Strict;
        warnings.push("the mode is strict: every change asks (--mode auto overrides)".into());
    }

    let network = settings["network"].as_str() != Some("off");
    let isolate = settings["isolation"].as_str() != Some("off");
    let cache_dirs = match settings.get("cache_dirs") {
        Some(v) => strings(v)
            .iter()
            .map(|p| expand_home(p, paths.home))
            .collect(),
        None => paths.home.map(default_cache_dirs).unwrap_or_default(),
    };
    let cfg = SandboxConfig {
        root: root.clone(),
        git_dirs: git_dirs_outside(&root),
        cache_dirs,
        tmpdir: paths.tmpdir.to_owned(),
        network,
    };
    let isolation = choose(isolate, cfg, paths.home);
    let mut policy = Policy::new(mode, rules, &root, paths.home, paths.tmpdir);
    match &isolation {
        Isolation::Unavailable(why) => {
            warnings.push(format!(
                "bash is not isolated ({why}); every bash call asks"
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
        Isolation::Bwrap(_) => "bwrap",
        Isolation::Off => "off",
        Isolation::Unavailable(_) => "unavailable",
    };
    let info = json!({
        "mode": mode.as_str(),
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
            approver,
            grants,
            info,
        },
        warnings,
    })
}
