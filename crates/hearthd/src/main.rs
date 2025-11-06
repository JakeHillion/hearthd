mod config;
mod ha;

use config::Config;
use ha::{protocol::Response, Runtime, Sandbox};
use std::collections::HashMap;
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
    let config = Config::from_file(&cli.config)?;

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
    info!(
        "System location: {}, {} (elevation: {}m, timezone: {})",
        config.location.latitude,
        config.location.longitude,
        config.location.elevation,
        config.location.timezone
    );

    // Create runtime with location config
    let mut runtime = Runtime::new(config.location);

    // Track sandboxes
    let mut sandboxes: HashMap<String, Sandbox> = HashMap::new();

    // Start all enabled HA integrations
    for (entry_id, integration) in config.integrations.ha {
        if !integration.enabled {
            info!("Integration {} is disabled, skipping", entry_id);
            continue;
        }

        info!(
            "Starting HA integration: {} (domain: {})",
            entry_id, integration.domain
        );

        // Convert config to JSON for Python
        let config_json = integration.config_to_json()?;
        runtime.register_ha_config(entry_id.clone(), config_json.clone());

        // Create and start sandbox
        let mut sandbox = Sandbox::new(
            entry_id.clone(),
            config.system.python_path.clone(),
            config.system.ha_source_path.clone(),
        );

        match sandbox.start().await {
            Ok(()) => {
                info!("[{}] Sandbox started successfully", entry_id);

                // Wait for Ready message
                match sandbox.recv().await {
                    Ok(msg) => {
                        info!("[{}] Received message: {:?}", entry_id, msg);

                        // Send SetupIntegration
                        let setup = Response::SetupIntegration {
                            domain: integration.domain.clone(),
                            entry_id: entry_id.clone(),
                            config: config_json,
                        };

                        match sandbox.send(setup).await {
                            Ok(()) => {
                                info!("[{}] Sent SetupIntegration", entry_id);

                                // Wait for setup response
                                match sandbox.recv().await {
                                    Ok(response_msg) => {
                                        use ha::protocol::Message;
                                        match response_msg {
                                            Message::SetupComplete {
                                                ref entry_id,
                                                ref platforms,
                                            } => {
                                                info!("[{}] Integration setup complete", entry_id);
                                                info!("[{}] Platforms: {:?}", entry_id, platforms);
                                            }
                                            Message::SetupFailed {
                                                ref entry_id,
                                                ref error,
                                                ref error_type,
                                                ref missing_package,
                                            } => {
                                                match error_type.as_deref() {
                                                    Some("missing_dependency") => {
                                                        tracing::error!(
                                                            "[{}] Integration setup failed: Missing Python dependency",
                                                            entry_id
                                                        );
                                                        if let Some(pkg) = missing_package {
                                                            tracing::error!(
                                                                "[{}] Please install: pip install {}",
                                                                entry_id, pkg
                                                            );
                                                        }
                                                        tracing::error!(
                                                            "[{}] Error: {}",
                                                            entry_id, error
                                                        );
                                                    }
                                                    Some("integration_not_found") => {
                                                        tracing::error!(
                                                            "[{}] Integration setup failed: Integration not found in HA source",
                                                            entry_id
                                                        );
                                                        tracing::error!(
                                                            "[{}] Error: {}",
                                                            entry_id, error
                                                        );
                                                    }
                                                    Some("invalid_integration") => {
                                                        tracing::error!(
                                                            "[{}] Integration setup failed: Invalid integration structure",
                                                            entry_id
                                                        );
                                                        tracing::error!(
                                                            "[{}] Error: {}",
                                                            entry_id, error
                                                        );
                                                    }
                                                    _ => {
                                                        tracing::error!(
                                                            "[{}] Integration setup failed: {}",
                                                            entry_id, error
                                                        );
                                                    }
                                                }
                                            }
                                            _ => {
                                                tracing::warn!(
                                                    "[{}] Unexpected message: {:?}",
                                                    entry_id, response_msg
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "[{}] Failed to receive setup response: {}",
                                            entry_id, e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "[{}] Failed to send SetupIntegration: {}",
                                    entry_id, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("[{}] Failed to receive Ready message: {}", entry_id, e);
                    }
                }

                sandboxes.insert(entry_id.clone(), sandbox);
            }
            Err(e) => {
                tracing::error!("[{}] Failed to start sandbox: {}", entry_id, e);
            }
        }
    }

    info!("All integrations started, entering main loop");
    info!("Press Ctrl+C to exit");

    // Wait for Ctrl+C
    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Received shutdown signal");
        }
        Err(e) => {
            tracing::error!("Failed to listen for shutdown signal: {}", e);
        }
    }

    // Shutdown all sandboxes
    info!("Shutting down sandboxes...");
    for (entry_id, mut sandbox) in sandboxes {
        info!("[{}] Stopping sandbox", entry_id);
        if let Err(e) = sandbox.stop().await {
            tracing::error!("[{}] Error stopping sandbox: {}", entry_id, e);
        }
    }

    info!("hearthd shutdown complete");

    Ok(())
}
