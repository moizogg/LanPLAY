//! Video pipeline (Phase 4–6).
//!
//! Capture → encode (H.264) → stream → decode → present.

mod capture;
mod decode;
mod encode;
#[cfg(windows)]
mod mf_h264;
mod nv12;
mod present;
mod settings;
mod stats;
mod stream;

pub use capture::{run_host_capture_loop, CaptureBackend, CaptureConfig, HostCaptureHandle};
pub use encode::{create_encoder, probe_encoders, EncoderSettings, VideoEncoder};
pub use settings::{
    list_encoder_options, resolution_presets, EncoderOption, ResolutionMode, ResolutionPreset,
    VideoSettings,
};

/// Human-readable hardware encoder probe (why software?).
pub fn hardware_encoder_probe() -> String {
    #[cfg(windows)]
    {
        let _ = crate::mf_h264::hardware_h264_available();
        crate::mf_h264::last_probe_detail()
    }
    #[cfg(not(windows))]
    {
        "Hardware encode only on Windows.".into()
    }
}
pub use stats::{AtomicCaptureStats, CaptureSnapshot};
pub use stream::{
    run_client_video_loop, ClientVideoHandle, ClientVideoSnapshot, VideoSenderHandle,
    VideoStreamSink,
};
