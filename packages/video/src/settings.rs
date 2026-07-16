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
            // Prefer 1080p60 @ 25 Mbps; HW MF/NVENC when available via `auto`.
            fps: 60,
            bitrate_kbps: 25_000,
            resolution_mode: ResolutionMode::Auto,
            max_edge: 1920,
            width: 1920,
            height: 1080,
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
            "auto" | "hw" | "hardware" | "mf" => "auto".into(),
            "openh264" | "software" => "openh264".into(),
            "nvenc" | "amf" | "qsv" => enc,
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
    #[cfg(windows)]
    let hw = crate::mf_h264::hardware_h264_available();
    #[cfg(not(windows))]
    let hw = false;

    vec![
        EncoderOption {
            id: "auto".into(),
            name: "Auto (hardware preferred)".into(),
            available: true,
            hardware: hw,
            detail: if hw {
                "Uses Media Foundation HW H.264 (NVENC/AMF/QSV when present), else OpenH264."
                    .into()
            } else {
                "No HW H.264 MFT found — will use OpenH264 software.".into()
            },
        },
        EncoderOption {
            id: "nvenc".into(),
            name: "Hardware H.264 (NVENC/MF)".into(),
            available: hw,
            hardware: true,
            detail: if hw {
                "Hardware MFT encode — low latency path (driver NVENC/AMF/QSV)."
                    .into()
            } else {
                "No hardware H.264 encoder MFT detected on this PC.".into()
            },
        },
        EncoderOption {
            id: "openh264".into(),
            name: "OpenH264 (software)".into(),
            available: true,
            hardware: false,
            detail: "CPU H.264 — works everywhere; higher latency than HW.".into(),
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
