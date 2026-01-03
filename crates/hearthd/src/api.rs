use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::routing::post;
use serde::Deserialize;
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

use crate::Engine;

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

/// Request body for POST /v1/entities/:id/command
#[derive(Debug, Deserialize)]
#[serde(tag = "command")]
enum EntityCommandRequest {
    #[serde(rename = "light")]
    Light { on: bool, brightness: Option<u8> },
}

/// Response for POST /v1/entities/:id/command
#[derive(Serialize)]
struct EntityCommandResponse {
    success: bool,
    message: String,
}

/// Shared application state
#[derive(Clone)]
struct AppState {
    version: &'static str,
    engine: Arc<Engine>,
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

/// Handler for GET /v1/dump_state
#[tracing::instrument(skip(state))]
async fn dump_state(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::debug!("Handling /v1/dump_state request");

    let entities = state.engine.get_all_entities_json().await;

    (StatusCode::OK, Json(entities))
}

/// Handler for GET /v1/entities
#[tracing::instrument(skip(state))]
async fn list_entities(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::debug!("Handling /v1/entities request");

    let entities = state.engine.get_all_entities_json().await;

    (StatusCode::OK, Json(entities))
}

/// Handler for GET /v1/entities/:id
#[tracing::instrument(skip(state))]
async fn get_entity(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<String>,
) -> impl IntoResponse {
    tracing::debug!("Handling /v1/entities/{} request", entity_id);

    match state.engine.get_entity_json(&entity_id).await {
        Some(entity) => (StatusCode::OK, Json(entity)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Entity not found",
                "entity_id": entity_id
            })),
        )
            .into_response(),
    }
}

/// Handler for POST /v1/entities/:id/command
#[tracing::instrument(skip(state))]
async fn send_entity_command(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<String>,
    Json(request): Json<EntityCommandRequest>,
) -> impl IntoResponse {
    match request {
        EntityCommandRequest::Light { on, brightness } => {
            tracing::debug!(
                "Handling POST /v1/entities/{}/command: light on={} brightness={:?}",
                entity_id,
                on,
                brightness
            );

            match state
                .engine
                .send_light_command(entity_id.clone(), on, brightness)
            {
                Ok(()) => (
                    StatusCode::OK,
                    Json(EntityCommandResponse {
                        success: true,
                        message: format!("Command sent to entity {}", entity_id),
                    }),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(EntityCommandResponse {
                        success: false,
                        message: format!("Failed to send command: {}", e),
                    }),
                )
                    .into_response(),
            }
        }
    }
}

/// Create the API router with all endpoints
fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/ping", get(ping))
        .route("/v1/info", get(info))
        .route("/v1/dump_state", get(dump_state))
        .route("/v1/entities", get(list_entities))
        .route("/v1/entities/:id", get(get_entity))
        .route("/v1/entities/:id/command", post(send_entity_command))
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
/// * `engine` - Shared reference to the Engine for querying state
/// * `shutdown_rx` - A oneshot receiver that will trigger graceful shutdown
///
/// # Returns
/// Returns Ok(()) if the server shuts down gracefully, or an error if startup fails
pub async fn serve(
    listen: String,
    port: u16,
    engine: Arc<Engine>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");

    let state = Arc::new(AppState { version, engine });
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
