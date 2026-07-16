//! Host UDP receive loop.
//!
//! Virtual pad is debounced: only create after sustained "connected" reports,
//! only remove after sustained disconnect / timeout (no plug/unplug spam).

use crate::packet_stats::AtomicInputStats;
use crate::virtual_pad::{create_virtual_pad, probe_vigem, VirtualPadBackend};
use lanplay_input::{apply_kbm_on_host, HostKbmState};
use lanplay_protocol::{
    packet_magic, InputPacket, KbmPacket, INPUT_PACKET_MAGIC, INPUT_PACKET_SIZE, KBM_PACKET_MAGIC,
    KBM_PACKET_SIZE,
};
use parking_lot::Mutex;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Shared host-side pad lifecycle flags for the UI.
#[derive(Clone, Default)]
pub struct HostPadFlags {
    pub virtual_active: Arc<AtomicU8>,
}

pub struct HostInputConfig {
    pub media_port: u16,
    pub allow_remote_input: bool,
    pub allowed_peer: Arc<Mutex<Option<IpAddr>>>,
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

    let probe = probe_vigem();
    let vigem_ok = probe.available;
    let vigem_detail = probe.detail.clone();
    stats.set_detail(format!(
        "Listening — accept a client first. Virtual pad only after their controller stays connected. ({})",
        vigem_detail
    ));

    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);
    let flags_t = pad_flags.clone();
    let port = config.media_port;
    let allow = config.allow_remote_input;
    let allowed_peer = config.allowed_peer;

    let join = thread::Builder::new()
        .name("lanplay-host-input".into())
        .spawn(move || {
            host_loop(
                port,
                allow,
                stop_t,
                stats_t,
                flags_t,
                vigem_ok,
                allowed_peer,
            );
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
    allowed_peer: Arc<Mutex<Option<IpAddr>>>,
) {
    let bind = format!("0.0.0.0:{media_port}");
    let sock = match std::net::UdpSocket::bind(&bind) {
        Ok(s) => s,
        Err(e) => {
            stats.set_detail(format!("failed to bind {bind}: {e}"));
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(50)));
    stats.set_detail(format!(
        "Listening on {bind}. UDP accepted only from approved client."
    ));

    let mut buf = [0u8; 256];
    let mut pad: Option<Box<dyn VirtualPadBackend>> = None;
    let mut kbm_state = HostKbmState::default();

    // Debounce: avoid USB plug/unplug spam from flaky XInput reports
    let mut connected_streak: u32 = 0;
    let mut disconnected_streak: u32 = 0;
    let mut last_connected_seen: Option<Instant> = None;
    let mut last_log = Instant::now();

    /// Need this many consecutive "connected" packets before creating ViGEm target.
    const CREATE_STREAK: u32 = 10; // ~40ms at 250Hz
    /// Need this many consecutive "disconnected" before removing (ignore short XInput blips).
    const DESTROY_STREAK: u32 = 120; // ~500ms of "no pad"
    /// Also remove if no *connected* packet at all for this long (UDP loss / unplug).
    const DESTROY_SILENCE: Duration = Duration::from_millis(2500);

    while !stop.load(Ordering::Relaxed) {
        // Silence timeout: client stopped reporting a connected pad
        if pad.is_some() {
            if let Some(t) = last_connected_seen {
                if t.elapsed() > DESTROY_SILENCE {
                    if let Some(mut p) = pad.take() {
                        let _ = p.unplug();
                    }
                    pad_flags.virtual_active.store(0, Ordering::Relaxed);
                    connected_streak = 0;
                    disconnected_streak = 0;
                    last_connected_seen = None;
                    stats.set_detail(
                        "Virtual pad removed (client controller silent / unplugged).",
                    );
                }
            }
        }

        match sock.recv_from(&mut buf) {
            Ok((n, from)) if n >= 4 => {
                let allowed = *allowed_peer.lock();
                match allowed {
                    Some(ip) if ip == from.ip() => {}
                    _ => continue,
                }

                let magic = match packet_magic(&buf[..n]) {
                    Some(m) => m,
                    None => continue,
                };

                if magic == INPUT_PACKET_MAGIC && n >= INPUT_PACKET_SIZE {
                    let packet = match InputPacket::decode(&buf[..INPUT_PACKET_SIZE]) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };

                    let now = unix_micros();
                    let latency = now.saturating_sub(packet.client_ts_us);
                    let latency = if latency > 5_000_000 { 0 } else { latency };
                    stats.record_recv(packet.seq, latency, packet.is_connected());

                    if !allow_remote_input {
                        continue;
                    }

                    if packet.is_connected() {
                        connected_streak = connected_streak.saturating_add(1);
                        disconnected_streak = 0;
                        last_connected_seen = Some(Instant::now());

                        // Create only after sustained connected reports
                        if pad.is_none() && connected_streak >= CREATE_STREAK {
                            if !vigem_ok {
                                if last_log.elapsed() > Duration::from_secs(2) {
                                    stats.set_detail(
                                        "Client has controller but ViGEmBus not ready — install driver.",
                                    );
                                    last_log = Instant::now();
                                }
                            } else {
                                match create_virtual_pad() {
                                    Ok(p) => {
                                        stats.set_detail(
                                            "Client controller stable → virtual Xbox 360 on host.",
                                        );
                                        pad_flags.virtual_active.store(1, Ordering::Relaxed);
                                        pad = Some(p);
                                    }
                                    Err(e) => {
                                        stats.set_detail(format!("create virtual pad failed: {e}"));
                                    }
                                }
                            }
                        }

                        if let Some(ref mut p) = pad {
                            if let Err(e) = p.apply(&packet) {
                                if last_log.elapsed() > Duration::from_secs(1) {
                                    stats.set_detail(format!("pad apply error: {e}"));
                                    last_log = Instant::now();
                                }
                            }
                        }
                    } else {
                        // Do NOT unplug on a single disconnected packet
                        disconnected_streak = disconnected_streak.saturating_add(1);
                        connected_streak = 0;

                        if pad.is_some() && disconnected_streak >= DESTROY_STREAK {
                            if let Some(mut p) = pad.take() {
                                let _ = p.unplug();
                            }
                            pad_flags.virtual_active.store(0, Ordering::Relaxed);
                            last_connected_seen = None;
                            disconnected_streak = 0;
                            stats.set_detail(
                                "Client reports no controller (stable) — virtual pad removed.",
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
                }
            }
            Ok(_) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                if last_log.elapsed() > Duration::from_secs(2) {
                    stats.set_detail(format!("recv error: {e}"));
                    last_log = Instant::now();
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    if let Some(mut p) = pad.take() {
        let _ = p.unplug();
    }
    pad_flags.virtual_active.store(0, Ordering::Relaxed);
}

fn unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}
