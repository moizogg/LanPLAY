//! Video pipeline (Phase 4–5).
//!
//! Capture → encode (H.264) → (Phase 6: network → decode → present).

mod capture;
mod encode;
mod settings;
mod stats;

pub use capture::{run_host_capture_loop, CaptureBackend, CaptureConfig, HostCaptureHandle};
pub use encode::{create_encoder, probe_encoders, EncoderSettings, VideoEncoder};
pub use settings::{
    list_encoder_options, resolution_presets, EncoderOption, ResolutionMode, ResolutionPreset,
    VideoSettings,
};
pub use stats::{AtomicCaptureStats, CaptureSnapshot};
