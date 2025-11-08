mod config;
mod ha;
mod state;

use config::Config;
use ha::Registry as HaRegistry;
use ha::SandboxBuilder;
use state::State;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing::debug;
use tracing::error;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Check if config file exists
    if !cli.config.exists() {
        eprintln!(
            "Error: Configuration file not found: {}",
            cli.config.display()
        );
        eprintln!("Please create a configuration file or specify a different path with --config");
        return Err("Configuration file not found".into());
    }

    // Load and parse the configuration file
    let config = Arc::new(Config::from_file(&cli.config)?);

    // Initialize tracing/logging with proper log level configuration
    let log_targets = {
        let mut t = TracingTargets::new().with_default(config.logging.level);
        for (target, lvl) in &config.logging.overrides {
            t = t.with_target(target.clone(), *lvl);
        }
        t
    };
    tracing_subscriber::registry()
        .with(log_targets)
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Debug print config at debug level
    debug!("Configuration loaded: {:#?}", config);

    info!("hearthd starting");
    info!("Loaded config from: {}", cli.config.display());

    let mut state: Arc<State> = Arc::default();

    // Start all enabled HA integrations
    let mut ha_registry = HaRegistry::default();
    for (name, integration) in &config.integrations.ha {
        if !integration.enabled {
            info!("Integration {} is disabled, skipping", name);
            continue;
        }

        ha_registry
            .register(SandboxBuilder::new(
                name.clone(),
                config.system.python_path.clone(),
                config.system.ha_source_path.clone(),
            ))
            .await?;
    }

    info!("All integrations started, entering main loop");
    info!("Press Ctrl+C to exit");

    // Wait for Ctrl+C
    loop {
        tokio::select!(
            maybe_cancelled = tokio::signal::ctrl_c() => {
                match maybe_cancelled {
                    Ok(()) => {
                        info!("Received shutdown signal, exiting...");
                        break;
                    }
                    Err(e) => {
                        error!("Failed to listen for shutdown signal: {}", e);
                        break;
                    },
                }
            }

            Err(e) = ha_registry.run() => {
                error!("Fatal error from ha_registry: {}", e);
                break;
            }
        );
    }

    info!("hearthd shutdown complete");
    Ok(())
}
