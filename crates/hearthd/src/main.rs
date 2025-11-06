mod config;
mod ha;

use config::Config;
use ha::{protocol::Response, Runtime, Sandbox};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse config file path from CLI or use default
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "hearthd.toml".to_string());

    // Load configuration
    let config = Config::from_file(&config_path)?;

    // Initialize tracing/logging
    tracing_subscriber::fmt()
        .with_max_level(parse_log_level(&config.system.log_level))
        .init();

    tracing::info!("hearthd starting");
    tracing::info!("Loaded config from: {}", config_path);
    tracing::info!(
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
            tracing::info!("Integration {} is disabled, skipping", entry_id);
            continue;
        }

        tracing::info!(
            "Starting HA integration: {} (domain: {})",
            entry_id,
            integration.domain
        );

        // Convert config to JSON for Python
        let config_json = integration.config_to_json()?;
        runtime.register_ha_config(entry_id.clone(), config_json.clone());

        // Create and start sandbox
        let mut sandbox = Sandbox::new(
            entry_id.clone(),
            config.system.python_path.clone(),
            config.system.ha_source_path.clone()
        );

        match sandbox.start().await {
            Ok(()) => {
                tracing::info!("[{}] Sandbox started successfully", entry_id);

                // Wait for Ready message
                match sandbox.recv().await {
                    Ok(msg) => {
                        tracing::info!("[{}] Received message: {:?}", entry_id, msg);

                        // Send SetupIntegration
                        let setup = Response::SetupIntegration {
                            domain: integration.domain.clone(),
                            entry_id: entry_id.clone(),
                            config: config_json,
                        };

                        match sandbox.send(setup).await {
                            Ok(()) => {
                                tracing::info!("[{}] Sent SetupIntegration", entry_id);

                                // Wait for setup response
                                match sandbox.recv().await {
                                    Ok(response_msg) => {
                                        use ha::protocol::Message;
                                        match response_msg {
                                            Message::SetupComplete { ref entry_id, ref platforms } => {
                                                tracing::info!("[{}] Integration setup complete", entry_id);
                                                tracing::info!("[{}] Platforms: {:?}", entry_id, platforms);
                                            }
                                            Message::SetupFailed { ref entry_id, ref error, ref error_type, ref missing_package } => {
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
                                                        tracing::error!("[{}] Error: {}", entry_id, error);
                                                    }
                                                    Some("integration_not_found") => {
                                                        tracing::error!(
                                                            "[{}] Integration setup failed: Integration not found in HA source",
                                                            entry_id
                                                        );
                                                        tracing::error!("[{}] Error: {}", entry_id, error);
                                                    }
                                                    Some("invalid_integration") => {
                                                        tracing::error!(
                                                            "[{}] Integration setup failed: Invalid integration structure",
                                                            entry_id
                                                        );
                                                        tracing::error!("[{}] Error: {}", entry_id, error);
                                                    }
                                                    _ => {
                                                        tracing::error!("[{}] Integration setup failed: {}", entry_id, error);
                                                    }
                                                }
                                            }
                                            _ => {
                                                tracing::warn!("[{}] Unexpected message: {:?}", entry_id, response_msg);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("[{}] Failed to receive setup response: {}", entry_id, e);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("[{}] Failed to send SetupIntegration: {}", entry_id, e);
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

    tracing::info!("All integrations started, entering main loop");
    tracing::info!("Press Ctrl+C to exit");

    // Wait for Ctrl+C
    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            tracing::info!("Received shutdown signal");
        }
        Err(e) => {
            tracing::error!("Failed to listen for shutdown signal: {}", e);
        }
    }

    // Shutdown all sandboxes
    tracing::info!("Shutting down sandboxes...");
    for (entry_id, mut sandbox) in sandboxes {
        tracing::info!("[{}] Stopping sandbox", entry_id);
        if let Err(e) = sandbox.stop().await {
            tracing::error!("[{}] Error stopping sandbox: {}", entry_id, e);
        }
    }

    tracing::info!("hearthd shutdown complete");

    Ok(())
}

fn parse_log_level(level: &str) -> tracing::Level {
    match level.to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" | "warning" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => {
            eprintln!("Invalid log level '{}', defaulting to INFO", level);
            tracing::Level::INFO
        }
    }
}
