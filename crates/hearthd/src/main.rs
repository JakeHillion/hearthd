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

    // Read every automation source now, but compile later: type-checking one
    // needs a deployment schema, and that is not known until integrations
    // have reported what they found.
    let mut sources: Vec<AutomationSource> = Vec::new();
    for (name, entry) in &cfg.automations.automations {
        match std::fs::read_to_string(&entry.file) {
            Ok(source) => sources.push(AutomationSource {
                name: name.clone(),
                path: entry.file.clone(),
                source,
            }),
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

    // An automation binds the devices it acts on by name, so it can only be
    // relocated once those devices are known. Integrations discover
    // asynchronously and nothing yet says when they have settled, so this
    // waits a fixed moment for them to report; a real settled signal should
    // replace it, and until then a device that arrives late is missing from
    // the schema and an automation naming it fails to relocate.
    tokio::time::sleep(DISCOVERY_SETTLE).await;
    let schema = Arc::new(hearthd::automations::schema::DeploymentSchema::from_state(
        &engine.state_snapshot(),
    ));

    let mut compiled = Vec::new();
    for automation in &sources {
        match compile_automation(automation, &schema) {
            Ok(auto) => compiled.push(auto),
            Err(diagnostic) => warn!(
                "Failed to compile '{}' ({}):\n{}",
                automation.name, automation.path, diagnostic
            ),
        }
    }
    if !compiled.is_empty() {
        let count = compiled.len();
        engine.set_runner(Arc::new(hearthd::automations::runner::Runner::new(
            engine.clone(),
            schema,
            compiled,
        )));
        info!("Running {} automation(s)", count);
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

/// How long to let integrations report what they discovered before the
/// deployment schema is taken.
const DISCOVERY_SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// One automation file, as read from disk.
struct AutomationSource {
    /// The name it is configured under, for diagnostics.
    name: String,
    path: String,
    source: String,
}

/// Run one automation through the whole compile pipeline — parse, desugar,
/// check, then lower to HIR, LIR and relocatable bytecode — and relocate it
/// against the deployment schema before building the VMs the runner executes.
///
/// The error is a rendered diagnostic rather than a value, because the only
/// thing that happens to it is being logged; a parse, type or relocation
/// failure names the span it came from the same way the tests do.
fn compile_automation(
    automation: &AutomationSource,
    schema: &hearthd::automations::schema::DeploymentSchema,
) -> Result<hearthd::automations::runner::CompiledAutomation, String> {
    let AutomationSource { path, source, .. } = automation;
    let program = hearthd::automations::parse(source).map_err(|errors| {
        errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let lowered = hearthd::automations::desugar_program(program);
    let typed = hearthd::automations::check::check_program(&lowered);
    if !typed.errors.is_empty() {
        return Err(hearthd::automations::check::format_type_errors(
            &typed.errors,
            source,
            path,
        ));
    }
    let hir = hearthd::automations::lower_program(&typed);
    let lir = hearthd::automations::lower_lir_program(&hir);
    let relocatable = hearthd::automations::lower_bytecode_program(&lir);
    // An entity this deployment does not have is a link failure, not a
    // compile one, so it is reported against the name in the source that
    // could not be resolved rather than against the file as a whole.
    let bytecode =
        hearthd::automations::relocate_program(&relocatable, schema).map_err(|unresolved| {
            let errors: Vec<_> = unresolved
                .iter()
                .map(|u| hearthd::automations::check::typed::TypeError {
                    message: u.to_string(),
                    span: u.symbol.span,
                })
                .collect();
            hearthd::automations::check::format_type_errors(&errors, source, path)
        })?;
    let auto = match bytecode {
        hearthd::automations::bytecode::BytecodeProgram::Automation(auto) => auto,
        // A template declares parameters, so it describes automations rather
        // than being one; nothing instantiates them yet.
        hearthd::automations::bytecode::BytecodeProgram::Template { .. } => {
            return Err("templates are not supported yet".to_string());
        }
    };
    hearthd::automations::runner::CompiledAutomation::new(
        auto.kind,
        // An observer without a filter would run on every event. The parser
        // requires one, so this is unreachable from source.
        auto.filter
            .ok_or_else(|| "automation has no filter".to_string())?,
        auto.body,
    )
    .map_err(|e| e.to_string())
}
