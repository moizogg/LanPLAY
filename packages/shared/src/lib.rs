//! Shared types, errors, and constants used across LANPlay packages.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default control-plane TCP port (session handshake — Phase 3+).
pub const DEFAULT_CONTROL_PORT: u16 = 47800;

/// Default media/input UDP port (Phase 2 controller packets use this).
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
    /// True when ViGEm bus is available on this machine.
    pub vigem_ok: bool,
    /// Packets received on the input UDP port.
    pub packets_received: u64,
    /// Estimated one-way controller latency (host_now - client_ts), ms.
    pub input_latency_ms: f32,
    /// Last remote sequence number applied.
    pub last_seq: u32,
    /// Remote pad currently plugged into ViGEm.
    pub virtual_pad_active: bool,
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
    /// Local XInput pad detected for user index 0.
    pub local_pad_connected: bool,
    /// Packets sent to host.
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
