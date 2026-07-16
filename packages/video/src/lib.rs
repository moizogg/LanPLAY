//! Video pipeline (Phase 4+).
//!
//! Phase 4: desktop capture + FPS/timing (no network yet).
//! Later: encode → stream → decode → present.

mod capture;
mod stats;

pub use capture::{run_host_capture_loop, CaptureBackend, CaptureConfig, HostCaptureHandle};
pub use stats::{AtomicCaptureStats, CaptureSnapshot};
