//! Host UDP receive loop.
//!
//! - Start Host = listen only (NO virtual pad).
//! - Virtual Xbox 360 appears only when a **client** reports a connected controller.
//! - Virtual pad is removed when client disconnects / unplugs pad / host stops.
//! - Keyboard/mouse from client is injected when remote input is allowed.

use crate::packet_stats::AtomicInputStats;
use crate::virtual_pad::{create_virtual_pad, probe_vigem, VirtualPadBackend};
use lanplay_input::{apply_kbm_on_host, HostKbmState};
use lanplay_protocol::{
    packet_magic, InputPacket, KbmPacket, INPUT_PACKET_MAGIC, INPUT_PACKET_SIZE, KBM_PACKET_MAGIC,
    KBM_PACKET_SIZE,
};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Shared host-side pad lifecycle flags for the UI.
#[derive(Clone, Default)]
pub struct HostPadFlags {
    /// 0 = none, 1 = virtual pad plugged for remote client controller
    pub virtual_active: Arc<AtomicU8>,
}

pub struct HostInputConfig {
    pub media_port: u16,
    pub allow_remote_input: bool,
}

pub struct HostInputHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    stats: Arc<AtomicInputStats>,
    pad_flags: HostPadFlags,
    vigem_ok: bool,
    vigem_detail: String,
}

impl HostInputHandle {
    pub fn stats(&self) -> Arc<AtomicInputStats> {
        Arc::clone(&self.stats)
    }

    pub fn pad_flags(&self) -> HostPadFlags {
        self.pad_flags.clone()
    }

    pub fn virtual_pad_active(&self) -> bool {
        self.pad_flags.virtual_active.load(Ordering::Relaxed) != 0
    }

    pub fn vigem_ok(&self) -> bool {
        self.vigem_ok
    }

    pub fn vigem_detail(&self) -> &str {
        &self.vigem_detail
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for HostInputHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub fn run_host_input_loop(config: HostInputConfig) -> lanplay_shared::Result<HostInputHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(AtomicInputStats::default());
    let pad_flags = HostPadFlags::default();

    // Probe bus only — do NOT create a virtual pad here.
    let probe = probe_vigem();
    let vigem_ok = probe.available;
    let vigem_detail = probe.detail.clone();
    stats.set_detail(format!(
        "Listening — waiting for client. No virtual pad until client connects a controller. ({})",
        vigem_detail
    ));

    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);
    let flags_t = pad_flags.clone();
    let port = config.media_port;
    let allow = config.allow_remote_input;

    let join = thread::Builder::new()
        .name("lanplay-host-input".into())
        .spawn(move || {
            host_loop(port, allow, stop_t, stats_t, flags_t, vigem_ok);
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(HostInputHandle {
        stop,
        join: Some(join),
        stats,
        pad_flags,
        vigem_ok,
        vigem_detail,
    })
}

fn host_loop(
    media_port: u16,
    allow_remote_input: bool,
    stop: Arc<AtomicBool>,
    stats: Arc<AtomicInputStats>,
    pad_flags: HostPadFlags,
    vigem_ok: bool,
) {
    let bind = format!("0.0.0.0:{media_port}");
    let sock = match std::net::UdpSocket::bind(&bind) {
        Ok(s) => s,
        Err(e) => {
            stats.set_detail(format!("failed to bind {bind}: {e}"));
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(100)));
    stats.set_detail(format!(
        "Listening on {bind}. Virtual pad = only when client has a controller. KBM from client when connected."
    ));

    let mut buf = [0u8; 256];
    let mut last_seq: Option<u32> = None;
    let mut pad: Option<Box<dyn VirtualPadBackend>> = None;
    let mut last_pad_packet = Instant::now();
    let mut kbm_state = HostKbmState::default();
    let mut last_client_seen = Instant::now();
    let mut client_seen = false;

    // If no pad packet for this long while pad is up, unplug (client gone / unplugged).
    const PAD_TIMEOUT: Duration = Duration::from_secs(2);
    const CLIENT_IDLE: Duration = Duration::from_secs(3);

    while !stop.load(Ordering::Relaxed) {
        // Timeout virtual pad if client stopped sending connected pad state
        if pad.is_some() && last_pad_packet.elapsed() > PAD_TIMEOUT {
            if let Some(mut p) = pad.take() {
                let _ = p.unplug();
            }
            pad_flags.virtual_active.store(0, Ordering::Relaxed);
            stats.set_detail(
                "Client controller gone / timed out — virtual Xbox 360 removed.",
            );
        }

        if client_seen && last_client_seen.elapsed() > CLIENT_IDLE {
            client_seen = false;
            if pad.is_none() {
                stats.set_detail(
                    "No client packets — still listening. No virtual pad until client controller.",
                );
            }
        }

        match sock.recv_from(&mut buf) {
            Ok((n, _from)) if n >= 4 => {
                last_client_seen = Instant::now();
                client_seen = true;

                let magic = match packet_magic(&buf[..n]) {
                    Some(m) => m,
                    None => continue,
                };

                if magic == INPUT_PACKET_MAGIC && n >= INPUT_PACKET_SIZE {
                    let packet = match InputPacket::decode(&buf[..INPUT_PACKET_SIZE]) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    last_seq = Some(packet.seq);

                    let now = unix_micros();
                    let latency = now.saturating_sub(packet.client_ts_us);
                    let latency = if latency > 5_000_000 { 0 } else { latency };
                    stats.record_recv(packet.seq, latency, packet.is_connected());

                    if !allow_remote_input {
                        continue;
                    }

                    if packet.is_connected() {
                        last_pad_packet = Instant::now();
                        // Create virtual pad only when client has a real controller
                        if pad.is_none() {
                            if !vigem_ok {
                                stats.set_detail(
                                    "Client has a controller but ViGEmBus is not ready — install driver.",
                                );
                            } else {
                                match create_virtual_pad() {
                                    Ok(p) => {
                                        stats.set_detail(
                                            "Client controller connected → virtual Xbox 360 created on host.",
                                        );
                                        pad_flags.virtual_active.store(1, Ordering::Relaxed);
                                        pad = Some(p);
                                    }
                                    Err(e) => {
                                        stats.set_detail(format!("Could not create virtual pad: {e}"));
                                    }
                                }
                            }
                        }
                        if let Some(ref mut p) = pad {
                            if let Err(e) = p.apply(&packet) {
                                stats.set_detail(format!("pad apply error: {e}"));
                            }
                        }
                    } else {
                        // Client explicitly has no pad — remove virtual device
                        if let Some(mut p) = pad.take() {
                            let _ = p.unplug();
                            pad_flags.virtual_active.store(0, Ordering::Relaxed);
                            stats.set_detail(
                                "Client has no controller — virtual Xbox 360 removed.",
                            );
                        }
                    }
                } else if magic == KBM_PACKET_MAGIC && n >= KBM_PACKET_SIZE {
                    if !allow_remote_input {
                        continue;
                    }
                    let packet = match KbmPacket::decode(&buf[..KBM_PACKET_SIZE]) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    apply_kbm_on_host(&mut kbm_state, &packet);
                    if pad.is_none() {
                        stats.set_detail(
                            "Receiving client keyboard/mouse. Virtual pad appears only if they plug a controller.",
                        );
                    }
                }
            }
            Ok(_) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                stats.set_detail(format!("recv error: {e}"));
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    // Host stop: always remove virtual pad
    if let Some(mut p) = pad.take() {
        let _ = p.unplug();
    }
    pad_flags.virtual_active.store(0, Ordering::Relaxed);
    let _ = last_seq;
}

fn unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}
