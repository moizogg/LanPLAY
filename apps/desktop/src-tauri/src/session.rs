//! Session state: Phase 2 real UDP controller path (stub stream still).

use lanplay_controllers::{
    poll_xinput, probe_vigem, run_client_input_loop, run_host_input_loop, ClientInputHandle,
    HostInputConfig, HostInputHandle,
};
use lanplay_networking::default_ports;
use lanplay_shared::{ClientStatus, ControllerStats, HostStatus, SessionState};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct SessionManager {
    inner: Arc<Mutex<SessionInner>>,
}

struct SessionInner {
    host: HostStatus,
    client: ClientStatus,
    host_input: Option<HostInputHandle>,
    client_input: Option<ClientInputHandle>,
    allow_remote_input: bool,
}

impl Default for SessionInner {
    fn default() -> Self {
        let (control_port, media_port) = default_ports();
        let vigem = probe_vigem();
        Self {
            host: HostStatus {
                state: SessionState::Idle,
                control_port,
                media_port,
                allow_remote_input: true,
                message: "Host idle. Start Host to wait for a client (no virtual pad yet).".into(),
                vigem_ok: vigem.available,
                packets_received: 0,
                input_latency_ms: 0.0,
                last_seq: 0,
                virtual_pad_active: false,
            },
            client: ClientStatus {
                state: SessionState::Idle,
                host_ip: None,
                control_port,
                media_port,
                message: "Enter host Tailscale IP. Your keyboard/mouse + controller go to the host."
                    .into(),
                local_pad_connected: false,
                packets_sent: 0,
                last_seq: 0,
            },
            host_input: None,
            client_input: None,
            allow_remote_input: true,
        }
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn host_status(&self) -> HostStatus {
        let mut inner = self.inner.lock();
        Self::refresh_host_metrics(&mut inner);
        inner.host.clone()
    }

    pub fn client_status(&self) -> ClientStatus {
        let mut inner = self.inner.lock();
        Self::refresh_client_metrics(&mut inner);
        inner.client.clone()
    }

    pub fn controller_stats(&self) -> ControllerStats {
        let mut inner = self.inner.lock();
        if inner.host_input.is_some() {
            Self::refresh_host_metrics(&mut inner);
            ControllerStats {
                role: "host".into(),
                packets: inner.host.packets_received,
                last_seq: inner.host.last_seq,
                input_latency_ms: inner.host.input_latency_ms,
                pad_connected: inner.host.virtual_pad_active,
                vigem_ok: inner.host.vigem_ok,
                detail: inner.host.message.clone(),
            }
        } else if inner.client_input.is_some() {
            Self::refresh_client_metrics(&mut inner);
            ControllerStats {
                role: "client".into(),
                packets: inner.client.packets_sent,
                last_seq: inner.client.last_seq,
                input_latency_ms: 0.0,
                pad_connected: inner.client.local_pad_connected,
                vigem_ok: false,
                detail: inner.client.message.clone(),
            }
        } else {
            // Do not call probe_vigem() on every UI poll — it is relatively heavy.
            // Host start / get_vigem_bundle_status handle real probes.
            ControllerStats {
                role: "idle".into(),
                packets: 0,
                last_seq: 0,
                input_latency_ms: 0.0,
                pad_connected: poll_xinput(0).connected,
                vigem_ok: inner.host.vigem_ok,
                detail: "Idle — start Host or Connect as Client.".into(),
            }
        }
    }

    pub fn start_host(&self) -> Result<HostStatus, String> {
        let mut inner = self.inner.lock();
        if inner.client_input.is_some()
            || inner.client.state == SessionState::Connecting
            || inner.client.state == SessionState::Streaming
        {
            return Err("Stop the client session before starting host mode.".into());
        }
        if inner.host_input.is_some() {
            return Err("Host is already listening.".into());
        }

        let media = inner.host.media_port;
        let allow = inner.allow_remote_input;

        let handle = run_host_input_loop(HostInputConfig {
            media_port: media,
            allow_remote_input: allow,
        })
        .map_err(|e| e.to_string())?;

        let vigem_ok = handle.vigem_ok();
        let vigem_detail = handle.vigem_detail().to_string();

        inner.host.vigem_ok = vigem_ok;
        inner.host.state = SessionState::Listening;
        // Important: no virtual pad until a client controller connects
        inner.host.virtual_pad_active = false;
        inner.host.message = if vigem_ok {
            format!(
                "Listening on :{media}. No virtual pad yet — appears when client plugs a controller. KBM works when client connects. {vigem_detail}"
            )
        } else {
            format!(
                "Listening on :{media}. Install gamepad support so client controllers can appear. {vigem_detail}"
            )
        };

        inner.host_input = Some(handle);
        Ok(inner.host.clone())
    }

    pub fn stop_host(&self) -> Result<HostStatus, String> {
        let mut inner = self.inner.lock();
        if let Some(h) = inner.host_input.take() {
            h.stop();
        }
        inner.host.state = SessionState::Idle;
        inner.host.packets_received = 0;
        inner.host.input_latency_ms = 0.0;
        inner.host.last_seq = 0;
        inner.host.virtual_pad_active = false;
        inner.host.message = "Host stopped. Any virtual pad was removed.".into();
        Ok(inner.host.clone())
    }

    pub fn set_allow_remote_input(&self, allow: bool) -> HostStatus {
        let mut inner = self.inner.lock();
        inner.allow_remote_input = allow;
        inner.host.allow_remote_input = allow;
        // Restart host loop if running so the flag takes effect.
        if inner.host_input.is_some() {
            if let Some(h) = inner.host_input.take() {
                h.stop();
            }
            let media = inner.host.media_port;
            match run_host_input_loop(HostInputConfig {
                media_port: media,
                allow_remote_input: allow,
            }) {
                Ok(handle) => {
                    inner.host.vigem_ok = handle.vigem_ok();
                    inner.host_input = Some(handle);
                    inner.host.message = if allow {
                        "Remote input allowed (host loop restarted).".into()
                    } else {
                        "Remote input blocked — view-only for controllers.".into()
                    };
                }
                Err(e) => {
                    inner.host.state = SessionState::Error;
                    inner.host.message = format!("Failed to re-apply input setting: {e}");
                }
            }
        } else if allow {
            inner.host.message = "Remote input allowed.".into();
        } else {
            inner.host.message = "Remote input blocked (view-only when streaming).".into();
        }
        inner.host.clone()
    }

    pub fn connect_client(
        &self,
        host_ip: String,
        control_port: u16,
        media_port: u16,
    ) -> Result<ClientStatus, String> {
        let mut inner = self.inner.lock();
        if inner.host_input.is_some()
            || inner.host.state == SessionState::Listening
            || inner.host.state == SessionState::Streaming
        {
            return Err("Stop the host session before connecting as client.".into());
        }
        if inner.client_input.is_some() {
            return Err("Already connected.".into());
        }

        let ip = host_ip.trim().to_string();
        if ip.is_empty() {
            return Err("Host IP is required.".into());
        }

        inner.client.state = SessionState::Connecting;
        inner.client.host_ip = Some(ip.clone());
        inner.client.control_port = control_port;
        inner.client.media_port = media_port;
        inner.client.message = format!("Starting controller sender to {ip}:{media_port}…");

        let handle = run_client_input_loop(ip.clone(), media_port, 250).map_err(|e| {
            inner.client.state = SessionState::Error;
            inner.client.message = e.to_string();
            e.to_string()
        })?;

        inner.client_input = Some(handle);
        inner.client.state = SessionState::Streaming;
        let pad = poll_xinput(0).connected;
        inner.client.local_pad_connected = pad;
        inner.client.message = if pad {
            format!(
                "Connected to {ip}:{media_port}. Sending keyboard/mouse + your controller → host (virtual pad on host)."
            )
        } else {
            format!(
                "Connected to {ip}:{media_port}. Sending keyboard/mouse. Plug a controller to create a virtual pad on host."
            )
        };
        Ok(inner.client.clone())
    }

    pub fn disconnect_client(&self) -> Result<ClientStatus, String> {
        let mut inner = self.inner.lock();
        if let Some(c) = inner.client_input.take() {
            c.stop();
        }
        inner.client.state = SessionState::Idle;
        inner.client.packets_sent = 0;
        inner.client.last_seq = 0;
        inner.client.message = "Disconnected. Controller send stopped.".into();
        Ok(inner.client.clone())
    }

    fn refresh_host_metrics(inner: &mut SessionInner) {
        if let Some(ref h) = inner.host_input {
            let s = h.stats();
            inner.host.packets_received = s.packets();
            inner.host.last_seq = s.last_seq();
            inner.host.input_latency_ms = s.latency_ms();
            // Virtual pad only when host actually plugged one for the remote client
            inner.host.virtual_pad_active = h.virtual_pad_active();
            let detail = s.detail();
            if !detail.is_empty() && inner.host.state == SessionState::Listening {
                if s.packets() > 0 {
                    inner.host.message = format!(
                        "{} — pkts {} · ~{:.1} ms · virtual_pad={}",
                        detail,
                        s.packets(),
                        s.latency_ms(),
                        if h.virtual_pad_active() { "ON" } else { "off" }
                    );
                } else if !detail.is_empty() {
                    inner.host.message = detail;
                }
            }
        }
    }

    fn refresh_client_metrics(inner: &mut SessionInner) {
        if let Some(ref c) = inner.client_input {
            let s = c.stats();
            inner.client.packets_sent = s.packets();
            inner.client.last_seq = s.last_seq();
            inner.client.local_pad_connected = s.pad_connected();
            if s.packets() > 0 {
                inner.client.message = format!(
                    "Sending — pkts {} · seq {} · pad {}",
                    s.packets(),
                    s.last_seq(),
                    if s.pad_connected() {
                        "connected"
                    } else {
                        "missing"
                    }
                );
            }
        } else {
            inner.client.local_pad_connected = poll_xinput(0).connected;
        }
    }
}
