use std::path::PathBuf;

use clap::Parser;
use hearthd::Config;
use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;
use tracing::debug;
use tracing::info;
use tracing::warn;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    // Notify systemd that the service is ready
    #[cfg(feature = "systemd")]
    {
        if let Err(e) = libsystemd::daemon::notify(false, &[libsystemd::daemon::NotifyState::Ready])
        {
            warn!("Failed to notify systemd: {}", e);
        } else {
            debug!("Notified systemd that service is ready");
        }
    }

    // Set up signal handlers
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    // Create shutdown channel for HTTP server
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Start HTTP API server
    let http_listen = cfg.http.listen.clone();
    let http_port = cfg.http.port;
    let http_server = tokio::spawn(async move {
        if let Err(e) = hearthd::api::serve(http_listen, http_port, shutdown_rx).await {
            warn!("HTTP API server error: {}", e);
        }
    });

    info!("hearthd ready, waiting for exit signal (SIGINT or SIGTERM)");

    // Wait for shutdown signal
    tokio::select! {
        _ = sigterm.recv() => {
            info!("Received SIGTERM, shutting down gracefully");
        }
        _ = sigint.recv() => {
            info!("Received SIGINT, shutting down gracefully");
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
