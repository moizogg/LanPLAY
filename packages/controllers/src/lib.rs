//! Controller + input subsystem.
//!
//! Host creates virtual Xbox 360 only after client controller reports stay stable.
//! Client stops sending when control TCP dies (host Stop Host).

mod host;
mod packet_stats;
mod paths;
mod physical;
mod virtual_pad;

pub use host::{run_host_input_loop, HostInputConfig, HostInputHandle, HostPadFlags};
pub use packet_stats::AtomicInputStats;
pub use paths::{
    bundle_status, bundled_driver_setup, configure_vigem_search_paths, install_bundled_driver,
    VigemBundleStatus,
};
pub use physical::{poll_xinput, PhysicalPadState};
pub use virtual_pad::{
    create_virtual_pad, probe_vigem, NullVirtualPad, VigemStatus, VirtualPadBackend,
};

use lanplay_input::{sample_kbm_on_client, ClientKbmState};
use lanplay_protocol::{InputPacket, FLAG_CONNECTED};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Client-side sender: pad + keyboard/mouse → host UDP.
pub struct ClientInputHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    stats: Arc<AtomicInputStats>,
    /// Shared with control-session watch — cleared when host drops TCP.
    session_alive: Arc<AtomicBool>,
}

impl ClientInputHandle {
    pub fn stats(&self) -> Arc<AtomicInputStats> {
        Arc::clone(&self.stats)
    }

    pub fn session_alive(&self) -> bool {
        self.session_alive.load(Ordering::Relaxed) && !self.stop.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.session_alive.store(false, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for ClientInputHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.session_alive.store(false, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Spawn client input thread. Stops when `session_alive` goes false (host stopped).
pub fn run_client_input_loop(
    host_ip: String,
    media_port: u16,
    poll_hz: u32,
    session_alive: Arc<AtomicBool>,
) -> lanplay_shared::Result<ClientInputHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(AtomicInputStats::default());
    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);
    let alive_t = Arc::clone(&session_alive);

    let join = thread::Builder::new()
        .name("lanplay-client-input".into())
        .spawn(move || {
            client_loop(host_ip, media_port, poll_hz, stop_t, alive_t, stats_t);
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(ClientInputHandle {
        stop,
        join: Some(join),
        stats,
        session_alive,
    })
}

fn client_loop(
    host_ip: String,
    media_port: u16,
    poll_hz: u32,
    stop: Arc<AtomicBool>,
    session_alive: Arc<AtomicBool>,
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
    let mut kbm_state = ClientKbmState::default();
    // Hysteresis for XInput flapping
    let mut pad_seen_streak: u32 = 0;
    let mut pad_lost_streak: u32 = 0;
    let mut pad_active = false;

    stats.set_detail(format!(
        "Sending KBM + controller to {addr}. Stops if host ends the session."
    ));

    while !stop.load(Ordering::Relaxed) && session_alive.load(Ordering::Relaxed) {
        seq = seq.wrapping_add(1);

        let kbm = sample_kbm_on_client(&mut kbm_state, seq);
        let _ = sock.send_to(&kbm.encode(), addr);

        let sample = poll_xinput(0);
        if sample.connected {
            pad_seen_streak = pad_seen_streak.saturating_add(1);
            pad_lost_streak = 0;
            if !pad_active && pad_seen_streak >= 5 {
                pad_active = true;
            }
        } else {
            pad_lost_streak = pad_lost_streak.saturating_add(1);
            pad_seen_streak = 0;
            if pad_active && pad_lost_streak >= 15 {
                pad_active = false;
            }
        }

        // Only claim connected when hysteresis says pad is stable
        let mut packet = if pad_active && sample.connected {
            InputPacket {
                controller_id: 0,
                flags: FLAG_CONNECTED,
                seq,
                client_ts_us: 0,
                buttons: sample.buttons,
                left_trigger: sample.left_trigger,
                right_trigger: sample.right_trigger,
                thumb_lx: sample.thumb_lx,
                thumb_ly: sample.thumb_ly,
                thumb_rx: sample.thumb_rx,
                thumb_ry: sample.thumb_ry,
            }
        } else if pad_active {
            // Brief XInput glitch — still send last-known connected empty-ish? better send connected with zeros
            InputPacket {
                controller_id: 0,
                flags: FLAG_CONNECTED,
                seq,
                client_ts_us: 0,
                buttons: 0,
                left_trigger: 0,
                right_trigger: 0,
                thumb_lx: 0,
                thumb_ly: 0,
                thumb_rx: 0,
                thumb_ry: 0,
            }
        } else {
            InputPacket::now_disconnected(0, seq)
        };
        packet.stamp_now();

        match sock.send_to(&packet.encode(), addr) {
            Ok(_) => stats.record_send(seq, pad_active),
            Err(e) => {
                // Host gone / network error — end session so UI resets
                stats.set_detail(format!("send error (host may have stopped): {e}"));
                session_alive.store(false, Ordering::SeqCst);
                break;
            }
        }

        thread::sleep(period);
    }

    stats.set_detail("Client input stopped (session ended).".into());
    session_alive.store(false, Ordering::SeqCst);
}
