//! Capture performance counters for the Host UI.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct AtomicCaptureStats {
    frames: AtomicU64,
    width: AtomicU32,
    height: AtomicU32,
    fps_x100: AtomicU32,
    last_capture_us: AtomicU64,
    active: AtomicBool,
    detail: Mutex<String>,
}

impl AtomicCaptureStats {
    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
    }

    pub fn record_frame(&self, width: u32, height: u32, capture_us: u64) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.width.store(width, Ordering::Relaxed);
        self.height.store(height, Ordering::Relaxed);
        self.last_capture_us.store(capture_us, Ordering::Relaxed);
    }

    pub fn set_fps(&self, fps: f32) {
        self.fps_x100
            .store((fps * 100.0).clamp(0.0, 1_000_000.0) as u32, Ordering::Relaxed);
    }

    pub fn set_detail(&self, msg: impl Into<String>) {
        if let Ok(mut g) = self.detail.lock() {
            *g = msg.into();
        }
    }

    pub fn snapshot(&self) -> CaptureSnapshot {
        CaptureSnapshot {
            active: self.active.load(Ordering::Relaxed),
            frames: self.frames.load(Ordering::Relaxed),
            width: self.width.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
            fps: self.fps_x100.load(Ordering::Relaxed) as f32 / 100.0,
            last_capture_ms: self.last_capture_us.load(Ordering::Relaxed) as f32 / 1000.0,
            detail: self
                .detail
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSnapshot {
    pub active: bool,
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub last_capture_ms: f32,
    pub detail: String,
}
