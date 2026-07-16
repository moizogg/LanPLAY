//! Host UDP receive loop → virtual Xbox 360.

use crate::packet_stats::AtomicInputStats;
use crate::virtual_pad::{create_virtual_pad, NullVirtualPad, VirtualPadBackend};
use lanplay_protocol::{InputPacket, INPUT_PACKET_SIZE};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct HostInputConfig {
    pub media_port: u16,
    pub allow_remote_input: bool,
}

pub struct HostInputHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    stats: Arc<AtomicInputStats>,
    vigem_ok: bool,
    vigem_detail: String,
}

impl HostInputHandle {
    pub fn stats(&self) -> Arc<AtomicInputStats> {
        Arc::clone(&self.stats)
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

    let (vigem_ok, vigem_detail, mut pad): (bool, String, Box<dyn VirtualPadBackend>) =
        match create_virtual_pad() {
            Ok(p) => {
                let st = p.status();
                (true, st.detail, p)
            }
            Err(e) => {
                let detail = e.clone();
                (
                    false,
                    detail.clone(),
                    Box::new(NullVirtualPad::new(detail)),
                )
            }
        };

    stats.set_detail(vigem_detail.clone());

    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);
    let port = config.media_port;
    let allow = config.allow_remote_input;

    let join = thread::Builder::new()
        .name("lanplay-host-input".into())
        .spawn(move || {
            host_loop(port, allow, stop_t, stats_t, &mut *pad);
            let _ = pad.unplug();
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(HostInputHandle {
        stop,
        join: Some(join),
        stats,
        vigem_ok,
        vigem_detail,
    })
}

fn host_loop(
    media_port: u16,
    allow_remote_input: bool,
    stop: Arc<AtomicBool>,
    stats: Arc<AtomicInputStats>,
    pad: &mut dyn VirtualPadBackend,
) {
    let bind = format!("0.0.0.0:{media_port}");
    let sock = match std::net::UdpSocket::bind(&bind) {
        Ok(s) => s,
        Err(e) => {
            stats.set_detail(format!("failed to bind {bind}: {e}"));
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    stats.set_detail(format!(
        "listening for controller UDP on {bind} (allow_input={allow_remote_input})"
    ));

    let mut buf = [0u8; 256];
    let mut last_seq: Option<u32> = None;

    while !stop.load(Ordering::Relaxed) {
        match sock.recv_from(&mut buf) {
            Ok((n, _from)) if n >= INPUT_PACKET_SIZE => {
                let packet = match InputPacket::decode(&buf[..INPUT_PACKET_SIZE]) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                // Latest-state: drop clearly older seq (wrapping-aware-ish).
                if let Some(prev) = last_seq {
                    if packet.seq != prev.wrapping_add(1)
                        && packet.seq.wrapping_sub(prev) > u32::MAX / 2
                    {
                        // very old; still apply — network reordering rare for small RTT
                    }
                }
                last_seq = Some(packet.seq);

                let now = unix_micros();
                let latency = now.saturating_sub(packet.client_ts_us);
                // Clamp absurd clock skew so UI doesn't show hours.
                let latency = if latency > 5_000_000 { 0 } else { latency };

                stats.record_recv(packet.seq, latency, packet.is_connected());

                if allow_remote_input {
                    if let Err(e) = pad.apply(&packet) {
                        stats.set_detail(format!("apply error: {e}"));
                    }
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                stats.set_detail(format!("recv error: {e}"));
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}
