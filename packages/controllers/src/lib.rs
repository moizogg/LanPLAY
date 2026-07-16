//! Controller subsystem (Phase 2).
//!
//! - Client: poll XInput → encode `InputPacket` → UDP to host  
//! - Host: UDP recv → decode → **static ViGEm** Xbox 360 (Sunshine-style)

mod host;
mod packet_stats;
mod paths;
mod physical;
mod virtual_pad;

pub use host::{run_host_input_loop, HostInputConfig, HostInputHandle};
pub use packet_stats::AtomicInputStats;
pub use paths::{
    bundle_status, bundled_driver_setup, configure_vigem_search_paths, install_bundled_driver,
    VigemBundleStatus,
};
pub use physical::{poll_xinput, PhysicalPadState};
pub use virtual_pad::{
    create_virtual_pad, probe_vigem, NullVirtualPad, VigemStatus, VirtualPadBackend,
};

use lanplay_protocol::{InputPacket, FLAG_CONNECTED};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Client-side sender: poll pad + send UDP packets to host.
pub struct ClientInputHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    stats: Arc<AtomicInputStats>,
}

impl ClientInputHandle {
    pub fn stats(&self) -> Arc<AtomicInputStats> {
        Arc::clone(&self.stats)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for ClientInputHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Spawn client input thread. Sends to `host:media_port` over UDP.
pub fn run_client_input_loop(
    host_ip: String,
    media_port: u16,
    poll_hz: u32,
) -> lanplay_shared::Result<ClientInputHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(AtomicInputStats::default());
    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);

    let join = thread::Builder::new()
        .name("lanplay-client-input".into())
        .spawn(move || {
            client_loop(host_ip, media_port, poll_hz, stop_t, stats_t);
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(ClientInputHandle {
        stop,
        join: Some(join),
        stats,
    })
}

fn client_loop(
    host_ip: String,
    media_port: u16,
    poll_hz: u32,
    stop: Arc<AtomicBool>,
    stats: Arc<AtomicInputStats>,
) {
    let addr: SocketAddr = match format!("{host_ip}:{media_port}").parse() {
        Ok(a) => a,
        Err(e) => {
            stats.set_detail(format!("bad host address: {e}"));
            return;
        }
    };

    let sock = match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            stats.set_detail(format!("bind failed: {e}"));
            return;
        }
    };
    let _ = sock.set_write_timeout(Some(Duration::from_millis(50)));

    let period = Duration::from_micros((1_000_000u64 / poll_hz.max(30) as u64).max(1));
    let mut seq: u32 = 0;
    stats.set_detail(format!("sending controller packets to {addr}"));

    while !stop.load(Ordering::Relaxed) {
        let pad = poll_xinput(0);
        seq = seq.wrapping_add(1);

        let mut packet = if pad.connected {
            InputPacket {
                controller_id: 0,
                flags: FLAG_CONNECTED,
                seq,
                client_ts_us: 0,
                buttons: pad.buttons,
                left_trigger: pad.left_trigger,
                right_trigger: pad.right_trigger,
                thumb_lx: pad.thumb_lx,
                thumb_ly: pad.thumb_ly,
                thumb_rx: pad.thumb_rx,
                thumb_ry: pad.thumb_ry,
            }
        } else {
            InputPacket::now_disconnected(0, seq)
        };
        packet.stamp_now();

        let bytes = packet.encode();
        match sock.send_to(&bytes, addr) {
            Ok(_) => {
                stats.record_send(seq, pad.connected);
            }
            Err(e) => {
                stats.set_detail(format!("send error: {e}"));
            }
        }

        thread::sleep(period);
    }
}
