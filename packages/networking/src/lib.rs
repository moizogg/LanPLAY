//! Networking abstraction for LANPlay.
//!
//! V1: direct connect to a host Tailscale (or LAN) IP.
//! Later: room codes / ICE / QUIC swap in behind the same traits.

use lanplay_shared::{DEFAULT_CONTROL_PORT, DEFAULT_MEDIA_PORT};

/// Placeholder transport trait. Real async sockets arrive in Phase 3.
pub trait NetworkTransport {
    /// Host: begin accepting a client.
    fn listen(&mut self, control_port: u16, media_port: u16) -> lanplay_shared::Result<()>;

    /// Client: connect to `host_ip`.
    fn connect(
        &mut self,
        host_ip: &str,
        control_port: u16,
        media_port: u16,
    ) -> lanplay_shared::Result<()>;

    fn close(&mut self) -> lanplay_shared::Result<()>;
}

/// Stub transport used by the Phase 1 shell (no real sockets yet).
#[derive(Debug, Default)]
pub struct StubTransport {
    pub listening: bool,
    pub connected: bool,
    pub peer: Option<String>,
}

impl NetworkTransport for StubTransport {
    fn listen(&mut self, _control_port: u16, _media_port: u16) -> lanplay_shared::Result<()> {
        self.listening = true;
        self.connected = false;
        self.peer = None;
        Ok(())
    }

    fn connect(
        &mut self,
        host_ip: &str,
        _control_port: u16,
        _media_port: u16,
    ) -> lanplay_shared::Result<()> {
        if host_ip.trim().is_empty() {
            return Err(lanplay_shared::LanPlayError::Message(
                "Host IP is required".into(),
            ));
        }
        self.connected = true;
        self.listening = false;
        self.peer = Some(host_ip.trim().to_string());
        Ok(())
    }

    fn close(&mut self) -> lanplay_shared::Result<()> {
        self.listening = false;
        self.connected = false;
        self.peer = None;
        Ok(())
    }
}

/// Default ports helper for UI / config.
pub fn default_ports() -> (u16, u16) {
    (DEFAULT_CONTROL_PORT, DEFAULT_MEDIA_PORT)
}
