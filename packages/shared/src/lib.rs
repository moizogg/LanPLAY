//! Shared types, errors, and constants used across LANPlay packages.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default control-plane TCP port (session handshake).
pub const DEFAULT_CONTROL_PORT: u16 = 47800;

/// Default media/input UDP port.
pub const DEFAULT_MEDIA_PORT: u16 = 47801;

/// App role selected in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    Host,
    Client,
}

/// High-level session state for the shell (Phase 1 stubs only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Listening,
    Connecting,
    Streaming,
    Error,
}

/// Host listen status returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub state: SessionState,
    pub control_port: u16,
    pub media_port: u16,
    pub allow_remote_input: bool,
    pub message: String,
}

/// Client connection status returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientStatus {
    pub state: SessionState,
    pub host_ip: Option<String>,
    pub control_port: u16,
    pub media_port: u16,
    pub message: String,
}

/// Tailscale discovery result for the host UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleInfo {
    /// Best-effort Tailscale IPv4 (e.g. 100.x.y.z), if found.
    pub ip: Option<String>,
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Error)]
pub enum LanPlayError {
    #[error("{0}")]
    Message(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LanPlayError>;
