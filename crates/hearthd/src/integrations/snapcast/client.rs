//! Raw TCP JSON-RPC client for Snapcast.
//!
//! The Snapcast control protocol is newline-delimited JSON-RPC 2.0. This
//! module handles connection framing, request/response matching, and routing
//! notifications back to the integration.

use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
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

/// Default capacity for the notification channel.
const NOTIFICATION_CHANNEL_SIZE: usize = 256;

/// A pending request waiting for its JSON-RPC response.
type PendingRequest = oneshot::Sender<Result<serde_json::Value, RpcClientError>>;

/// Errors that can occur when using the Snapcast RPC client.
#[derive(Debug)]
pub enum RpcClientError {
    /// Connection failed or was lost.
    Io(io::Error),

    /// The server returned a JSON-RPC error.
    JsonRpc { code: i64, message: String },

    /// A request was cancelled because the connection was lost.
    Cancelled,

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
            RpcClientError::Cancelled => write!(f, "request cancelled"),
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
    inner: Arc<Mutex<ClientInner>>,
    notification_tx: mpsc::Sender<RpcNotification>,
}

struct ClientInner {
    host: String,
    port: u16,
    reconnect_interval_ms: u64,
    stream: Option<TcpStream>,
    write_half: Option<tokio::io::WriteHalf<tokio::net::TcpStream>>,
    next_id: u64,
    pending: HashMap<u64, PendingRequest>,
}

impl SnapcastRpcClient {
    /// Create a new client that will connect to `host:port`.
    pub fn new(
        host: String,
        port: u16,
        reconnect_interval_ms: u64,
    ) -> (Self, mpsc::Receiver<RpcNotification>) {
        let (notification_tx, notification_rx) = mpsc::channel(NOTIFICATION_CHANNEL_SIZE);
        let inner = Arc::new(Mutex::new(ClientInner {
            host,
            port,
            reconnect_interval_ms,
            stream: None,
            write_half: None,
            next_id: 1,
            pending: HashMap::new(),
        }));
        (
            Self {
                inner,
                notification_tx,
            },
            notification_rx,
        )
    }

    /// Connect to the Snapserver and start the background read task.
    pub fn spawn(&self) -> tokio::task::JoinHandle<()> {
        let inner = self.inner.clone();
        let notification_tx = self.notification_tx.clone();
        tokio::spawn(async move { run_connection_loop(inner, notification_tx).await })
    }

    /// Send a JSON-RPC request and wait for its response.
    pub async fn request<T, R>(&self, method: &str, params: T) -> Result<R, RpcClientError>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let (id, line) = {
            let mut guard = self.inner.lock().await;
            let id = guard.next_id;
            guard.next_id += 1;
            let request = RpcRequest::new(id, method.to_string(), params);
            let line = serde_json::to_vec(&request)?;
            (id, line)
        };

        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.inner.lock().await;
            guard.pending.insert(id, tx);
        }

        self.send_raw(&line).await?;
        let result = rx.await.unwrap_or(Err(RpcClientError::Cancelled))?;
        Ok(serde_json::from_value(result)?)
    }

    /// Send raw bytes followed by a newline. Reconnects if necessary.
    async fn send_raw(&self, line: &[u8]) -> Result<(), RpcClientError> {
        loop {
            let mut guard = self.inner.lock().await;
            if let Some(stream) = guard.stream.as_mut() {
                if let Err(e) = stream.write_all(line).await {
                    warn!("Failed to write to Snapcast server: {e}");
                    guard.stream = None;
                    guard.write_half = None;
                    continue;
                }
                if let Err(e) = stream.write_all(b"\n").await {
                    warn!("Failed to write newline to Snapcast server: {e}");
                    guard.stream = None;
                    guard.write_half = None;
                    continue;
                }
                if let Err(e) = stream.flush().await {
                    warn!("Failed to flush Snapcast server stream: {e}");
                    guard.stream = None;
                    guard.write_half = None;
                    continue;
                }
                return Ok(());
            }
            if let Some(write_half) = guard.write_half.as_mut() {
                if let Err(e) = write_half.write_all(line).await {
                    warn!("Failed to write to Snapcast server: {e}");
                    guard.write_half = None;
                    continue;
                }
                if let Err(e) = write_half.write_all(b"\n").await {
                    warn!("Failed to write newline to Snapcast server: {e}");
                    guard.write_half = None;
                    continue;
                }
                if let Err(e) = write_half.flush().await {
                    warn!("Failed to flush Snapcast server stream: {e}");
                    guard.write_half = None;
                    continue;
                }
                return Ok(());
            }
            drop(guard);
            warn!("Snapcast RPC not connected; waiting for reconnect");
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.inner.lock().await.reconnect_interval_ms,
            ))
            .await;
        }
    }
}

async fn run_connection_loop(
    inner: Arc<Mutex<ClientInner>>,
    notification_tx: mpsc::Sender<RpcNotification>,
) {
    loop {
        let (host, port, reconnect_interval_ms) = {
            let guard = inner.lock().await;
            (guard.host.clone(), guard.port, guard.reconnect_interval_ms)
        };

        info!("Connecting to Snapserver at {host}:{port}");
        match TcpStream::connect((host.as_str(), port)).await {
            Ok(stream) => {
                info!("Connected to Snapserver {host}:{port}");
                {
                    let mut guard = inner.lock().await;
                    guard.stream = Some(stream);
                }
                if let Err(e) = read_loop(inner.clone(), &notification_tx).await {
                    warn!("Snapcast read loop ended: {e}");
                }
                {
                    let mut guard = inner.lock().await;
                    guard.stream = None;
                    guard.write_half = None;
                    let pending = std::mem::take(&mut guard.pending);
                    for (_, tx) in pending {
                        let _ = tx.send(Err(RpcClientError::Cancelled));
                    }
                }
            }
            Err(e) => {
                warn!("Failed to connect to Snapserver {host}:{port}: {e}");
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(reconnect_interval_ms)).await;
    }
}

async fn read_loop(
    inner: Arc<Mutex<ClientInner>>,
    notification_tx: &mpsc::Sender<RpcNotification>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let read_half = {
        let mut guard = inner.lock().await;
        let stream = guard
            .stream
            .take()
            .ok_or_else(|| -> Box<dyn Error + Send + Sync> {
                Box::new(io::Error::new(io::ErrorKind::NotConnected, "no stream"))
            })?;
        let (read_half, write_half) = split(stream);
        guard.write_half = Some(write_half);
        read_half
    };
    let reader = BufReader::new(read_half);

    let mut lines = reader.lines();
    while let Some(line_result) = lines.next_line().await? {
        let line = line_result;
        if line.is_empty() {
            continue;
        }
        debug!("Snapcast RPC line: {line}");

        // Try to parse as a notification first (no id).
        let notification: Result<RpcNotification, _> = serde_json::from_str(&line);
        if let Ok(notification) = notification {
            if notification_tx.try_send(notification).is_err() {
                warn!("Snapcast notification channel full; dropping notification");
            }
            continue;
        }

        // Otherwise treat as a response.
        let response: RpcResponse<serde_json::Value> = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to parse Snapcast RPC line: {e}");
                continue;
            }
        };

        let id = response.id.unwrap_or(0);
        let mut guard = inner.lock().await;
        if let Some(tx) = guard.pending.remove(&id) {
            let result = if let Some(err) = response.error {
                Err(RpcClientError::JsonRpc {
                    code: err.code,
                    message: err.message,
                })
            } else {
                Ok(response.result.unwrap_or(serde_json::Value::Null))
            };
            let _ = tx.send(result);
        } else {
            warn!("Received unexpected Snapcast RPC response for id {id}");
        }
    }

    Ok(())
}
