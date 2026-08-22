//! Raw TCP JSON-RPC client for Snapcast.
//!
//! The Snapcast control protocol is newline-delimited JSON-RPC 2.0. This
//! module handles connection framing, request/response matching, and routing
//! server-pushed notifications back to the integration.
//!
//! The client owns only the write half of the socket; the read half lives in
//! the connection task, which matches responses to waiting callers and
//! forwards everything else as a [`ClientEvent`]. Callers therefore keep a
//! usable handle for the lifetime of the integration, across reconnects.

use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::WriteHalf;
use tokio::io::split;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::models::RpcNotification;
use super::models::RpcRequest;
use super::models::RpcResponse;

/// Capacity for the event channel.
const EVENT_CHANNEL_SIZE: usize = 256;

/// How long to wait for a response before giving up on it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// A pending request waiting for its JSON-RPC response.
type PendingRequest = oneshot::Sender<Result<serde_json::Value, RpcClientError>>;

/// Something the connection task wants the integration to know about.
#[derive(Debug)]
pub enum ClientEvent {
    /// A connection was established. The integration's view of the server may
    /// be arbitrarily stale after a drop, so this is the cue to resynchronise
    /// rather than to trust what it already has.
    Connected,

    /// The server pushed a notification.
    Notification(RpcNotification),
}

/// Errors that can occur when using the Snapcast RPC client.
#[derive(Debug)]
pub enum RpcClientError {
    /// Connection failed or was lost.
    Io(io::Error),

    /// The server returned a JSON-RPC error.
    JsonRpc { code: i64, message: String },

    /// No connection is currently established.
    NotConnected,

    /// A request was cancelled because the connection was lost.
    Cancelled,

    /// The server did not answer within [`REQUEST_TIMEOUT`].
    TimedOut,

    /// The response could not be parsed.
    Parse(serde_json::Error),
}

impl std::fmt::Display for RpcClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcClientError::Io(e) => write!(f, "io error: {e}"),
            RpcClientError::JsonRpc { code, message } => {
                write!(f, "jsonrpc error {code}: {message}")
            }
            RpcClientError::NotConnected => write!(f, "not connected to snapserver"),
            RpcClientError::Cancelled => write!(f, "request cancelled"),
            RpcClientError::TimedOut => write!(f, "request timed out"),
            RpcClientError::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}

impl Error for RpcClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RpcClientError::Io(e) => Some(e),
            RpcClientError::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for RpcClientError {
    fn from(e: io::Error) -> Self {
        RpcClientError::Io(e)
    }
}

impl From<serde_json::Error> for RpcClientError {
    fn from(e: serde_json::Error) -> Self {
        RpcClientError::Parse(e)
    }
}

/// Handle to a connected (or reconnecting) Snapcast control client.
pub struct SnapcastRpcClient {
    host: String,
    port: u16,
    reconnect_interval_ms: u64,
    outbound: Arc<Mutex<Outbound>>,
    pending: Arc<Mutex<Pending>>,
    event_tx: mpsc::Sender<ClientEvent>,
}

/// Write half of the current connection, if any.
///
/// Held apart from [`Pending`] so that a write blocked on a full socket
/// buffer cannot also block the reader from handing responses to the callers
/// waiting on them, which would time out requests the server had already
/// answered.
#[derive(Default)]
struct Outbound {
    write: Option<WriteHalf<TcpStream>>,
}

/// Requests sent and not yet answered.
struct Pending {
    next_id: u64,
    requests: HashMap<u64, PendingRequest>,
}

impl SnapcastRpcClient {
    /// Create a client that will connect to `host:port`.
    pub fn new(
        host: String,
        port: u16,
        reconnect_interval_ms: u64,
    ) -> (Self, mpsc::Receiver<ClientEvent>) {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);
        (
            Self {
                host,
                port,
                reconnect_interval_ms,
                outbound: Arc::new(Mutex::new(Outbound::default())),
                pending: Arc::new(Mutex::new(Pending {
                    next_id: 1,
                    requests: HashMap::new(),
                })),
                event_tx,
            },
            event_rx,
        )
    }

    /// Start the background connect/read loop. It runs until aborted,
    /// reconnecting whenever the connection drops.
    pub fn spawn(&self) -> tokio::task::JoinHandle<()> {
        let outbound = self.outbound.clone();
        let pending = self.pending.clone();
        let event_tx = self.event_tx.clone();
        let host = self.host.clone();
        let port = self.port;
        let reconnect_interval_ms = self.reconnect_interval_ms;
        tokio::spawn(async move {
            run_connection_loop(
                host,
                port,
                reconnect_interval_ms,
                outbound,
                pending,
                event_tx,
            )
            .await
        })
    }

    /// Send a JSON-RPC request and wait for its response.
    ///
    /// Fails immediately when no connection is established: a command is only
    /// meaningful against the state the caller saw, so blocking until the
    /// server returns would apply it against a world that has since moved on,
    /// and would stall the integration's message loop while it waited.
    pub async fn request<T, R>(&self, method: &str, params: T) -> Result<R, RpcClientError>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let (tx, rx) = oneshot::channel();
        let (id, mut line) = {
            let mut guard = self.pending.lock().await;
            let id = guard.next_id;
            guard.next_id += 1;
            let request = RpcRequest::new(id, method.to_string(), params);
            let line = serde_json::to_vec(&request)?;
            guard.requests.insert(id, tx);
            (id, line)
        };
        line.push(b'\n');

        if let Err(e) = self.send_raw(&line).await {
            self.pending.lock().await.requests.remove(&id);
            return Err(e);
        }

        let result = match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RpcClientError::Cancelled),
            Err(_) => {
                self.pending.lock().await.requests.remove(&id);
                Err(RpcClientError::TimedOut)
            }
        }?;

        Ok(serde_json::from_value(result)?)
    }

    /// Write one already newline-terminated frame to the current connection.
    async fn send_raw(&self, line: &[u8]) -> Result<(), RpcClientError> {
        let mut guard = self.outbound.lock().await;
        let write = guard.write.as_mut().ok_or(RpcClientError::NotConnected)?;
        match write.write_all(line).await {
            Ok(()) => Ok(write.flush().await?),
            Err(e) => {
                // The connection task owns reconnection; dropping the write
                // half here just stops later sends going into a dead socket.
                guard.write = None;
                Err(RpcClientError::Io(e))
            }
        }
    }
}

async fn run_connection_loop(
    host: String,
    port: u16,
    reconnect_interval_ms: u64,
    outbound: Arc<Mutex<Outbound>>,
    pending: Arc<Mutex<Pending>>,
    event_tx: mpsc::Sender<ClientEvent>,
) {
    loop {
        info!("Connecting to Snapserver at {host}:{port}");
        match TcpStream::connect((host.as_str(), port)).await {
            Ok(stream) => {
                info!("Connected to Snapserver {host}:{port}");
                let (read_half, write_half) = split(stream);
                {
                    let mut guard = outbound.lock().await;
                    guard.write = Some(write_half);
                }

                // Announced only once the socket is writable, so the
                // resynchronise this triggers cannot race the connection it
                // depends on.
                if event_tx.send(ClientEvent::Connected).await.is_err() {
                    debug!("Snapcast event receiver dropped; stopping connection loop");
                    return;
                }

                if let Err(e) = read_loop(read_half, &pending, &event_tx).await {
                    warn!("Snapcast read loop ended: {e}");
                } else {
                    warn!("Snapcast connection closed by server");
                }

                outbound.lock().await.write = None;
                let abandoned = std::mem::take(&mut pending.lock().await.requests);
                for (_, tx) in abandoned {
                    let _ = tx.send(Err(RpcClientError::Cancelled));
                }
            }
            Err(e) => {
                warn!("Failed to connect to Snapserver {host}:{port}: {e}");
            }
        }

        tokio::time::sleep(Duration::from_millis(reconnect_interval_ms)).await;
    }
}

async fn read_loop(
    read_half: tokio::io::ReadHalf<TcpStream>,
    pending: &Arc<Mutex<Pending>>,
    event_tx: &mpsc::Sender<ClientEvent>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        debug!("Snapcast RPC line: {line}");

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to parse Snapcast RPC line: {e}");
                continue;
            }
        };

        // A response carries an id and a notification does not. Testing for
        // the id is what separates them; trying a notification parse first
        // would also match any response whose result happened to contain a
        // `method` key.
        if value.get("id").is_some() {
            dispatch_response(value, pending).await;
            continue;
        }

        match serde_json::from_value::<RpcNotification>(value) {
            Ok(notification) => {
                if event_tx
                    .send(ClientEvent::Notification(notification))
                    .await
                    .is_err()
                {
                    debug!("Snapcast event receiver dropped; stopping read loop");
                    return Ok(());
                }
            }
            Err(e) => warn!("Failed to parse Snapcast notification: {e}"),
        }
    }

    Ok(())
}

async fn dispatch_response(value: serde_json::Value, pending: &Arc<Mutex<Pending>>) {
    let response: RpcResponse<serde_json::Value> = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to parse Snapcast RPC response: {e}");
            return;
        }
    };

    let Some(id) = response.id else {
        warn!("Snapcast RPC response has no id");
        return;
    };

    let waiter = pending.lock().await.requests.remove(&id);

    let Some(tx) = waiter else {
        warn!("Received unexpected Snapcast RPC response for id {id}");
        return;
    };

    let result = match response.error {
        Some(err) => Err(RpcClientError::JsonRpc {
            code: err.code,
            message: err.message,
        }),
        None => Ok(response.result.unwrap_or(serde_json::Value::Null)),
    };
    let _ = tx.send(result);
}
