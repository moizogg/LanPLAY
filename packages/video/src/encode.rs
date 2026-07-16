//! H.264 encode backends.
//!
//! Priority (auto):
//! 1. **FFmpeg** `h264_qsv` / `h264_nvenc` / `h264_amf` — Sunshine-class HW
//! 2. **Media Foundation** HW MFT (when registered)
//! 3. **FFmpeg libx264** ultrafast+zerolatency (if ffmpeg present)
//! 4. **OpenH264** software soft profile

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
    /// `auto` | `nvenc` | `amf` | `qsv` | `openh264` | `hardware`
    pub encoder_id: String,
}

impl Default for EncoderSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_bps: 20_000_000,
            encoder_id: "auto".into(),
        }
    }
}

pub trait VideoEncoder: Send {
    fn name(&self) -> &str;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    /// Effective encode FPS after low-latency clamps (drives capture pacing).
    fn target_fps(&self) -> u32;
    fn encode_bgra(&mut self, bgra: &[u8], pts_us: u64) -> Result<Option<EncodedFrame>, String>;
    fn force_keyframe(&mut self);
}

pub fn probe_encoders() -> Vec<String> {
    let mut v = vec!["openh264 (software H.264)".into()];
    let caps = crate::ffmpeg_enc::probe_ffmpeg_caps();
    if caps.qsv {
        v.insert(0, "FFmpeg h264_qsv (Intel Quick Sync)".into());
    }
    if caps.nvenc {
        v.insert(0, "FFmpeg h264_nvenc".into());
    }
    if caps.amf {
        v.insert(0, "FFmpeg h264_amf".into());
    }
    if caps.x264 {
        v.push("FFmpeg libx264 ultrafast".into());
    }
    #[cfg(windows)]
    {
        if crate::mf_h264::hardware_h264_available() {
            v.push("Media Foundation HW H.264 MFT".into());
        }
    }
    v
}

/// Create encoder. Prefer FFmpeg HW (Sunshine path), then MF, then soft.
pub fn create_encoder(settings: EncoderSettings) -> Result<Box<dyn VideoEncoder>, String> {
    let id = settings.encoder_id.to_ascii_lowercase();
    let want_hw = matches!(
        id.as_str(),
        "auto" | "nvenc" | "amf" | "qsv" | "hardware" | "hw" | "mf" | "ffmpeg"
    );

    let mut errors: Vec<String> = Vec::new();

    // --- 1) FFmpeg hardware (QSV/NVENC/AMF) — same silicon Sunshine uses ---
    if want_hw {
        let prefer = match id.as_str() {
            "qsv" => Some(crate::ffmpeg_enc::FfmpegCodec::Qsv),
            "nvenc" => Some(crate::ffmpeg_enc::FfmpegCodec::Nvenc),
            "amf" => Some(crate::ffmpeg_enc::FfmpegCodec::Amf),
            "mf" => None, // skip ffmpeg HW, try MF only below
            _ => None,    // auto: try all HW codecs
        };
        if id != "mf" {
            match crate::ffmpeg_enc::try_create(settings.clone(), prefer) {
                Ok(e) => return Ok(Box::new(e)),
                Err(e) => errors.push(format!("ffmpeg-hw: {e}")),
            }
        }
    }

    // --- 2) Media Foundation HW MFT (Windows) ---
    #[cfg(windows)]
    {
        if want_hw && matches!(id.as_str(), "auto" | "hardware" | "hw" | "mf" | "nvenc" | "amf" | "qsv")
        {
            match crate::mf_h264::MfHardwareH264Encoder::new(settings.clone()) {
                Ok(e) => return Ok(Box::new(e)),
                Err(e) => errors.push(format!("mf: {e}")),
            }
        }
    }

    // Forced pure OpenH264
    if id == "openh264" || id == "software" {
        let soft = soft_low_latency_profile(settings);
        return OpenH264Encoder::new(soft).map(|e| Box::new(e) as Box<dyn VideoEncoder>);
    }

    // --- 3) FFmpeg libx264 ultrafast (much better realtime than OpenH264) ---
    {
        let soft = soft_low_latency_profile(settings.clone());
        match crate::ffmpeg_enc::try_create_x264(soft) {
            Ok(e) => return Ok(Box::new(e)),
            Err(e) => errors.push(format!("ffmpeg-x264: {e}")),
        }
    }

    // --- 4) OpenH264 soft profile last resort ---
    let mut soft = soft_low_latency_profile(settings);
    soft.encoder_id = "openh264-no-hw".into();
    let mut enc = OpenH264Encoder::new(soft)?;
    if !errors.is_empty() {
        let summary: String = errors.join(" · ").chars().take(120).collect();
        enc.name = format!("{} | {}", enc.name, summary);
    }
    Ok(Box::new(enc))
}

/// When HW encode is unavailable: survival profile so OpenH264 can keep up.
/// 1080p60 software is what made the stream feel broken vs Sunshine.
fn soft_low_latency_profile(mut s: EncoderSettings) -> EncoderSettings {
    let long = s.width.max(s.height);
    // ~960 long edge @ 30fps — weak CPUs can actually sustain this
    if long > 960 {
        let scale = 960.0 / long as f32;
        s.width = ((s.width as f32 * scale) as u32).max(2) & !1;
        s.height = ((s.height as f32 * scale) as u32).max(2) & !1;
    }
    s.fps = s.fps.min(30);
    s.bitrate_bps = s.bitrate_bps.min(6_000_000).max(1_500_000);
    s
}

struct OpenH264Encoder {
    encoder: Encoder,
    width: u32,
    height: u32,
    fps: u32,
    force_idr: bool,
    ready: bool,
    pub(crate) name: String,
}

impl OpenH264Encoder {
    fn new(settings: EncoderSettings) -> Result<Self, String> {
        let w = settings.width.max(16) & !1;
        let h = settings.height.max(16) & !1;
        let fps = settings.fps.max(1);

        let api = OpenH264API::from_source();
        // Realtime screen: allow frame skip under load (keeps latency from exploding).
        let cfg = EncoderConfig::new()
            .max_frame_rate(fps as f32)
            .set_bitrate_bps(settings.bitrate_bps.max(1_500_000))
            .usage_type(UsageType::ScreenContentRealTime)
            .enable_skip_frame(true)
            .set_multiple_thread_idc(0);

        let encoder = Encoder::with_api_config(api, cfg)
            .map_err(|e| format!("OpenH264 init failed: {e:?}"))?;

        Ok(Self {
            encoder,
            width: w,
            height: h,
            fps,
            force_idr: false,
            ready: false,
            name: if settings.encoder_id.contains("no-hw") {
                format!(
                    "openh264 {}x{}@{} soft (no HW — Sunshine uses QSV/NVENC on GPU)",
                    w, h, fps
                )
            } else {
                format!("openh264 {}x{}@{} software", w, h, fps)
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
    fn target_fps(&self) -> u32 {
        self.fps
    }
    fn force_keyframe(&mut self) {
        self.force_idr = true;
    }

    fn encode_bgra(&mut self, bgra: &[u8], pts_us: u64) -> Result<Option<EncodedFrame>, String> {
        let w = self.width as usize;
        let h = self.height as usize;
        let expected = w * h * 4;
        if bgra.len() < expected {
            return Err(format!("BGRA buffer too small: {} < {expected}", bgra.len()));
        }

        let slice = BgraSliceU8::new(&bgra[..expected], (w, h));
        let yuv = YUVBuffer::from_rgb_source(slice);

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

/// Fast nearest-neighbor scale for the encode hot path (latency over polish).
/// Bilinear is nicer but costs several ms on weak CPUs — that is pure glass-to-glass lag.
pub fn scale_bgra_nn(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    scale_bgra_nearest(src, src_w, src_h, dst_w, dst_h)
}

/// Nearest-neighbor BGRA scale (even dest). Used on the stream encode path.
pub fn scale_bgra_nearest(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
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
    for y in 0..dst_h {
        let sy = (y as u64 * src_h as u64 / dst_h as u64) as u32;
        let sy = sy.min(src_h - 1);
        for x in 0..dst_w {
            let sx = (x as u64 * src_w as u64 / dst_w as u64) as u32;
            let sx = sx.min(src_w - 1);
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((y * dst_w + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

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

/// Cap long edge for encode size selection.
pub fn choose_encode_size(cap_w: u32, cap_h: u32, max_edge: u32) -> (u32, u32) {
    if cap_w == 0 || cap_h == 0 {
        return (1920, 1080);
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
        assert_eq!((w, h), (1280, 720));
    }
}
