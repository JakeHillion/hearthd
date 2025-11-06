mod config;

use config::Config;

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
    /// Path to the configuration file
    #[arg(
        short,
        long,
        value_name = "FILE",
        default_value = "/etc/hearthd/config.toml"
    )]
    config: PathBuf,
}

fn main() -> Result<(), i32> {
    let cli = Cli::parse();

    // Check if config file exists
    if !cli.config.exists() {
        eprintln!(
            "Error: Configuration file not found: {}",
            cli.config.display()
        );
        eprintln!("Please create a configuration file or specify a different path with --config");
        return Err(1);
    }

    // Load and parse the configuration file
    let cfg = match Config::from_file(&cli.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "Error: Failed to parse configuration file: {}",
                cli.config.display()
            );
            eprintln!("Reason: {}", e);
            return Err(1);
        }
    };

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
