//! Capture + encode performance counters for the Host UI.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct AtomicCaptureStats {
    frames: AtomicU64,
    encoded_frames: AtomicU64,
    width: AtomicU32,
    height: AtomicU32,
    encode_width: AtomicU32,
    encode_height: AtomicU32,
    fps_x100: AtomicU32,
    encode_fps_x100: AtomicU32,
    last_capture_us: AtomicU64,
    last_encode_us: AtomicU64,
    bytes_encoded: AtomicU64,
    bitrate_kbps: AtomicU32,
    active: AtomicBool,
    detail: Mutex<String>,
    encoder_name: Mutex<String>,
}

impl AtomicCaptureStats {
    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
    }

    pub fn set_encoder_name(&self, name: impl Into<String>) {
        if let Ok(mut g) = self.encoder_name.lock() {
            *g = name.into();
        }
    }

    pub fn record_frame(&self, width: u32, height: u32, capture_us: u64) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.width.store(width, Ordering::Relaxed);
        self.height.store(height, Ordering::Relaxed);
        self.last_capture_us.store(capture_us, Ordering::Relaxed);
    }

    pub fn record_encode(
        &self,
        encode_w: u32,
        encode_h: u32,
        encode_us: u64,
        payload_bytes: usize,
    ) {
        self.encoded_frames.fetch_add(1, Ordering::Relaxed);
        self.encode_width.store(encode_w, Ordering::Relaxed);
        self.encode_height.store(encode_h, Ordering::Relaxed);
        self.last_encode_us.store(encode_us, Ordering::Relaxed);
        self.bytes_encoded
            .fetch_add(payload_bytes as u64, Ordering::Relaxed);
    }

    pub fn set_fps(&self, fps: f32) {
        self.fps_x100
            .store((fps * 100.0).clamp(0.0, 1_000_000.0) as u32, Ordering::Relaxed);
    }

    pub fn set_encode_fps(&self, fps: f32) {
        self.encode_fps_x100
            .store((fps * 100.0).clamp(0.0, 1_000_000.0) as u32, Ordering::Relaxed);
    }

    pub fn set_bitrate_kbps(&self, kbps: u32) {
        self.bitrate_kbps.store(kbps, Ordering::Relaxed);
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
            encoded_frames: self.encoded_frames.load(Ordering::Relaxed),
            width: self.width.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
            encode_width: self.encode_width.load(Ordering::Relaxed),
            encode_height: self.encode_height.load(Ordering::Relaxed),
            fps: self.fps_x100.load(Ordering::Relaxed) as f32 / 100.0,
            encode_fps: self.encode_fps_x100.load(Ordering::Relaxed) as f32 / 100.0,
            last_capture_ms: self.last_capture_us.load(Ordering::Relaxed) as f32 / 1000.0,
            last_encode_ms: self.last_encode_us.load(Ordering::Relaxed) as f32 / 1000.0,
            bitrate_kbps: self.bitrate_kbps.load(Ordering::Relaxed),
            encoder: self
                .encoder_name
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "none".into()),
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
    pub encoded_frames: u64,
    pub width: u32,
    pub height: u32,
    pub encode_width: u32,
    pub encode_height: u32,
    pub fps: f32,
    pub encode_fps: f32,
    pub last_capture_ms: f32,
    pub last_encode_ms: f32,
    pub bitrate_kbps: u32,
    pub encoder: String,
    pub detail: String,
}
