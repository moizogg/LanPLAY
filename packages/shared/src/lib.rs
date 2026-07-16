//! Shared types, errors, and constants used across LANPlay packages.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default control-plane TCP port (join handshake / accept-reject).
pub const DEFAULT_CONTROL_PORT: u16 = 47800;

/// Default media/input UDP port (controller + KBM after accept).
pub const DEFAULT_MEDIA_PORT: u16 = 47801;

/// App role selected in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    Host,
    Client,
}

/// High-level session state for the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Listening,
    /// Client waiting for host Accept/Reject
    WaitingApproval,
    Connecting,
    Streaming,
    Error,
}

/// Pending client join shown on Host UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJoinInfo {
    pub peer_ip: String,
    pub client_name: String,
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
    pub vigem_ok: bool,
    pub packets_received: u64,
    pub input_latency_ms: f32,
    pub last_seq: u32,
    pub virtual_pad_active: bool,
    /// Someone is waiting for Accept / Reject.
    pub pending_join: Option<PendingJoinInfo>,
    /// Host accepted a client (UDP input allowed from them).
    pub session_active: bool,
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
    pub local_pad_connected: bool,
    pub packets_sent: u64,
    pub last_seq: u32,
}

/// Live controller path metrics (host + client summary).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerStats {
    pub role: String,
    pub packets: u64,
    pub last_seq: u32,
    pub input_latency_ms: f32,
    pub pad_connected: bool,
    pub vigem_ok: bool,
    pub detail: String,
}

/// Tailscale discovery result for the host UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleInfo {
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
