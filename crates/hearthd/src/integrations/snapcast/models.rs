//! Snapcast JSON-RPC data model.
//!
//! Types are intentionally minimal: only the fields needed to build the Matter
//! nodes are parsed. Everything else is ignored.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// Top-level response wrapper for `Server.GetStatus`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ServerStatus {
    pub groups: Vec<Group>,
    #[serde(rename = "server")]
    pub server_info: ServerInfo,
    pub streams: Vec<Stream>,
}

/// Server metadata returned by `Server.GetStatus`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ServerInfo {
    pub host: HostInfo,
    pub snapserver: SnapserverInfo,
}

/// Host metadata for the Snapserver.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HostInfo {
    pub arch: String,
    pub ip: String,
    pub mac: String,
    pub name: String,
    pub os: String,
}

/// Snapserver software metadata.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SnapserverInfo {
    pub control_protocol_version: u32,
    pub name: String,
    pub protocol_version: u32,
    pub version: String,
}

/// A synchronized group of Snapcast clients.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Group {
    pub id: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "stream_id")]
    pub stream_id: String,
    pub clients: Vec<Client>,
}

/// A Snapcast client (player).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Client {
    pub id: String,
    pub connected: bool,
    pub host: HostInfo,
    pub config: ClientConfig,
    #[serde(default)]
    pub last_seen: LastSeen,
}

/// Client configuration, primarily volume.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClientConfig {
    pub instance: u8,
    pub latency: i32,
    #[serde(default)]
    pub name: String,
    pub volume: Volume,
}

/// Client volume state.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Volume {
    pub muted: bool,
    pub percent: u8,
}

/// Last-seen timestamp.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LastSeen {
    pub sec: i64,
    pub usec: i64,
}

/// A Snapcast audio stream.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Stream {
    pub id: String,
    #[serde(default)]
    pub status: String,
    pub uri: StreamUri,
    #[serde(default)]
    pub properties: StreamProperties,
}

/// Stream URI describing the source.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StreamUri {
    pub scheme: String,
    pub raw: String,
}

/// Stream properties, including playback metadata.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StreamProperties {
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
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
    pub params: serde_json::Value,
}

/// Parameters for `Client.SetVolume`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSetVolumeParams {
    pub id: String,
    pub volume: Volume,
}

/// Parameters for `Group.SetMute`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSetMuteParams {
    pub id: String,
    pub mute: bool,
}

/// Parameters for `Group.SetStream`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSetStreamParams {
    pub id: String,
    #[serde(rename = "stream_id")]
    pub stream_id: String,
}

/// Parameters for `Stream.Control`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamControlParams {
    pub id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}
