mod config;
mod ha;

use config::Config;
use ha::Runtime;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    // Register all enabled HA integrations
    for (entry_id, integration) in config.integrations.ha {
        if !integration.enabled {
            tracing::info!("Integration {} is disabled, skipping", entry_id);
            continue;
        }

        tracing::info!(
            "Registering HA integration: {} (domain: {})",
            entry_id,
            integration.domain
        );

        // Convert config to JSON for Python
        let config_json = integration.config_to_json()?;
        runtime.register_ha_config(entry_id.clone(), config_json);

        // TODO: Create sandbox and send SetupIntegration message
        tracing::debug!(
            "Integration {} registered, sandbox creation not yet implemented",
            entry_id
        );
    }

    tracing::info!("Configuration complete");
    tracing::info!("Note: Sandbox and main loop not yet implemented");

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
