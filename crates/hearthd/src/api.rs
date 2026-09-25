use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use serde::Deserialize;
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

use crate::Engine;
use crate::engine::Event;
use crate::engine::NodeId;
use crate::matter::AttributeWrite;
use crate::matter::ClusterCommand;
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

/// Request body for POST /v1/entities/:id/write
///
/// The body addresses a Matter endpoint within the resolved node and carries
/// the attribute write to perform. Example:
///   { "endpoint": 1, "write": { "cluster": "Thermostat", "attribute": "system_mode", "value": "Cool" } }
#[derive(Debug, Deserialize)]
struct EntityWriteRequest {
    endpoint: EndpointId,
    write: AttributeWrite,
}

/// Response for the POST /v1/entities/:id/{command,write} endpoints
#[derive(Serialize)]
struct SubmitResponse {
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

/// Handler for GET /v1/state
#[tracing::instrument(skip(state))]
async fn get_state(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::debug!("Handling /v1/state request");

    let snapshot = state.engine.state_snapshot();

    (StatusCode::OK, Json(snapshot))
}

/// Resolve an entity and put the event `build` makes for its node on the
/// engine's stream, so it is ordered with every report and request before
/// it. Answers once the event is queued; what the device did about it shows
/// up as a later report in `/v1/state`.
async fn submit_for_entity(
    state: &AppState,
    entity_id: &str,
    what: &str,
    build: impl FnOnce(NodeId) -> Event,
) -> Response {
    let node_id = match state.engine.resolve_entity_id(entity_id) {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(SubmitResponse {
                    success: false,
                    message: format!("Unknown entity: {}", entity_id),
                }),
            )
                .into_response();
        }
    };

    match state.engine.submit(build(node_id)).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(SubmitResponse {
                success: true,
                message: format!("{} queued for entity {}", what, entity_id),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SubmitResponse {
                success: false,
                message: format!("Engine is not accepting events: {}", e),
            }),
        )
            .into_response(),
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

    submit_for_entity(&state, &entity_id, "Command", |node_id| Event::Invoke {
        node_id,
        endpoint_id: request.endpoint,
        command: request.command,
    })
    .await
}

/// Handler for POST /v1/entities/{id}/write
#[tracing::instrument(skip(state))]
async fn write_entity_attribute(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<String>,
    Json(request): Json<EntityWriteRequest>,
) -> impl IntoResponse {
    tracing::debug!(
        "Handling POST /v1/entities/{}/write: endpoint={} write={:?}",
        entity_id,
        request.endpoint,
        request.write
    );

    submit_for_entity(&state, &entity_id, "Write", |node_id| Event::Write {
        node_id,
        endpoint_id: request.endpoint,
        write: request.write,
    })
    .await
}

/// Create the API router with all endpoints
fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/ping", get(ping))
        .route("/v1/info", get(info))
        .route("/v1/state", get(get_state))
        .route("/v1/entities/{id}/command", post(send_entity_command))
        .route("/v1/entities/{id}/write", post(write_entity_attribute))
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
