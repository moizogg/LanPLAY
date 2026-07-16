//! Session: join accept/reject + UDP input + video stream (Phase 6).

use crate::settings_store;
use lanplay_controllers::{
    poll_xinput, probe_vigem, run_client_input_loop, run_host_input_loop, CaptureStatus,
    ClientInputHandle, HostInputConfig, HostInputHandle,
};
use lanplay_networking::{
    client_request_join, default_ports, local_client_name, run_host_join_listener,
    ClientControlSession, HostJoinHandle, JoinDecision,
};
use lanplay_shared::{
    video_port_from_media, ClientStatus, ControllerStats, HostStatus, PendingJoinInfo,
    SessionState,
};
use lanplay_video::{
    run_client_video_loop, run_host_capture_loop, CaptureSnapshot, ClientVideoHandle,
    ClientVideoSnapshot, HostCaptureHandle, VideoSenderHandle, VideoSettings, VideoStreamSink,
};
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Default)]
pub struct SessionManager {
    inner: Arc<Mutex<SessionInner>>,
}

struct SessionInner {
    host: HostStatus,
    client: ClientStatus,
    host_input: Option<HostInputHandle>,
    host_join: Option<HostJoinHandle>,
    host_capture: Option<HostCaptureHandle>,
    host_video_sink: Option<VideoStreamSink>,
    host_video_sender: Option<VideoSenderHandle>,
    client_input: Option<ClientInputHandle>,
    client_control: Option<ClientControlSession>,
    client_video: Option<ClientVideoHandle>,
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
                message: "Host idle. Start Host, then Accept join requests from clients.".into(),
                vigem_ok: vigem.available,
                packets_received: 0,
                input_latency_ms: 0.0,
                last_seq: 0,
                virtual_pad_active: false,
                pending_join: None,
                session_active: false,
            },
            client: ClientStatus {
                state: SessionState::Idle,
                host_ip: None,
                control_port,
                media_port,
                message: "Enter host Tailscale IP. Host must Accept before you can play.".into(),
                local_pad_connected: false,
                packets_sent: 0,
                last_seq: 0,
            },
            host_input: None,
            host_join: None,
            host_capture: None,
            host_video_sink: None,
            host_video_sender: None,
            client_input: None,
            client_control: None,
            client_video: None,
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
        } else if inner.client_input.is_some()
            || matches!(
                inner.client.state,
                SessionState::WaitingApproval | SessionState::Connecting | SessionState::Streaming
            )
        {
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
            ControllerStats {
                role: "idle".into(),
                packets: 0,
                last_seq: 0,
                input_latency_ms: 0.0,
                pad_connected: poll_xinput(0).connected,
                vigem_ok: inner.host.vigem_ok,
                detail: "Idle — Start Host or Connect as Client.".into(),
            }
        }
    }

    pub fn start_host(&self) -> Result<HostStatus, String> {
        let mut inner = self.inner.lock();
        if inner.client_input.is_some()
            || matches!(
                inner.client.state,
                SessionState::Connecting
                    | SessionState::WaitingApproval
                    | SessionState::Streaming
            )
        {
            return Err("Stop the client session before starting host mode.".into());
        }
        if inner.host_input.is_some() || inner.host_join.is_some() {
            return Err("Host is already listening.".into());
        }

        let control = inner.host.control_port;
        let media = inner.host.media_port;
        let allow = inner.allow_remote_input;

        let join_handle = run_host_join_listener(control).map_err(|e| e.to_string())?;
        let allowed_peer = join_handle.allowed_peer();

        let input_handle = match run_host_input_loop(HostInputConfig {
            media_port: media,
            allow_remote_input: allow,
            allowed_peer,
        }) {
            Ok(h) => h,
            Err(e) => {
                join_handle.stop();
                return Err(e.to_string());
            }
        };

        // Phase 5–6: capture + encode + video sink (streams after Accept)
        let video = settings_store::get();
        let capture_cfg = video.to_capture_config();
        let video_sink = VideoStreamSink::new();
        let video_sender = video_sink.clone().start_sender().ok();
        let capture_handle =
            run_host_capture_loop(capture_cfg, Some(video_sink.clone())).ok();

        let vigem_ok = input_handle.vigem_ok();
        let vigem_detail = input_handle.vigem_detail().to_string();
        let vport = video_port_from_media(media);

        inner.host.vigem_ok = vigem_ok;
        inner.host.state = SessionState::Listening;
        inner.host.virtual_pad_active = false;
        inner.host.pending_join = None;
        inner.host.session_active = false;
        let res_label = match video.resolution_mode {
            lanplay_video::ResolutionMode::Auto => format!("auto≤{}", video.max_edge),
            lanplay_video::ResolutionMode::Fixed => format!("{}x{}", video.width, video.height),
        };
        inner.host.message = format!(
            "Listening :{control}/input :{media}/video :{vport}. Encode {res_label} @ {}fps / {}kbps. Accept joins. {vigem_detail}",
            video.fps, video.bitrate_kbps
        );

        inner.host_join = Some(join_handle);
        inner.host_input = Some(input_handle);
        inner.host_capture = capture_handle;
        inner.host_video_sink = Some(video_sink);
        inner.host_video_sender = video_sender;
        Ok(inner.host.clone())
    }

    pub fn stop_host(&self) -> Result<HostStatus, String> {
        let mut inner = self.inner.lock();
        // Closing join handle shuts down accepted TCP → client detects host gone
        if let Some(j) = inner.host_join.take() {
            j.stop();
        }
        if let Some(h) = inner.host_input.take() {
            h.stop();
        }
        if let Some(c) = inner.host_capture.take() {
            c.stop();
        }
        if let Some(s) = inner.host_video_sender.take() {
            s.stop();
        }
        if let Some(sink) = inner.host_video_sink.take() {
            sink.set_peer(None);
        }
        inner.host.state = SessionState::Idle;
        inner.host.packets_received = 0;
        inner.host.input_latency_ms = 0.0;
        inner.host.last_seq = 0;
        inner.host.virtual_pad_active = false;
        inner.host.pending_join = None;
        inner.host.session_active = false;
        inner.host.message = "Host stopped. Clients will disconnect.".into();
        Ok(inner.host.clone())
    }

    pub fn respond_to_join(&self, accept: bool) -> Result<HostStatus, String> {
        let mut inner = self.inner.lock();
        let decision = if accept {
            JoinDecision::Accept
        } else {
            JoinDecision::Reject
        };
        // Scope join borrow so we can mutate `inner` afterward (E0502).
        let (msg, peer_ip) = {
            let join = inner
                .host_join
                .as_ref()
                .ok_or_else(|| "Host is not listening.".to_string())?;
            let msg = join.decide(decision)?;
            let peer_ip = if accept {
                join.allowed_peer().lock().clone()
            } else {
                None
            };
            (msg, peer_ip)
        };

        inner.host.pending_join = None;
        if accept {
            inner.host.session_active = true;
            inner.host.state = SessionState::Streaming;
            // Point video UDP at accepted peer (media_port + 1).
            if let Some(ip) = peer_ip {
                let vport = video_port_from_media(inner.host.media_port);
                let addr = SocketAddr::new(ip, vport);
                if let Some(ref sink) = inner.host_video_sink {
                    sink.set_peer(Some(addr));
                }
                inner.host.message = format!(
                    "{msg}. Streaming video → {addr}. Client KBM/pad live; pad plugs when controller is stable."
                );
            } else {
                inner.host.message = format!("{msg}. Session active (video peer unresolved).");
            }
        } else {
            inner.host.session_active = false;
            inner.host.state = SessionState::Listening;
            if let Some(ref sink) = inner.host_video_sink {
                sink.set_peer(None);
            }
            inner.host.message = format!("{msg}. Still listening for another join.");
        }
        Ok(inner.host.clone())
    }

    pub fn set_allow_remote_input(&self, allow: bool) -> HostStatus {
        let mut inner = self.inner.lock();
        inner.allow_remote_input = allow;
        inner.host.allow_remote_input = allow;
        if inner.host_input.is_some() && inner.host_join.is_some() {
            if let Some(h) = inner.host_input.take() {
                h.stop();
            }
            let media = inner.host.media_port;
            let allowed = inner.host_join.as_ref().unwrap().allowed_peer();
            match run_host_input_loop(HostInputConfig {
                media_port: media,
                allow_remote_input: allow,
                allowed_peer: allowed,
            }) {
                Ok(handle) => {
                    inner.host.vigem_ok = handle.vigem_ok();
                    inner.host_input = Some(handle);
                    inner.host.message = if allow {
                        "Remote input allowed.".into()
                    } else {
                        "Remote input blocked (view-only).".into()
                    };
                }
                Err(e) => {
                    inner.host.message = format!("Failed to update input setting: {e}");
                }
            }
        } else {
            inner.host.message = if allow {
                "Remote input allowed.".into()
            } else {
                "Remote input blocked.".into()
            };
        }
        inner.host.clone()
    }

    pub fn connect_client(
        &self,
        host_ip: String,
        control_port: u16,
        media_port: u16,
    ) -> Result<ClientStatus, String> {
        let ip = host_ip.trim().to_string();
        if ip.is_empty() {
            return Err("Host IP is required.".into());
        }

        {
            let mut inner = self.inner.lock();
            if inner.host_input.is_some()
                || matches!(
                    inner.host.state,
                    SessionState::Listening | SessionState::Streaming
                )
            {
                return Err("Stop the host session before connecting as client.".into());
            }
            if inner.client_input.is_some()
                || inner.client_control.is_some()
                || matches!(
                    inner.client.state,
                    SessionState::Connecting
                        | SessionState::WaitingApproval
                        | SessionState::Streaming
                )
            {
                return Err("Already connecting or connected.".into());
            }

            inner.client.state = SessionState::WaitingApproval;
            inner.client.host_ip = Some(ip.clone());
            inner.client.control_port = control_port;
            inner.client.media_port = media_port;
            // Bind video port early so we don't miss the post-Accept IDR.
            let vport = video_port_from_media(media_port);
            if let Some(old) = inner.client_video.take() {
                old.stop();
            }
            match run_client_video_loop(vport) {
                Ok(vh) => {
                    inner.client_video = Some(vh);
                    inner.client.message = format!(
                        "Requesting to join {ip}… video listen :{vport}. Waiting for Accept."
                    );
                }
                Err(e) => {
                    inner.client.message = format!(
                        "Requesting to join {ip}… (video bind failed: {e}). Waiting for Accept."
                    );
                }
            }
        }

        let mgr = self.inner.clone();
        let name = local_client_name();
        let ip_bg = ip.clone();
        std::thread::Builder::new()
            .name("lanplay-client-join".into())
            .spawn(move || {
                let join_result =
                    client_request_join(&ip_bg, control_port, &name, Duration::from_secs(120));
                let mut inner = mgr.lock();
                if !matches!(inner.client.state, SessionState::WaitingApproval) {
                    if let Ok(ctrl) = join_result {
                        ctrl.stop();
                    }
                    if let Some(v) = inner.client_video.take() {
                        v.stop();
                    }
                    return;
                }
                match join_result {
                    Ok(ctrl) => {
                        let alive = ctrl.alive_flag();
                        let vport = video_port_from_media(media_port);
                        match run_client_input_loop(ip_bg.clone(), media_port, 250, alive) {
                            Ok(handle) => {
                                inner.client_control = Some(ctrl);
                                inner.client_input = Some(handle);
                                inner.client.state = SessionState::Streaming;
                                let pad = poll_xinput(0).connected;
                                inner.client.local_pad_connected = pad;
                                let video_ok = inner.client_video.is_some();
                                inner.client.message = if video_ok {
                                    format!(
                                        "Host accepted! Video :{vport} + input → {ip_bg}."
                                    )
                                } else {
                                    format!(
                                        "Host accepted (input only). Video port :{vport} failed earlier."
                                    )
                                };
                            }
                            Err(e) => {
                                if let Some(v) = inner.client_video.take() {
                                    v.stop();
                                }
                                ctrl.stop();
                                inner.client.state = SessionState::Error;
                                inner.client.message = e.to_string();
                            }
                        }
                    }
                    Err(reason) => {
                        if let Some(v) = inner.client_video.take() {
                            v.stop();
                        }
                        inner.client.state = SessionState::Error;
                        inner.client.message = format!("Join failed: {reason}");
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(self.inner.lock().client.clone())
    }

    pub fn disconnect_client(&self) -> Result<ClientStatus, String> {
        let mut inner = self.inner.lock();
        if let Some(c) = inner.client_input.take() {
            c.stop();
        }
        if let Some(c) = inner.client_control.take() {
            c.stop();
        }
        if let Some(v) = inner.client_video.take() {
            v.stop();
        }
        inner.client.state = SessionState::Idle;
        inner.client.packets_sent = 0;
        inner.client.last_seq = 0;
        inner.client.message = "Disconnected.".into();
        Ok(inner.client.clone())
    }

    pub fn get_client_video(&self) -> ClientVideoSnapshot {
        let inner = self.inner.lock();
        if let Some(ref v) = inner.client_video {
            v.snapshot()
        } else {
            ClientVideoSnapshot {
                active: false,
                width: 0,
                height: 0,
                fps: 0.0,
                frames: 0,
                packets: 0,
                jpeg_base64: String::new(),
                detail: "Join a host to receive video.".into(),
            }
        }
    }

    /// Moonlight-style capture toggle for client KBM.
    pub fn set_input_capture(&self, active: bool) -> Result<CaptureStatus, String> {
        let inner = self.inner.lock();
        let Some(ref c) = inner.client_input else {
            return Err("Not in a live client session.".into());
        };
        c.set_capture(active);
        Ok(CaptureStatus::from_state(&c.capture()))
    }

    pub fn toggle_input_capture(&self) -> Result<CaptureStatus, String> {
        let inner = self.inner.lock();
        let Some(ref c) = inner.client_input else {
            return Err("Not in a live client session.".into());
        };
        let on = c.capture().toggle();
        Ok(CaptureStatus {
            active: on,
            hint: if on {
                "Input capture ON — mouse/keyboard go to host. Press Ctrl+Shift+Alt+Z to release."
                    .into()
            } else {
                "Input capture OFF — use this PC normally. Click Capture to control host."
                    .into()
            },
        })
    }

    pub fn get_input_capture(&self) -> CaptureStatus {
        let inner = self.inner.lock();
        if let Some(ref c) = inner.client_input {
            CaptureStatus::from_state(&c.capture())
        } else {
            CaptureStatus {
                active: false,
                hint: "Join a host session to enable input capture.".into(),
            }
        }
    }

    pub fn get_video_settings(&self) -> VideoSettings {
        settings_store::get()
    }

    pub fn set_video_settings(&self, settings: VideoSettings) -> Result<VideoSettings, String> {
        // Settings apply on next Start Host (capture thread already running otherwise).
        settings_store::set(settings)
    }

    /// Phase 4–5 capture metrics (host only).
    pub fn get_capture_stats(&self) -> CaptureSnapshot {
        let inner = self.inner.lock();
        if let Some(ref c) = inner.host_capture {
            c.stats().snapshot()
        } else {
            CaptureSnapshot {
                active: false,
                frames: 0,
                encoded_frames: 0,
                width: 0,
                height: 0,
                encode_width: 0,
                encode_height: 0,
                fps: 0.0,
                encode_fps: 0.0,
                last_capture_ms: 0.0,
                last_encode_ms: 0.0,
                bitrate_kbps: 0,
                encoder: "none".into(),
                detail: "Capture not running (Start Host to begin).".into(),
            }
        }
    }

    fn refresh_host_metrics(inner: &mut SessionInner) {
        if let Some(ref join) = inner.host_join {
            inner.host.pending_join = join.pending_snapshot().map(|p| PendingJoinInfo {
                peer_ip: p.peer_ip,
                client_name: p.client_name,
            });
            let active = join.has_accepted_session();
            if inner.host.session_active && !active {
                inner.host.session_active = false;
                inner.host.state = SessionState::Listening;
                inner.host.message =
                    "Client disconnected. Waiting for a new join request.".into();
            } else {
                inner.host.session_active = active;
            }
            if active && inner.host.state == SessionState::Listening {
                inner.host.state = SessionState::Streaming;
            }
        }

        if let Some(ref h) = inner.host_input {
            let s = h.stats();
            inner.host.packets_received = s.packets();
            inner.host.last_seq = s.last_seq();
            inner.host.input_latency_ms = s.latency_ms();
            inner.host.virtual_pad_active = h.virtual_pad_active();
            let detail = s.detail();
            if inner.host.pending_join.is_none() && !detail.is_empty() && s.packets() > 0 {
                inner.host.message = format!(
                    "{} · pkts {} · pad={}",
                    detail,
                    s.packets(),
                    if h.virtual_pad_active() { "ON" } else { "off" }
                );
            }
        }
    }

    fn refresh_client_metrics(inner: &mut SessionInner) {
        // Host stopped → control TCP dies → session_alive false → reset UI to idle
        let control_dead = inner
            .client_control
            .as_ref()
            .is_some_and(|c| !c.is_alive());
        let input_dead = inner
            .client_input
            .as_ref()
            .is_some_and(|c| !c.session_alive());

        if (control_dead || input_dead)
            && matches!(
                inner.client.state,
                SessionState::Streaming | SessionState::WaitingApproval
            )
        {
            if let Some(c) = inner.client_input.take() {
                c.stop();
            }
            if let Some(c) = inner.client_control.take() {
                c.stop();
            }
            inner.client.state = SessionState::Idle;
            inner.client.packets_sent = 0;
            inner.client.last_seq = 0;
            inner.client.message =
                "Host stopped or connection lost. You can Request to join again.".into();
            return;
        }

        if let Some(ref c) = inner.client_input {
            let s = c.stats();
            inner.client.packets_sent = s.packets();
            inner.client.last_seq = s.last_seq();
            inner.client.local_pad_connected = s.pad_connected();
            if s.packets() > 0 && inner.client.state == SessionState::Streaming {
                inner.client.message = format!(
                    "Live — pkts {} · pad {}",
                    s.packets(),
                    if s.pad_connected() {
                        "connected"
                    } else {
                        "none"
                    }
                );
            }
        } else {
            inner.client.local_pad_connected = poll_xinput(0).connected;
        }
    }
}
