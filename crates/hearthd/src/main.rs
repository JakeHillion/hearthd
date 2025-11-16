mod config;

use config::{format_diagnostics, Config};

use std::path::PathBuf;

use clap::Parser;
use tracing::debug;
use tracing::info;
use tracing_subscriber::filter::Targets as TracingTargets;
use tracing_subscriber::prelude::*;

#[derive(Parser)]
#[command(name = "hearthd")]
#[command(about = "Home automation daemon for location-based services", long_about = None)]
struct Cli {
    /// Path to configuration file(s). Can be specified multiple times to merge configs.
    /// Example: --config base.toml --config secrets.toml
    #[arg(
        short,
        long,
        value_name = "FILE",
        default_value = "/etc/hearthd/config.toml"
    )]
    config: Vec<PathBuf>,
}

fn main() -> Result<(), i32> {
    let cli = Cli::parse();

    // Load and parse the configuration files
    let (cfg, diagnostics) = match Config::from_files(&cli.config) {
        Ok((cfg, diagnostics)) => (cfg, diagnostics),
        Err(e) => {
            eprintln!("{}", e);
            return Err(1);
        }
    };

    // Display any warnings (errors would have prevented loading)
    if !diagnostics.is_empty() {
        eprintln!("{}", format_diagnostics(&diagnostics));
    }

    let log_targets = {
        let mut t = TracingTargets::new().with_default(cfg.logging.level);
        for (target, lvl) in &cfg.logging.overrides {
            t = t.with_target(target.clone(), *lvl);
        }
        t
    };
    tracing_subscriber::registry()
        .with(log_targets)
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Debug print config at debug level
    debug!("Configuration loaded: {:#?}", cfg);

    info!("hearthd starting");

    // Main daemon logic will go here
    Ok(())
}
