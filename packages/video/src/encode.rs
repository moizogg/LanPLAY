//! H.264 encode backends (Phase 5).
//!
//! Working path: OpenH264 (software) for pipeline validation.
//! Trait is replaceable for NVENC / AMF / QSV later.

use openh264::encoder::{Encoder, EncoderConfig, FrameType, UsageType};
use openh264::formats::{BgraSliceU8, YUVBuffer};
use openh264::OpenH264API;

/// One encoded access unit (Annex-B NAL units).
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub pts_us: u64,
}

#[derive(Debug, Clone)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    /// Preferred encoder (`openh264`; HW ids fall back until implemented).
    pub encoder_id: String,
}

impl Default for EncoderSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_bps: 8_000_000,
            encoder_id: "openh264".into(),
        }
    }
}

pub trait VideoEncoder: Send {
    fn name(&self) -> &str;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn encode_bgra(&mut self, bgra: &[u8], pts_us: u64) -> Result<Option<EncodedFrame>, String>;
    fn force_keyframe(&mut self);
}

pub fn probe_encoders() -> Vec<String> {
    vec!["openh264 (software H.264)".into()]
}

pub fn create_encoder(settings: EncoderSettings) -> Result<Box<dyn VideoEncoder>, String> {
    // HW paths (nvenc/amf/qsv) will branch here; until then OpenH264 is the working path.
    let id = settings.encoder_id.to_ascii_lowercase();
    let mut soft = settings;
    if matches!(id.as_str(), "nvenc" | "amf" | "qsv") {
        // Not implemented yet — fall back so Host still works with a HW pick in Settings.
        soft.encoder_id = format!("openh264-fallback-{id}");
    }
    OpenH264Encoder::new(soft).map(|e| Box::new(e) as Box<dyn VideoEncoder>)
}

struct OpenH264Encoder {
    encoder: Encoder,
    width: u32,
    height: u32,
    /// Request IDR on next encode (only after encoder has been initialized).
    force_idr: bool,
    /// True after at least one successful encode (OpenH264 initialized).
    ready: bool,
    name: String,
}

impl OpenH264Encoder {
    fn new(settings: EncoderSettings) -> Result<Self, String> {
        // Even dimensions required by OpenH264 / YUV420.
        let w = settings.width.max(16) & !1;
        let h = settings.height.max(16) & !1;

        let api = OpenH264API::from_source();
        // openh264 0.6: resolution is taken from the first YUV frame on encode.
        // Higher bitrate floor + no frame skip → much less “144p mush” on screen content.
        let cfg = EncoderConfig::new()
            .max_frame_rate(settings.fps.max(1) as f32)
            .set_bitrate_bps(settings.bitrate_bps.max(2_000_000))
            .usage_type(UsageType::ScreenContentRealTime)
            .enable_skip_frame(false)
            .set_multiple_thread_idc(0); // auto threads

        let encoder = Encoder::with_api_config(api, cfg)
            .map_err(|e| format!("OpenH264 init failed: {e:?}"))?;

        Ok(Self {
            encoder,
            width: w,
            height: h,
            force_idr: false,
            ready: false,
            name: if settings.encoder_id.starts_with("openh264-fallback-") {
                let hw = settings
                    .encoder_id
                    .strip_prefix("openh264-fallback-")
                    .unwrap_or("hw");
                format!("openh264 {}x{}@{} (fallback; {hw} not ready)", w, h, settings.fps)
            } else {
                format!("openh264 {}x{}@{} software", w, h, settings.fps)
            },
        })
    }
}

impl VideoEncoder for OpenH264Encoder {
    fn name(&self) -> &str {
        &self.name
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn force_keyframe(&mut self) {
        self.force_idr = true;
    }

    fn encode_bgra(&mut self, bgra: &[u8], pts_us: u64) -> Result<Option<EncodedFrame>, String> {
        let w = self.width as usize;
        let h = self.height as usize;
        let expected = w * h * 4;
        if bgra.len() < expected {
            return Err(format!(
                "BGRA buffer too small: {} < {expected}",
                bgra.len()
            ));
        }

        // Tight BGRA8 → YUV I420 (openh264 formats).
        let slice = BgraSliceU8::new(&bgra[..expected], (w, h));
        let yuv = YUVBuffer::from_rgb_source(slice);

        // ForceIntraFrame only after the encoder has been initialized by the first encode.
        // First frame already gets an IDR via internal reinit.
        if self.force_idr && self.ready {
            self.encoder.force_intra_frame();
            self.force_idr = false;
        }

        let bitstream = self
            .encoder
            .encode(&yuv)
            .map_err(|e| format!("openh264 encode: {e:?}"))?;
        self.ready = true;

        let keyframe = matches!(
            bitstream.frame_type(),
            FrameType::IDR | FrameType::I | FrameType::IPMixed
        );
        let raw = bitstream.to_vec();
        if raw.is_empty() {
            return Ok(None);
        }

        Ok(Some(EncodedFrame {
            data: raw,
            keyframe,
            pts_us,
        }))
    }
}

/// Bilinear scale BGRA → even destination (much sharper than nearest-neighbor).
pub fn scale_bgra_nn(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    // Keep name for call sites; implementation is bilinear.
    scale_bgra_bilinear(src, src_w, src_h, dst_w, dst_h)
}

/// Bilinear scale BGRA8 tightly packed → even destination size.
pub fn scale_bgra_bilinear(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let dst_w = dst_w.max(2) & !1;
    let dst_h = dst_h.max(2) & !1;
    let mut out = vec![0u8; (dst_w * dst_h * 4) as usize];
    if src_w == 0 || src_h == 0 || src.len() < (src_w * src_h * 4) as usize {
        return out;
    }
    if src_w == dst_w && src_h == dst_h {
        let n = out.len().min(src.len());
        out[..n].copy_from_slice(&src[..n]);
        return out;
    }
    let x_ratio = (src_w.saturating_sub(1)) as f32 / dst_w.max(1) as f32;
    let y_ratio = (src_h.saturating_sub(1)) as f32 / dst_h.max(1) as f32;
    for y in 0..dst_h {
        let fy = y as f32 * y_ratio;
        let y0 = fy as u32;
        let y1 = (y0 + 1).min(src_h - 1);
        let wy = fy - y0 as f32;
        for x in 0..dst_w {
            let fx = x as f32 * x_ratio;
            let x0 = fx as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let wx = fx - x0 as f32;
            let di = ((y * dst_w + x) * 4) as usize;
            for c in 0usize..4 {
                let i00 = ((y0 * src_w + x0) * 4) as usize + c;
                let i10 = ((y0 * src_w + x1) * 4) as usize + c;
                let i01 = ((y1 * src_w + x0) * 4) as usize + c;
                let i11 = ((y1 * src_w + x1) * 4) as usize + c;
                let top = src[i00] as f32 * (1.0 - wx) + src[i10] as f32 * wx;
                let bot = src[i01] as f32 * (1.0 - wx) + src[i11] as f32 * wx;
                out[di + c] = (top * (1.0 - wy) + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Cap long edge for software encode CPU budget (still look decent).
pub fn choose_encode_size(cap_w: u32, cap_h: u32, max_edge: u32) -> (u32, u32) {
    if cap_w == 0 || cap_h == 0 {
        return (1280, 720);
    }
    let max_edge = max_edge.max(320);
    let long = cap_w.max(cap_h);
    if long <= max_edge {
        return (cap_w & !1, cap_h & !1);
    }
    let scale = max_edge as f32 / long as f32;
    let w = ((cap_w as f32 * scale) as u32).max(2) & !1;
    let h = ((cap_h as f32 * scale) as u32).max(2) & !1;
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_encode_size_caps_long_edge() {
        let (w, h) = choose_encode_size(1920, 1080, 1280);
        assert!(w <= 1280 && h <= 1280);
        assert_eq!(w % 2, 0);
        assert_eq!(h % 2, 0);
        // 1920 is long edge → 1280x720
        assert_eq!((w, h), (1280, 720));
    }

    #[test]
    fn scale_even_dims() {
        let src = vec![0u8; 4 * 4 * 4];
        let out = scale_bgra_nn(&src, 4, 4, 2, 2);
        assert_eq!(out.len(), 2 * 2 * 4);
    }
}
