//! Snapcast JSON-RPC data model.
//!
//! Types are intentionally minimal: only the fields needed to build the Matter
//! nodes are parsed. Everything else is ignored.
//!
//! Snapserver omits fields it has nothing to say about — a stream that has
//! never played carries no `metadata`, and property sets differ by stream
//! backend. Every struct here therefore derives `Default` and is marked
//! `#[serde(default)]`, so a missing field costs that one value rather than
//! failing the whole status parse and leaving the integration with no nodes.

use serde::Deserialize;
use serde::Serialize;

/// Result of `Server.GetStatus`.
///
/// The status is wrapped one level deeper than the other responses: the
/// result object holds a single `server` key, and the groups and streams are
/// inside that. Parsing straight into [`ServerStatus`] silently matches
/// nothing.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct GetStatusResult {
    pub server: ServerStatus,
}

/// The server state carried by `Server.GetStatus`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerStatus {
    pub groups: Vec<Group>,
    #[serde(rename = "server")]
    pub server_info: ServerInfo,
    pub streams: Vec<Stream>,
}

/// Server metadata returned by `Server.GetStatus`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerInfo {
    pub host: HostInfo,
    pub snapserver: SnapserverInfo,
}

/// Host metadata for a Snapserver or client.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct HostInfo {
    pub arch: String,
    pub ip: String,
    pub mac: String,
    pub name: String,
    pub os: String,
}

/// Snapserver software metadata.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SnapserverInfo {
    pub control_protocol_version: u32,
    pub name: String,
    pub protocol_version: u32,
    pub version: String,
}

/// A synchronized group of Snapcast clients.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub muted: bool,
    pub name: String,
    pub stream_id: String,
    pub clients: Vec<Client>,
}

/// A Snapcast client (player).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Client {
    pub id: String,
    pub connected: bool,
    pub host: HostInfo,
    pub config: ClientConfig,
    #[serde(rename = "lastSeen")]
    pub last_seen: LastSeen,
}

/// Client configuration, primarily volume.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ClientConfig {
    pub instance: u8,
    pub latency: i32,
    pub name: String,
    pub volume: Volume,
}

/// Client volume state. `percent` is 0-100.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Volume {
    pub muted: bool,
    pub percent: u8,
}

/// Last-seen timestamp.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct LastSeen {
    pub sec: i64,
    pub usec: i64,
}

/// A Snapcast audio stream.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Stream {
    pub id: String,
    pub status: String,
    pub uri: StreamUri,
    pub properties: StreamProperties,
}

/// Stream URI describing the source.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StreamUri {
    pub scheme: String,
    pub raw: String,
}

/// Stream properties, including playback metadata.
///
/// Snapserver spells these in camelCase; parsing them as snake_case leaves
/// every field `None` and the playback state stuck at its default.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StreamProperties {
    pub can_control: Option<bool>,
    pub can_play: Option<bool>,
    pub can_pause: Option<bool>,
    pub can_seek: Option<bool>,
    pub can_go_next: Option<bool>,
    pub can_go_previous: Option<bool>,
    pub loop_status: Option<String>,
    pub playback_status: Option<String>,
    pub position: Option<f64>,
    pub shuffle: Option<bool>,
    pub volume: Option<u8>,
    pub mute: Option<bool>,
    pub metadata: Option<StreamMetadata>,
}

/// Metadata for the currently playing track.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StreamMetadata {
    pub title: Option<String>,
    pub artist: Option<Vec<String>>,
    pub album: Option<String>,
    pub album_artist: Option<Vec<String>>,
    pub duration: Option<f64>,
    pub track_id: Option<String>,
}

/// JSON-RPC request envelope.
#[derive(Debug, Clone, Serialize)]
pub struct RpcRequest<T> {
    pub id: u64,
    pub jsonrpc: String,
    pub method: String,
    pub params: T,
}

impl<T> RpcRequest<T> {
    pub fn new(id: u64, method: String, params: T) -> Self {
        Self {
            id,
            jsonrpc: "2.0".to_string(),
            method,
            params,
        }
    }
}

/// JSON-RPC response envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcResponse<R> {
    pub id: Option<u64>,
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub result: Option<R>,
    pub error: Option<RpcError>,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// JSON-RPC notification envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcNotification {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub method: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Parameters for `Client.SetVolume`.
#[derive(Debug, Clone, Serialize)]
pub struct ClientSetVolumeParams {
    pub id: String,
    pub volume: Volume,
}

/// Parameters for `Group.SetMute`.
#[derive(Debug, Clone, Serialize)]
pub struct GroupSetMuteParams {
    pub id: String,
    pub mute: bool,
}

/// Parameters for `Group.SetStream`.
#[derive(Debug, Clone, Serialize)]
pub struct GroupSetStreamParams {
    pub id: String,
    pub stream_id: String,
}

/// Parameters for `Stream.Control`.
#[derive(Debug, Clone, Serialize)]
pub struct StreamControlParams {
    pub id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}
