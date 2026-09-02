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
use crate::matter::ClusterCommand;
use crate::matter::ClusterWrite;
use crate::matter::EndpointId;

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
///
/// The body addresses a Matter endpoint within the resolved node and carries
/// the cluster command to invoke. Example:
///   { "endpoint": 1, "command": { "command": "OnOffOn" } }
#[derive(Debug, Deserialize)]
struct EntityCommandRequest {
    endpoint: EndpointId,
    command: ClusterCommand,
}

/// Response for POST /v1/entities/:id/command
#[derive(Serialize)]
struct EntityCommandResponse {
    success: bool,
    message: String,
}

/// Request body for POST /v1/entities/:id/attributes
///
/// The body addresses a Matter endpoint within the resolved node and carries
/// a list of cluster attribute writes to apply. Each entry targets one cluster
/// and may set any subset of that cluster's writable attributes. Example:
///   {
///     "endpoint": 1,
///     "writes": [
///       { "cluster": "Thermostat", "system_mode": "Heat" },
///       { "cluster": "Thermostat", "occupied_heating_setpoint": 2100 }
///     ]
///   }
#[derive(Debug, Deserialize)]
struct EntityAttributesRequest {
    endpoint: EndpointId,
    writes: Vec<ClusterWrite>,
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

/// Handler for GET /v1/state
#[tracing::instrument(skip(state))]
async fn get_state(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::debug!("Handling /v1/state request");

    let snapshot = state.engine.state_snapshot();

    (StatusCode::OK, Json(snapshot))
}

/// Resolve `entity_id` to a `NodeId`, returning a 404 response on failure.
fn resolve_entity(
    engine: &crate::Engine,
    entity_id: &str,
) -> Result<crate::engine::NodeId, (StatusCode, Json<EntityCommandResponse>)> {
    match engine.resolve_entity_id(entity_id) {
        Some(id) => Ok(id),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(EntityCommandResponse {
                success: false,
                message: format!("Unknown entity: {}", entity_id),
            }),
        )),
    }
}

/// Handler for POST /v1/entities/{id}/command
#[tracing::instrument(skip(state))]
async fn send_entity_command(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<String>,
    Json(request): Json<EntityCommandRequest>,
) -> impl IntoResponse {
    tracing::debug!(
        "Handling POST /v1/entities/{}/command: endpoint={} command={:?}",
        entity_id,
        request.endpoint,
        request.command
    );

    let node_id = match resolve_entity(&state.engine, &entity_id) {
        Ok(id) => id,
        Err(response) => return response.into_response(),
    };

    match state
        .engine
        .invoke_command(node_id, request.endpoint, request.command)
    {
        Ok(()) => (
            StatusCode::OK,
            Json(EntityCommandResponse {
                success: true,
                message: format!("Command sent to entity {}", entity_id),
            }),
        )
            .into_response(),
        Err(e) => {
            let status = if e.to_string().contains("unknown node")
                || e.to_string().contains("has no endpoint")
                || e.to_string().contains("does not support cluster")
                || e.to_string().contains("is read-only")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(EntityCommandResponse {
                    success: false,
                    message: format!("Failed to send command: {}", e),
                }),
            )
                .into_response()
        }
    }
}

/// Handler for POST /v1/entities/{id}/attributes
#[tracing::instrument(skip(state))]
async fn write_entity_attributes(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<String>,
    Json(request): Json<EntityAttributesRequest>,
) -> impl IntoResponse {
    tracing::debug!(
        "Handling POST /v1/entities/{}/attributes: endpoint={} writes={:?}",
        entity_id,
        request.endpoint,
        request.writes
    );

    let node_id = match resolve_entity(&state.engine, &entity_id) {
        Ok(id) => id,
        Err(response) => return response.into_response(),
    };

    match state
        .engine
        .write_attributes(node_id, request.endpoint, request.writes)
    {
        Ok(()) => (
            StatusCode::OK,
            Json(EntityCommandResponse {
                success: true,
                message: format!("Attributes written to entity {}", entity_id),
            }),
        )
            .into_response(),
        Err(e) => {
            let status = if e.to_string().contains("unknown node")
                || e.to_string().contains("has no endpoint")
                || e.to_string().contains("does not support cluster")
                || e.to_string().contains("is read-only")
                || e.to_string().contains("no fields to set")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(EntityCommandResponse {
                    success: false,
                    message: format!("Failed to write attributes: {}", e),
                }),
            )
                .into_response()
        }
    }
}

/// Create the API router with all endpoints
fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/ping", get(ping))
        .route("/v1/info", get(info))
        .route("/v1/state", get(get_state))
        .route("/v1/entities/{id}/command", post(send_entity_command))
        .route(
            "/v1/entities/{id}/attributes",
            post(write_entity_attributes),
        )
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
