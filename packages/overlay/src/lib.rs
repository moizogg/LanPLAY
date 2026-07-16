//! Overlay metrics model (populated in later phases).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStats {
    pub fps: f32,
    pub rtt_ms: f32,
    pub bitrate_kbps: f32,
    pub input_latency_ms: f32,
    pub packet_loss_pct: f32,
}
