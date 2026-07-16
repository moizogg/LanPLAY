use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// Lock-free-ish counters for the input path (UI polling).
#[derive(Debug, Default)]
pub struct AtomicInputStats {
    packets: AtomicU64,
    last_seq: AtomicU32,
    /// Latency in microseconds (host-side estimate).
    latency_us: AtomicU64,
    pad_connected: AtomicBool,
    detail: Mutex<String>,
}

impl AtomicInputStats {
    pub fn record_send(&self, seq: u32, connected: bool) {
        self.packets.fetch_add(1, Ordering::Relaxed);
        self.last_seq.store(seq, Ordering::Relaxed);
        self.pad_connected.store(connected, Ordering::Relaxed);
    }

    pub fn record_recv(&self, seq: u32, latency_us: u64, connected: bool) {
        self.packets.fetch_add(1, Ordering::Relaxed);
        self.last_seq.store(seq, Ordering::Relaxed);
        self.latency_us.store(latency_us, Ordering::Relaxed);
        self.pad_connected.store(connected, Ordering::Relaxed);
    }

    pub fn set_detail(&self, msg: impl Into<String>) {
        if let Ok(mut g) = self.detail.lock() {
            *g = msg.into();
        }
    }

    pub fn packets(&self) -> u64 {
        self.packets.load(Ordering::Relaxed)
    }

    pub fn last_seq(&self) -> u32 {
        self.last_seq.load(Ordering::Relaxed)
    }

    pub fn latency_ms(&self) -> f32 {
        self.latency_us.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn pad_connected(&self) -> bool {
        self.pad_connected.load(Ordering::Relaxed)
    }

    pub fn detail(&self) -> String {
        self.detail
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}
