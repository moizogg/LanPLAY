//! Video pipeline (Phase 4–5).
//!
//! Capture → encode (H.264) → (Phase 6: network → decode → present).

mod capture;
mod encode;
mod stats;

pub use capture::{run_host_capture_loop, CaptureBackend, CaptureConfig, HostCaptureHandle};
pub use encode::{create_encoder, probe_encoders, EncoderSettings, VideoEncoder};
pub use stats::{AtomicCaptureStats, CaptureSnapshot};
