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
use crate::engine::ResolveError;
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

/// Request body for POST /v1/nodes/:name/command
///
/// The body addresses a Matter endpoint within the resolved node and carries
/// the cluster command to invoke. Example:
///   { "endpoint": 1, "command": { "command": "OnOffOn" } }
#[derive(Debug, Deserialize)]
struct NodeCommandRequest {
    endpoint: EndpointId,
    command: ClusterCommand,
}

/// Request body for POST /v1/nodes/:name/write
///
/// The body addresses a Matter endpoint within the resolved node and carries
/// the attribute write to perform. Example:
///   { "endpoint": 1, "write": { "cluster": "Thermostat", "attribute": "system_mode", "value": "Cool" } }
#[derive(Debug, Deserialize)]
struct NodeWriteRequest {
    endpoint: EndpointId,
    write: AttributeWrite,
}

/// Response for the POST /v1/nodes/:name/{command,write} endpoints
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

fn failure(status: StatusCode, message: String) -> Response {
    (
        status,
        Json(SubmitResponse {
            success: false,
            message,
        }),
    )
        .into_response()
}

/// Resolve a node by alias, discovered name or id, and put the event
/// `build` makes for it on the engine's stream, so it is ordered with every
/// report and request before it. Answers once the event is queued; what the
/// device did about it shows up as a later report in `/v1/state`.
async fn submit_for_node(
    state: &AppState,
    name: &str,
    what: &str,
    build: impl FnOnce(NodeId) -> Event,
) -> Response {
    let node_id = match state.engine.resolve(name) {
        Ok(id) => id,
        Err(e @ ResolveError::Unknown(_)) => return failure(StatusCode::NOT_FOUND, e.to_string()),
        Err(e @ ResolveError::Ambiguous { .. }) => {
            return failure(StatusCode::CONFLICT, e.to_string());
        }
    };

    // An alias resolves from startup, so this is the ordinary answer while
    // its device is still registering, and the message says which device
    // that is.
    if !state.engine.state_snapshot().nodes.contains_key(&node_id) {
        let message = match state.engine.alias_target(name) {
            Some(target) => {
                format!(
                    "alias {name} names {target} (node {node_id}), which has not been announced"
                )
            }
            None => format!("node {node_id} has not been announced"),
        };
        return failure(StatusCode::NOT_FOUND, message);
    }

    match state.engine.submit(build(node_id)).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(SubmitResponse {
                success: true,
                message: format!("{what} queued for node {name}"),
            }),
        )
            .into_response(),
        Err(e) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Engine is not accepting events: {e}"),
        ),
    }
}

/// Handler for POST /v1/nodes/{name}/command
#[tracing::instrument(skip(state))]
async fn send_node_command(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<NodeCommandRequest>,
) -> impl IntoResponse {
    tracing::debug!(
        "Handling POST /v1/nodes/{}/command: endpoint={} command={:?}",
        name,
        request.endpoint,
        request.command
    );

    submit_for_node(&state, &name, "Command", |node_id| Event::Invoke {
        node_id,
        endpoint_id: request.endpoint,
        command: request.command,
    })
    .await
}

/// Handler for POST /v1/nodes/{name}/write
#[tracing::instrument(skip(state))]
async fn write_node_attribute(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<NodeWriteRequest>,
) -> impl IntoResponse {
    tracing::debug!(
        "Handling POST /v1/nodes/{}/write: endpoint={} write={:?}",
        name,
        request.endpoint,
        request.write
    );

    submit_for_node(&state, &name, "Write", |node_id| Event::Write {
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
        .route("/v1/nodes/{name}/command", post(send_node_command))
        .route("/v1/nodes/{name}/write", post(write_node_attribute))
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
