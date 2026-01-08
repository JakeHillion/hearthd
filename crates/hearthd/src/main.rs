use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tracing::debug;
use tracing::info;
use tracing::warn;
use tracing_subscriber::filter::Targets as TracingTargets;
use tracing_subscriber::prelude::*;

#[derive(Parser)]
#[command(name = "hearthd")]
#[command(about = "Home automation made declarative", long_about = None)]
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Load and parse the configuration files
    let (cfg, diagnostics) = match hearthd::Config::from_files(&cli.config) {
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

    // Load and parse automations
    for (name, entry) in &cfg.automations.automations {
        match std::fs::read_to_string(&entry.file) {
            Ok(source) => match hearthd::automations::parse(&source) {
                Ok(ast) => info!(?ast, "Parsed automation '{}': {}", name, entry.file),
                Err(errs) => warn!(?errs, "Failed to parse '{}': {}", name, entry.file),
            },
            Err(e) => warn!("Failed to read '{}' ({}): {}", name, entry.file, e),
        }
    }

    info!("hearthd starting");
    let mut engine = hearthd::Engine::new();

    // Register integrations from config
    engine.register_integrations_from_config(&cfg)?;

    // Wrap engine in Arc for thread-safe sharing
    let engine: Arc<hearthd::Engine> = Arc::new(engine);
    let engine_for_http = engine.clone();

    info!("hearthd started");

    // Set up signal handlers
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    // Create shutdown channel for HTTP server
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Start HTTP API server
    let http_listen = cfg.http.listen.clone();
    let http_port = cfg.http.port;
    let http_server = tokio::spawn(async move {
        if let Err(e) =
            hearthd::api::serve(http_listen, http_port, engine_for_http, shutdown_rx).await
        {
            warn!("HTTP API server error: {}", e);
        }
    });

    info!("hearthd ready, waiting for exit signal (SIGINT or SIGTERM)");

    // Run engine in background
    let engine_for_run = engine.clone();
    let mut engine_handle = tokio::spawn(async move {
        if let Err(e) = engine_for_run.run().await {
            warn!("Engine error: {}", e);
        }
    });

    // Main event loop - wait for shutdown signal or engine completion
    #[allow(clippy::never_loop)]
    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down gracefully");
                break;
            }
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down gracefully");
                break;
            }
            result = &mut engine_handle => {
                match result {
                    Ok(()) => {
                        warn!("Engine task completed unexpectedly");
                    }
                    Err(e) if e.is_panic() => {
                        warn!("Engine task panicked: {:?}", e);
                    }
                    Err(e) => {
                        warn!("Engine task failed: {}", e);
                    }
                }
                break;
            }
        }
    }

    // Stop the engine if it's still running
    engine_handle.abort();
    match engine_handle.await {
        Ok(()) => {
            info!("Engine shutdown complete");
        }
        Err(e) if e.is_cancelled() => {
            info!("Engine task cancelled");
        }
        Err(e) if e.is_panic() => {
            warn!("Engine task panicked during shutdown: {:?}", e);
        }
        Err(e) => {
            warn!("Engine task failed during shutdown: {}", e);
        }
    }

    // Trigger HTTP server shutdown
    if shutdown_tx.send(()).is_err() {
        warn!("HTTP server already stopped");
    }

    // Wait for HTTP server to finish
    match http_server.await {
        Ok(()) => debug!("HTTP server stopped cleanly"),
        Err(e) => warn!("HTTP server task error: {}", e),
    }

    info!("hearthd stopped");
    Ok(())
}
