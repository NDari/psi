use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use psi_cli::config::{self, SettingsPaths};
use serde_json::{Map, Value};
use tracing_subscriber::EnvFilter;

/// A coding agent harness for local models.
#[derive(Debug, Parser)]
#[command(name = "psi", version)]
struct Cli {
    /// Safety mode; overrides the settings files (D-11).
    #[arg(long, value_enum)]
    mode: Option<Mode>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    Auto,
    Strict,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Strict => "strict",
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("psi: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Logs go to stderr, filtered by `PSI_LOG` (for example `PSI_LOG=debug`).
fn init_tracing() {
    let filter = EnvFilter::try_from_env("PSI_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

fn run(cli: Cli) -> Result<()> {
    let cwd = std::env::current_dir().context("reading the working directory")?;
    let paths = SettingsPaths::discover(std::env::home_dir().as_deref(), &cwd);
    let settings = config::load(&paths, flags_layer(&cli))?;
    tracing::debug!(%settings, "settings loaded");
    // TODO(phase-2): print and JSON modes. TODO(phase-4): interactive mode.
    eprintln!("psi: no modes are built yet; see `psi --help`");
    Ok(())
}

fn flags_layer(cli: &Cli) -> Value {
    let mut m = Map::new();
    if let Some(mode) = cli.mode {
        m.insert("mode".into(), mode.as_str().into());
    }
    Value::Object(m)
}
