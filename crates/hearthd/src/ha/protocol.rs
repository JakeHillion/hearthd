//! Protocol definitions for Home Assistant integration communication.
//!
//! Defines message types exchanged between Rust and Python over Unix sockets.
//!
//! Design principles:
//! - Rust-heavy: Most logic lives in Rust, Python is a thin wrapper
//! - Async lifecycle: Rust manages integration lifecycle
//! - State flows to Rust: Python sends state updates, Rust persists
//! - JSON over Unix socket: Newline-delimited JSON messages
//! - Request-response: Some operations need correlation via IDs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Messages sent from Python to Rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Python sandbox initialized and ready
    Ready,

    /// Register a new entity
    EntityRegister {
        entry_id: String,
        entity_id: String,
        platform: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        device_class: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        capabilities: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        device_info: Option<DeviceInfo>,
    },

    /// Update entity state
    StateUpdate {
        entity_id: String,
        state: String,
        attributes: serde_json::Value,
        last_updated: String, // ISO 8601 timestamp
    },

    /// Request to make an HTTP call (Rust proxies for security)
    HttpRequest {
        request_id: String,
        method: HttpMethod,
        url: String,
        headers: HashMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<Vec<u8>>,
        timeout_ms: u64,
    },

    /// Log message from Python
    Log {
        level: LogLevel,
        logger: String,
        message: String,
    },

    /// Schedule a periodic update timer
    ScheduleUpdate {
        timer_id: String,
        entry_id: String,
        interval_seconds: u64,
    },

    /// Cancel a scheduled timer
    CancelTimer { timer_id: String },

    /// Request configuration values
    GetConfig {
        request_id: String,
        keys: Vec<String>,
    },

    /// Integration setup completed successfully
    SetupComplete {
        entry_id: String,
        platforms: Vec<String>,
    },

    /// Integration setup failed
    SetupFailed {
        entry_id: String,
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        missing_package: Option<String>,
    },

    /// Integration unload completed
    UnloadComplete { entry_id: String },

    /// Coordinator update completed
    UpdateComplete {
        timer_id: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub identifiers: Vec<Vec<String>>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sw_version: Option<String>,
}

/// Responses sent from Rust to Python
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Acknowledge message received (optional)
    Ack {
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },

    /// Request to set up an integration
    SetupIntegration {
        domain: String,
        entry_id: String,
        config: serde_json::Value,
    },

    /// Request to unload an integration
    UnloadIntegration { entry_id: String },

    /// Timer fired, trigger coordinator update
    TriggerUpdate { timer_id: String, entry_id: String },

    /// HTTP request result
    #[allow(clippy::enum_variant_names)] // Response suffix is appropriate here
    HttpResponse {
        request_id: String,
        status: u16,
        headers: HashMap<String, String>,
        body: Vec<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// Configuration query result
    #[allow(clippy::enum_variant_names)] // Response suffix is appropriate here
    ConfigResponse {
        request_id: String,
        config: HashMap<String, serde_json::Value>,
    },

    /// Graceful shutdown signal
    Shutdown,

    /// Error response
    Error { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid message type: {0}")]
    #[allow(dead_code)] // WIP: May be used for protocol validation
    InvalidMessageType(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[allow(dead_code)] // WIP: Will be used for protocol operations
pub type Result<T> = std::result::Result<T, ProtocolError>;
