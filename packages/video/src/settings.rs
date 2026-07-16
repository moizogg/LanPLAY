//! Host video / encode settings (Sunshine-style knobs for Phase 5+).

use crate::capture::CaptureConfig;
use serde::{Deserialize, Serialize};

/// How encode resolution is chosen from the captured desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionMode {
    /// Scale so the long edge ≤ `max_edge` (keeps aspect).
    #[default]
    Auto,
    /// Exact encode size (`width` × `height`), scaled from capture.
    Fixed,
}

/// User-facing video encode settings (persisted + Settings tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSettings {
    /// DXGI output index (0 = primary).
    pub output_index: u32,
    /// Target encode FPS (capture may run faster).
    pub fps: u32,
    /// Target bitrate in kilobits per second.
    pub bitrate_kbps: u32,
    pub resolution_mode: ResolutionMode,
    /// Used when `resolution_mode == Auto`.
    pub max_edge: u32,
    /// Used when `resolution_mode == Fixed`.
    pub width: u32,
    pub height: u32,
    /// Encoder id: `openh264`, later `nvenc` / `amf` / `qsv` / `auto`.
    pub encoder: String,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            output_index: 0,
            // Prefer smooth over sharp until GPU zero-copy encode lands.
            // `auto` uses HW MF when present; software path clamps further (~960p30).
            fps: 30,
            bitrate_kbps: 8_000,
            resolution_mode: ResolutionMode::Auto,
            max_edge: 1280,
            width: 1280,
            height: 720,
            encoder: "auto".into(),
        }
    }
}

impl VideoSettings {
    /// Clamp to sane ranges (Sunshine-style safety rails).
    pub fn sanitize(mut self) -> Self {
        self.output_index = self.output_index.min(7);
        self.fps = self.fps.clamp(5, 240);
        self.bitrate_kbps = self.bitrate_kbps.clamp(500, 100_000);
        self.max_edge = self.max_edge.clamp(320, 3840) & !1;
        self.width = self.width.clamp(160, 3840) & !1;
        self.height = self.height.clamp(160, 2160) & !1;
        let enc = self.encoder.to_ascii_lowercase();
        self.encoder = match enc.as_str() {
            "auto" | "hw" | "hardware" | "ffmpeg" => "auto".into(),
            "openh264" | "software" => "openh264".into(),
            "nvenc" | "amf" | "qsv" | "mf" => enc,
            _ => "auto".into(),
        };
        self
    }

    pub fn to_capture_config(&self) -> CaptureConfig {
        let s = self.clone().sanitize();
        CaptureConfig {
            output_index: s.output_index,
            target_fps: s.fps,
            bitrate_bps: s.bitrate_kbps.saturating_mul(1000),
            encode_max_edge: s.max_edge,
            fixed_width: match s.resolution_mode {
                ResolutionMode::Fixed => Some(s.width),
                ResolutionMode::Auto => None,
            },
            fixed_height: match s.resolution_mode {
                ResolutionMode::Fixed => Some(s.height),
                ResolutionMode::Auto => None,
            },
            encoder_id: s.encoder,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderOption {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub hardware: bool,
    pub detail: String,
}

/// Encoders exposed in Settings.
pub fn list_encoder_options() -> Vec<EncoderOption> {
    let ff = crate::ffmpeg_enc::probe_ffmpeg_caps();
    #[cfg(windows)]
    let mf = crate::mf_h264::hardware_h264_available();
    #[cfg(not(windows))]
    let mf = false;
    let any_hw = ff.qsv || ff.nvenc || ff.amf || mf;

    vec![
        EncoderOption {
            id: "auto".into(),
            name: "Auto (Sunshine-class preferred)".into(),
            available: true,
            hardware: any_hw,
            detail: {
                let mut s = String::from("Order: FFmpeg QSV/NVENC/AMF → MF MFT → FFmpeg x264 → OpenH264. ");
                s.push_str(&ff.detail);
                s
            },
        },
        EncoderOption {
            id: "qsv".into(),
            name: "Intel QSV (FFmpeg h264_qsv)".into(),
            available: ff.qsv,
            hardware: true,
            detail: if ff.qsv {
                "Same Quick Sync path Sunshine uses on HD Graphics / Arc.".into()
            } else if ff.path.is_some() {
                "ffmpeg found but no h264_qsv in this build (need full/essentials with QSV).".into()
            } else {
                "ffmpeg not bundled — CI ships it, or set LANPLAY_FFMPEG.".into()
            },
        },
        EncoderOption {
            id: "nvenc".into(),
            name: "NVIDIA NVENC (FFmpeg)".into(),
            available: ff.nvenc || mf,
            hardware: true,
            detail: if ff.nvenc {
                "FFmpeg h264_nvenc ultra-low-latency.".into()
            } else if mf {
                "No ffmpeg NVENC — will try Media Foundation MFT.".into()
            } else {
                "NVENC not available.".into()
            },
        },
        EncoderOption {
            id: "amf".into(),
            name: "AMD AMF (FFmpeg)".into(),
            available: ff.amf || mf,
            hardware: true,
            detail: if ff.amf {
                "FFmpeg h264_amf ultralowlatency.".into()
            } else {
                "AMF not available via ffmpeg.".into()
            },
        },
        EncoderOption {
            id: "openh264".into(),
            name: "OpenH264 (software only)".into(),
            available: true,
            hardware: false,
            detail: "Forced CPU path — for debugging only.".into(),
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionPreset {
    pub id: String,
    pub label: String,
    pub mode: ResolutionMode,
    pub width: u32,
    pub height: u32,
    pub max_edge: u32,
}

pub fn resolution_presets() -> Vec<ResolutionPreset> {
    vec![
        ResolutionPreset {
            id: "auto-1920".into(),
            label: "Auto (max edge 1920) — sharp default".into(),
            mode: ResolutionMode::Auto,
            width: 1920,
            height: 1080,
            max_edge: 1920,
        },
        ResolutionPreset {
            id: "auto-1280".into(),
            label: "Auto (max edge 1280)".into(),
            mode: ResolutionMode::Auto,
            width: 1280,
            height: 720,
            max_edge: 1280,
        },
        ResolutionPreset {
            id: "auto-960".into(),
            label: "Auto (max edge 960) — lighter CPU".into(),
            mode: ResolutionMode::Auto,
            width: 960,
            height: 540,
            max_edge: 960,
        },
        ResolutionPreset {
            id: "720p".into(),
            label: "1280×720".into(),
            mode: ResolutionMode::Fixed,
            width: 1280,
            height: 720,
            max_edge: 1280,
        },
        ResolutionPreset {
            id: "1080p".into(),
            label: "1920×1080".into(),
            mode: ResolutionMode::Fixed,
            width: 1920,
            height: 1080,
            max_edge: 1920,
        },
        ResolutionPreset {
            id: "custom".into(),
            label: "Custom…".into(),
            mode: ResolutionMode::Fixed,
            width: 1920,
            height: 1080,
            max_edge: 1920,
        },
    ]
}
