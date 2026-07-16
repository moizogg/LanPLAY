//! In-process session state for the Phase 1 shell (stub transport only).

use lanplay_networking::{default_ports, NetworkTransport, StubTransport};
use lanplay_shared::{ClientStatus, HostStatus, SessionState};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct SessionManager {
    inner: Arc<Mutex<SessionInner>>,
}

struct SessionInner {
    transport: StubTransport,
    host: HostStatus,
    client: ClientStatus,
}

impl Default for SessionInner {
    fn default() -> Self {
        let (control_port, media_port) = default_ports();
        Self {
            transport: StubTransport::default(),
            host: HostStatus {
                state: SessionState::Idle,
                control_port,
                media_port,
                allow_remote_input: true,
                message: "Host is idle. Click Start Host to listen.".into(),
            },
            client: ClientStatus {
                state: SessionState::Idle,
                host_ip: None,
                control_port,
                media_port,
                message: "Enter the host Tailscale IP and connect.".into(),
            },
        }
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn host_status(&self) -> HostStatus {
        self.inner.lock().host.clone()
    }

    pub fn client_status(&self) -> ClientStatus {
        self.inner.lock().client.clone()
    }

    pub fn start_host(&self) -> Result<HostStatus, String> {
        let mut inner = self.inner.lock();
        if inner.client.state == SessionState::Connecting
            || inner.client.state == SessionState::Streaming
        {
            return Err("Stop the client session before starting host mode.".into());
        }

        let control = inner.host.control_port;
        let media = inner.host.media_port;
        inner
            .transport
            .listen(control, media)
            .map_err(|e| e.to_string())?;

        inner.host.state = SessionState::Listening;
        inner.host.message = format!(
            "Listening on control :{} / media :{} (Phase 1 stub — no real sockets yet).",
            control, media
        );
        Ok(inner.host.clone())
    }

    pub fn stop_host(&self) -> Result<HostStatus, String> {
        let mut inner = self.inner.lock();
        inner.transport.close().map_err(|e| e.to_string())?;
        inner.host.state = SessionState::Idle;
        inner.host.message = "Host stopped.".into();
        Ok(inner.host.clone())
    }

    pub fn set_allow_remote_input(&self, allow: bool) -> HostStatus {
        let mut inner = self.inner.lock();
        inner.host.allow_remote_input = allow;
        if allow {
            inner.host.message = "Remote input allowed.".into();
        } else {
            inner.host.message = "Remote input blocked (view-only when streaming).".into();
        }
        inner.host.clone()
    }

    pub fn connect_client(&self, host_ip: String, control_port: u16, media_port: u16) -> Result<ClientStatus, String> {
        let mut inner = self.inner.lock();
        if inner.host.state == SessionState::Listening || inner.host.state == SessionState::Streaming {
            return Err("Stop the host session before connecting as client.".into());
        }

        let ip = host_ip.trim().to_string();
        if ip.is_empty() {
            return Err("Host IP is required.".into());
        }

        inner.client.state = SessionState::Connecting;
        inner.client.host_ip = Some(ip.clone());
        inner.client.control_port = control_port;
        inner.client.media_port = media_port;
        inner.client.message = format!("Connecting to {}…", ip);

        match inner.transport.connect(&ip, control_port, media_port) {
            Ok(()) => {
                // Phase 1: stub succeeds immediately (no real network).
                inner.client.state = SessionState::Streaming;
                inner.client.message = format!(
                    "Connected to {} (Phase 1 stub — no real stream yet).",
                    ip
                );
                Ok(inner.client.clone())
            }
            Err(e) => {
                inner.client.state = SessionState::Error;
                inner.client.message = e.to_string();
                Err(e.to_string())
            }
        }
    }

    pub fn disconnect_client(&self) -> Result<ClientStatus, String> {
        let mut inner = self.inner.lock();
        inner.transport.close().map_err(|e| e.to_string())?;
        inner.client.state = SessionState::Idle;
        inner.client.message = "Disconnected.".into();
        Ok(inner.client.clone())
    }
}
