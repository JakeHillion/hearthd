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

    // Read each automation source up front; we'll compile after
    // integrations have had a chance to discover devices.
    let mut sources: Vec<(String, String, String)> = Vec::new(); // (name, path, source)
    for (name, entry) in &cfg.automations.automations {
        match std::fs::read_to_string(&entry.file) {
            Ok(source) => sources.push((name.clone(), entry.file.clone(), source)),
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

    // Give integrations a brief window to publish their initial
    // discovery so the schema we build covers the real device set.
    // Real discovery-settled signalling can replace this later.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let snapshot = engine.state_snapshot();
    let schema = Arc::new(hearthd::automations::schema::DeploymentSchema::from_state(
        &snapshot,
    ));
    let mut compiled: Vec<hearthd::automations::runtime::CompiledAutomation> = Vec::new();
    for (id, (name, path, source)) in sources.iter().enumerate() {
        match compile_automation(id, source, schema.clone()) {
            Ok(auto) => compiled.push(auto),
            Err(diag) => warn!("Failed to compile '{}' ({}):\n{}", name, path, diag),
        }
    }
    if !compiled.is_empty() {
        let runner = Arc::new(hearthd::automations::runtime::Runner::new(
            engine.clone(),
            schema,
            compiled,
        ));
        engine.set_runner(runner);
        info!("automation runner installed");
    }

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

/// Run an automation source string through the full compile pipeline:
/// parse → desugar → check (with schema) → lower → lower_lir → lower_bytecode.
/// Returns a single `CompiledAutomation` for a top-level observer/mutator.
/// Template programs are not supported yet and produce an error.
fn compile_automation(
    id: usize,
    source: &str,
    schema: Arc<hearthd::automations::schema::DeploymentSchema>,
) -> Result<hearthd::automations::runtime::CompiledAutomation, String> {
    let program = hearthd::automations::parse(source).map_err(|e| format!("parse: {:?}", e))?;
    let lowered = hearthd::automations::desugar_program(program);
    let typed = hearthd::automations::check::check_program_with_schema(&lowered, schema);
    if !typed.errors.is_empty() {
        return Err(format!("type errors: {:?}", typed.errors));
    }
    let hir = hearthd::automations::lower_program(&typed);
    let lir = hearthd::automations::lower_lir_program(&hir);
    let bc = hearthd::automations::lower_bytecode_program(&lir);
    match bc {
        hearthd::automations::repr::BytecodeProgram::Automation(auto) => {
            Ok(hearthd::automations::runtime::CompiledAutomation {
                id,
                kind: auto.kind,
                filter: auto.filter.ok_or_else(|| "filter required".to_string())?,
                body: auto.body,
            })
        }
        hearthd::automations::repr::BytecodeProgram::Template { .. } => {
            Err("templates are not supported yet".to_string())
        }
    }
}
