use hearthd::Config;

use std::path::PathBuf;

use clap::Parser;
use tracing::{debug, info, warn};
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Load and parse the configuration files
    let (cfg, diagnostics) = match Config::from_files(&cli.config) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    // Set up tracing
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

    // Display any warnings (errors would have prevented loading)
    for diagnostic in &diagnostics.0 {
        if diagnostic.is_warning() {
            warn!("{}", diagnostic);
        }
    }

    // Debug print config at debug level
    debug!("Configuration loaded: {:#?}", cfg);

    info!("hearthd starting");

    // Main daemon logic will go here
    Ok(())
}
