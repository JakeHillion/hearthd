use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

/// Response for the /v1/ping endpoint
#[derive(Serialize)]
struct PingResponse {
    status: String,
}

/// Response for the /v1/info endpoint
#[derive(Serialize)]
struct InfoResponse {
    version: String,
    hostname: String,
}

/// Shared application state
#[derive(Clone)]
struct AppState {
    version: &'static str,
}

/// Handler for GET /v1/ping
#[tracing::instrument]
async fn ping() -> impl IntoResponse {
    tracing::debug!("Handling /v1/ping request");
    (
        StatusCode::OK,
        Json(PingResponse {
            status: "ok".to_string(),
        }),
    )
}

/// Handler for GET /v1/info
#[tracing::instrument(skip(state))]
async fn info(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::debug!("Handling /v1/info request");

    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    (
        StatusCode::OK,
        Json(InfoResponse {
            version: state.version.to_string(),
            hostname,
        }),
    )
}

/// Create the API router with all endpoints
fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/ping", get(ping))
        .route("/v1/info", get(info))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Start the HTTP API server
///
/// This function will bind to the specified address and serve the API endpoints.
/// It will run until the provided shutdown signal is triggered.
///
/// # Arguments
/// * `listen` - The IP address to listen on (e.g., "127.0.0.1")
/// * `port` - The port to listen on (e.g., 8565)
/// * `shutdown_rx` - A oneshot receiver that will trigger graceful shutdown
///
/// # Returns
/// Returns Ok(()) if the server shuts down gracefully, or an error if startup fails
pub async fn serve(
    listen: String,
    port: u16,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");

    let state = Arc::new(AppState { version });
    let app = create_router(state);

    let addr: SocketAddr = format!("{}:{}", listen, port).parse()?;
    tracing::info!("Starting HTTP API server on {}", addr);

    let listener = TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
            tracing::info!("HTTP API server shutting down gracefully");
        })
        .await?;

    Ok(())
}
