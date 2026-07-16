//! Controller + input subsystem (Moonlight-style client capture).

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

pub use lanplay_input::{CaptureState, CaptureStatus};
use lanplay_input::{
    sample_kbm_on_client, ungrab_hotkey_pressed, ClientKbmState, ExclusivePadGuard,
};
use lanplay_protocol::{InputPacket, FLAG_CONNECTED};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Client-side sender: pad + keyboard/mouse → host UDP.
pub struct ClientInputHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    stats: Arc<AtomicInputStats>,
    session_alive: Arc<AtomicBool>,
    capture: CaptureState,
}

impl ClientInputHandle {
    pub fn stats(&self) -> Arc<AtomicInputStats> {
        Arc::clone(&self.stats)
    }

    pub fn session_alive(&self) -> bool {
        self.session_alive.load(Ordering::Relaxed) && !self.stop.load(Ordering::Relaxed)
    }

    pub fn capture(&self) -> CaptureState {
        self.capture.clone()
    }

    pub fn set_capture(&self, on: bool) {
        self.capture.set_active(on);
    }

    pub fn stop(mut self) {
        self.capture.set_active(false);
        self.stop.store(true, Ordering::SeqCst);
        self.session_alive.store(false, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for ClientInputHandle {
    fn drop(&mut self) {
        self.capture.set_active(false);
        self.stop.store(true, Ordering::SeqCst);
        self.session_alive.store(false, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Spawn client input thread.
/// Auto-enables capture when session starts (Moonlight starts captured for stream).
pub fn run_client_input_loop(
    host_ip: String,
    media_port: u16,
    poll_hz: u32,
    session_alive: Arc<AtomicBool>,
) -> lanplay_shared::Result<ClientInputHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(AtomicInputStats::default());
    // Start with capture ON after join (like Moonlight entering a stream)
    let capture = CaptureState::new(true);
    capture.set_active(true);

    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);
    let alive_t = Arc::clone(&session_alive);
    let capture_t = capture.clone();

    let join = thread::Builder::new()
        .name("lanplay-client-input".into())
        .spawn(move || {
            client_loop(host_ip, media_port, poll_hz, stop_t, alive_t, stats_t, capture_t);
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(ClientInputHandle {
        stop,
        join: Some(join),
        stats,
        session_alive,
        capture,
    })
}

fn client_loop(
    host_ip: String,
    media_port: u16,
    poll_hz: u32,
    stop: Arc<AtomicBool>,
    session_alive: Arc<AtomicBool>,
    stats: Arc<AtomicInputStats>,
    capture: CaptureState,
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

    let exclusive = ExclusivePadGuard::acquire();
    let exclusive_n = exclusive.count();

    let period = Duration::from_micros((1_000_000u64 / poll_hz.max(30) as u64).max(1));
    let mut seq: u32 = 0;
    let mut kbm_state = ClientKbmState::default();
    let mut pad_seen_streak: u32 = 0;
    let mut pad_lost_streak: u32 = 0;
    let mut pad_active = false;
    let mut last_good = PhysicalPadState::default();
    let mut ungrab_armed = true;
    let mut last_detail = Instant::now();

    stats.set_detail(format!(
        "Live → {addr}. Capture ON (Ctrl+Shift+Alt+Z to release). HID holds: {exclusive_n}."
    ));

    while !stop.load(Ordering::Relaxed) && session_alive.load(Ordering::Relaxed) {
        seq = seq.wrapping_add(1);

        // Moonlight-style ungrab hotkey
        if ungrab_hotkey_pressed() {
            if ungrab_armed && capture.is_active() {
                capture.set_active(false);
                // Force one empty packet path for raise-all-keys
                ungrab_armed = false;
                stats.set_detail(
                    "Capture OFF — local desktop free. Click Capture to control host again.",
                );
            }
        } else {
            ungrab_armed = true;
        }

        let capt = capture.is_active();
        let kbm = sample_kbm_on_client(&mut kbm_state, seq, capt);
        let _ = sock.send_to(&kbm.encode(), addr);

        // Gamepad always sent (like Moonlight background gamepad optional — we always send pad)
        let sample = poll_first_connected_pad();
        if sample.connected {
            pad_seen_streak = pad_seen_streak.saturating_add(1);
            pad_lost_streak = 0;
            last_good = sample;
            if !pad_active && pad_seen_streak >= 8 {
                pad_active = true;
                if last_detail.elapsed() > Duration::from_secs(1) {
                    stats.set_detail("Controller active → host virtual pad.");
                    last_detail = Instant::now();
                }
            }
        } else {
            pad_lost_streak = pad_lost_streak.saturating_add(1);
            pad_seen_streak = 0;
            if pad_active && pad_lost_streak >= 250 {
                pad_active = false;
                last_good = PhysicalPadState::default();
                stats.set_detail("Controller disconnected locally.");
            }
        }

        let mut packet = if pad_active {
            InputPacket {
                controller_id: 0,
                flags: FLAG_CONNECTED,
                seq,
                client_ts_us: 0,
                buttons: last_good.buttons,
                left_trigger: last_good.left_trigger,
                right_trigger: last_good.right_trigger,
                thumb_lx: last_good.thumb_lx,
                thumb_ly: last_good.thumb_ly,
                thumb_rx: last_good.thumb_rx,
                thumb_ry: last_good.thumb_ry,
            }
        } else {
            InputPacket::now_disconnected(0, seq)
        };
        packet.stamp_now();

        match sock.send_to(&packet.encode(), addr) {
            Ok(_) => stats.record_send(seq, pad_active),
            Err(e) => {
                stats.set_detail(format!("send error (host may have stopped): {e}"));
                session_alive.store(false, Ordering::SeqCst);
                break;
            }
        }

        thread::sleep(period);
    }

    capture.set_active(false);
    drop(exclusive);
    stats.set_detail("Client input stopped. Capture released.");
    session_alive.store(false, Ordering::SeqCst);
}

fn poll_first_connected_pad() -> PhysicalPadState {
    for user in 0..4 {
        let s = poll_xinput(user);
        if s.connected {
            return s;
        }
    }
    PhysicalPadState::default()
}
