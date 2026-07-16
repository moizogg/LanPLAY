//! Video pipeline (Phase 4–6).
//!
//! Capture → encode (H.264) → stream → decode → present.

mod capture;
#[cfg(windows)]
mod d3d11_gpu;
mod decode;
mod encode;
mod ffmpeg_enc;
#[cfg(windows)]
mod mf_h264;
mod nv12;
mod present;
mod settings;
mod stats;
mod stream;

pub use capture::{run_host_capture_loop, CaptureBackend, CaptureConfig, HostCaptureHandle};
pub use encode::{create_encoder, probe_encoders, EncoderSettings, VideoEncoder};
pub use ffmpeg_enc::configure_ffmpeg_search_paths;
pub use settings::{
    list_encoder_options, resolution_presets, EncoderOption, ResolutionMode, ResolutionPreset,
    VideoSettings,
};

/// Human-readable hardware encoder probe (why software?).
pub fn hardware_encoder_probe() -> String {
    let ff = crate::ffmpeg_enc::probe_ffmpeg_caps();
    let mut parts = vec![ff.detail];
    #[cfg(windows)]
    {
        let _ = crate::mf_h264::hardware_h264_available();
        let mf = crate::mf_h264::last_probe_detail();
        if !mf.is_empty() {
            parts.push(format!("MF: {mf}"));
        }
    }
    let live = crate::ffmpeg_enc::last_ffmpeg_probe();
    if !live.is_empty() && !parts.iter().any(|p| p == &live) {
        parts.push(live);
    }
    parts.join(" · ")
}
pub use stats::{AtomicCaptureStats, CaptureSnapshot};
pub use stream::{
    run_client_video_loop, ClientVideoHandle, ClientVideoSnapshot, VideoSenderHandle,
    VideoStreamSink,
};
