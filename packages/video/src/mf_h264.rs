//! Windows Media Foundation **hardware** H.264 encoder (NVENC / AMF / **Intel QSV**).
//!
//! Enumerates vendor MFTs the same class of silicon Sunshine uses. On HD Graphics
//! 4000 that means **Quick Sync** when the Intel driver registers the encoder MFT.

#![cfg(windows)]

use crate::encode::{EncodedFrame, EncoderSettings, VideoEncoder};
use crate::nv12::bgra_to_nv12;
use std::sync::Once;
use windows::core::{Interface, GUID, VARIANT};
use windows::Win32::Media::MediaFoundation::{
    ICodecAPI, IMFActivate, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFSampleExtension_CleanPoint, MFStartup, MFTEnumEx,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_FRIENDLY_NAME_Attribute,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_INFO, MFT_REGISTER_TYPE_INFO, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MF_API_VERSION, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFSTARTUP_NOSOCKET,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};

static MF_INIT: Once = Once::new();
static mut MF_OK: bool = false;
static LAST_PROBE: std::sync::Mutex<String> =
    std::sync::Mutex::new(String::new());

fn me(e: windows::core::Error) -> String {
    e.to_string()
}

fn ensure_mf() -> Result<(), String> {
    MF_INIT.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        match MFStartup(MF_API_VERSION, MFSTARTUP_NOSOCKET) {
            Ok(()) => MF_OK = true,
            Err(e) => {
                MF_OK = false;
                if let Ok(mut g) = LAST_PROBE.lock() {
                    *g = format!("MFStartup failed: {e}");
                }
            }
        }
    });
    if unsafe { MF_OK } {
        Ok(())
    } else {
        Err(LAST_PROBE
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "Media Foundation startup failed".into()))
    }
}

/// Last probe detail for UI (“why am I on software?”).
pub fn last_probe_detail() -> String {
    LAST_PROBE
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

pub fn hardware_h264_available() -> bool {
    match list_hw_h264_names() {
        Ok(names) if !names.is_empty() => {
            if let Ok(mut g) = LAST_PROBE.lock() {
                *g = format!("HW H.264 MFT(s): {}", names.join(" | "));
            }
            true
        }
        Ok(_) => {
            if let Ok(mut g) = LAST_PROBE.lock() {
                *g = "No hardware H.264 MFT registered (update Intel/NVIDIA/AMD video drivers)."
                    .into();
            }
            false
        }
        Err(e) => {
            if let Ok(mut g) = LAST_PROBE.lock() {
                *g = e;
            }
            false
        }
    }
}

fn activate_friendly_name(act: &IMFActivate) -> String {
    unsafe {
        let mut pwstr = windows::core::PWSTR::null();
        let mut len = 0u32;
        if act
            .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut pwstr, &mut len)
            .is_ok()
            && !pwstr.is_null()
        {
            let s = pwstr.to_string().unwrap_or_default();
            let _ = windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as *const _));
            if !s.is_empty() {
                return s;
            }
        }
    }
    "Hardware H.264 MFT".into()
}

fn looks_like_hw_encoder(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("intel")
        || n.contains("quick sync")
        || n.contains("qsv")
        || n.contains("nvidia")
        || n.contains("nvenc")
        || n.contains("amd")
        || n.contains("amf")
        || n.contains("hardware")
        || n.contains("hwe")
        || n.contains("mft") && (n.contains("h264") || n.contains("h.264") || n.contains("avc"))
}

/// Enumerate HW H.264 encoder friendly names (for probe + pick).
pub fn list_hw_h264_names() -> Result<Vec<String>, String> {
    ensure_mf()?;
    let mut names = Vec::new();
    // Try several flag combos — Intel HD 4000 era drivers are picky.
    // Intel HD 4000-era drivers are picky; also try LOCALMFT / ALL-style combos.
    // Sunshine uses FFmpeg h264_qsv (Media SDK), not MF — so MF may still be empty
    // even when QSV works in Sunshine.
    let flag_sets = [
        MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0,
        MFT_ENUM_FLAG_HARDWARE.0
            | MFT_ENUM_FLAG_SYNCMFT.0
            | MFT_ENUM_FLAG_ASYNCMFT.0
            | MFT_ENUM_FLAG_SORTANDFILTER.0,
        MFT_ENUM_FLAG_SYNCMFT.0
            | MFT_ENUM_FLAG_ASYNCMFT.0
            | MFT_ENUM_FLAG_HARDWARE.0
            | 0x10 // MFT_ENUM_FLAG_LOCALMFT
            | MFT_ENUM_FLAG_SORTANDFILTER.0,
        MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_ASYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0,
        0x3F, // MFT_ENUM_FLAG_ALL
    ];

    unsafe {
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };

        for flags in flag_sets {
            let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0u32;
            if MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                windows::Win32::Media::MediaFoundation::MFT_ENUM_FLAG(flags),
                None,
                Some(&output as *const _),
                &mut activates,
                &mut count,
            )
            .is_err()
                || count == 0
                || activates.is_null()
            {
                continue;
            }

            for i in 0..count as isize {
                let slot = activates.offset(i);
                if let Some(act) = (*slot).take() {
                    let name = activate_friendly_name(&act);
                    // For non-HARDWARE-only enum, filter by name heuristics
                    let hardware_enum = (flags & MFT_ENUM_FLAG_HARDWARE.0) != 0;
                    if hardware_enum || looks_like_hw_encoder(&name) {
                        if !names.iter().any(|n| n == &name) {
                            names.push(name);
                        }
                    }
                    // drop act
                }
            }
            CoTaskMemFree(Some(activates as *const _));
            if !names.is_empty() {
                break;
            }
        }
    }

    if names.is_empty() {
        if let Ok(mut g) = LAST_PROBE.lock() {
            *g = "No HW H.264 MFT (Intel HD 4000 often has QSV via Media SDK/FFmpeg, not MF — Sunshine uses that path). Falling back to software."
                .into();
        }
    }

    Ok(names)
}

fn find_hw_h264_activate() -> Result<(IMFActivate, String), String> {
    ensure_mf()?;
    unsafe {
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let flag_sets = [
            MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0,
            MFT_ENUM_FLAG_HARDWARE.0
                | MFT_ENUM_FLAG_SYNCMFT.0
                | MFT_ENUM_FLAG_ASYNCMFT.0
                | MFT_ENUM_FLAG_SORTANDFILTER.0,
            MFT_ENUM_FLAG_SYNCMFT.0
                | MFT_ENUM_FLAG_ASYNCMFT.0
                | MFT_ENUM_FLAG_HARDWARE.0
                | 0x10
                | MFT_ENUM_FLAG_SORTANDFILTER.0,
            MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_ASYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0,
            0x3F,
        ];

        let mut last_err = "No H.264 encoder MFT found".to_string();
        for flags in flag_sets {
            let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0u32;
            match MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                windows::Win32::Media::MediaFoundation::MFT_ENUM_FLAG(flags),
                None,
                Some(&output as *const _),
                &mut activates,
                &mut count,
            ) {
                Ok(()) if count > 0 && !activates.is_null() => {
                    let hardware_enum = (flags & MFT_ENUM_FLAG_HARDWARE.0) != 0;
                    let mut chosen: Option<(IMFActivate, String)> = None;
                    for i in 0..count as isize {
                        let slot = activates.offset(i);
                        if let Some(act) = (*slot).take() {
                            let name = activate_friendly_name(&act);
                            let prefer = hardware_enum
                                || looks_like_hw_encoder(&name)
                                || name.to_ascii_lowercase().contains("intel");
                            // Prefer Intel/NVIDIA/AMD names over generic software CMSH264
                            let is_soft_ms = name.to_ascii_lowercase().contains("microsoft")
                                && !name.to_ascii_lowercase().contains("hardware");
                            if prefer && !is_soft_ms && chosen.is_none() {
                                chosen = Some((act, name));
                            } else if prefer
                                && !is_soft_ms
                                && name.to_ascii_lowercase().contains("intel")
                            {
                                chosen = Some((act, name));
                            }
                            // else drop
                        }
                    }
                    CoTaskMemFree(Some(activates as *const _));
                    if let Some(c) = chosen {
                        return Ok(c);
                    }
                    last_err = format!("MFT enum flags=0x{flags:x} returned {count} but none preferred");
                }
                Ok(()) => {
                    last_err = format!("MFT enum flags=0x{flags:x}: empty");
                }
                Err(e) => {
                    last_err = format!("MFTEnumEx 0x{flags:x}: {e}");
                }
            }
        }
        Err(last_err)
    }
}

fn pack_frame_size(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | (h as u64)
}
fn pack_frame_rate(num: u32, den: u32) -> u64 {
    ((num as u64) << 32) | (den as u64)
}

fn guid_low_latency() -> GUID {
    GUID::from_u128(0x9c27891a_ed7a_40e1_88e8_d25a0b5c36e6)
}
fn guid_mean_bitrate() -> GUID {
    GUID::from_u128(0xf7222374_2144_4815_b550_a37f8e12ee52)
}
fn guid_rc_mode() -> GUID {
    GUID::from_u128(0x1c0608e9_370c_4710_8a58_cb6181c42423)
}
fn guid_force_kf() -> GUID {
    GUID::from_u128(0x398c1b98_8353_475a_9ef2_8f265d260345)
}

unsafe fn codec_set_u32(api: &ICodecAPI, g: &GUID, v: u32) {
    let var = VARIANT::from(v as i32);
    let _ = api.SetValue(g, &var);
}
unsafe fn codec_set_bool(api: &ICodecAPI, g: &GUID, v: bool) {
    let var = VARIANT::from(v);
    let _ = api.SetValue(g, &var);
}

pub struct MfHardwareH264Encoder {
    transform: IMFTransform,
    width: u32,
    height: u32,
    fps: u32,
    force_idr: bool,
    name: String,
}

// COM pointer used only from encode thread.
unsafe impl Send for MfHardwareH264Encoder {}

impl MfHardwareH264Encoder {
    pub fn new(settings: EncoderSettings) -> Result<Self, String> {
        ensure_mf()?;
        let w = settings.width.max(16) & !1;
        let h = settings.height.max(16) & !1;
        // Cap ancient iGPUs (HD 4000 era): QSV struggles above 720p60
        let (w, h, fps) = soften_for_old_igpu(w, h, settings.fps.max(1));
        let bitrate = settings.bitrate_bps.max(1_000_000).min(15_000_000);

        let (activate, friendly) = find_hw_h264_activate().map_err(|e| {
            if let Ok(mut g) = LAST_PROBE.lock() {
                *g = e.clone();
            }
            e
        })?;

        let transform: IMFTransform = unsafe {
            activate
                .ActivateObject()
                .map_err(|e| format!("ActivateObject ({friendly}): {e}"))?
        };

        unsafe {
            let out_ty = MFCreateMediaType().map_err(me)?;
            out_ty
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(me)?;
            out_ty
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(me)?;
            out_ty
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(w, h))
                .map_err(me)?;
            out_ty
                .SetUINT64(&MF_MT_FRAME_RATE, pack_frame_rate(fps, 1))
                .map_err(me)?;
            out_ty.SetUINT32(&MF_MT_AVG_BITRATE, bitrate).map_err(me)?;
            out_ty
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(me)?;
            out_ty
                .SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 0)
                .map_err(me)?;
            transform
                .SetOutputType(0, &out_ty, 0)
                .map_err(|e| format!("SetOutputType H264 ({friendly}): {e}"))?;

            let in_ty = MFCreateMediaType().map_err(me)?;
            in_ty
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(me)?;
            in_ty
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
                .map_err(me)?;
            in_ty
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(w, h))
                .map_err(me)?;
            in_ty
                .SetUINT64(&MF_MT_FRAME_RATE, pack_frame_rate(fps, 1))
                .map_err(me)?;
            in_ty
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(me)?;
            in_ty.SetUINT32(&MF_MT_DEFAULT_STRIDE, w).map_err(me)?;
            transform
                .SetInputType(0, &in_ty, 0)
                .map_err(|e| format!("SetInputType NV12 ({friendly}): {e}"))?;

            if let Ok(api) = transform.cast::<ICodecAPI>() {
                codec_set_bool(&api, &guid_low_latency(), true);
                codec_set_u32(&api, &guid_mean_bitrate(), bitrate);
                codec_set_u32(&api, &guid_rc_mode(), 1); // CBR
            }

            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|e| format!("BEGIN_STREAMING: {e}"))?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|e| format!("START_OF_STREAM: {e}"))?;
        }

        if let Ok(mut g) = LAST_PROBE.lock() {
            *g = format!("Using {friendly} @ {w}x{h}p{fps}");
        }

        Ok(Self {
            transform,
            width: w,
            height: h,
            fps,
            force_idr: true,
            name: format!("{friendly} {w}x{h}@{fps} HW"),
        })
    }

    unsafe fn drain_output(&mut self) -> Result<(Vec<u8>, bool), String> {
        let mut bitstream = Vec::new();
        let mut keyframe = false;

        loop {
            let info = self.transform.GetOutputStreamInfo(0).map_err(me)?;
            let out_size = info.cbSize.max(4096);
            let out_buf = MFCreateMemoryBuffer(out_size).map_err(me)?;
            let out_sample = MFCreateSample().map_err(me)?;
            out_sample.AddBuffer(&out_buf).map_err(me)?;

            let mut outs = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(Some(out_sample)),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            match self.transform.ProcessOutput(0, &mut outs, &mut status) {
                Ok(()) => {
                    if let Some(s) = outs[0].pSample.as_ref() {
                        if s.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0 {
                            keyframe = true;
                        }
                        let buf = s.ConvertToContiguousBuffer().map_err(me)?;
                        let mut p: *mut u8 = std::ptr::null_mut();
                        let mut len = 0u32;
                        buf.Lock(&mut p, None, Some(&mut len)).map_err(me)?;
                        if !p.is_null() && len > 0 {
                            bitstream
                                .extend_from_slice(std::slice::from_raw_parts(p, len as usize));
                        }
                        let _ = buf.Unlock();
                    }
                    let _ = std::mem::ManuallyDrop::take(&mut outs[0].pSample);
                    let _ = std::mem::ManuallyDrop::take(&mut outs[0].pEvents);
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    let _ = std::mem::ManuallyDrop::take(&mut outs[0].pSample);
                    let _ = std::mem::ManuallyDrop::take(&mut outs[0].pEvents);
                    break;
                }
                Err(e) => {
                    let _ = std::mem::ManuallyDrop::take(&mut outs[0].pSample);
                    let _ = std::mem::ManuallyDrop::take(&mut outs[0].pEvents);
                    return Err(format!("ProcessOutput: {e}"));
                }
            }
        }
        Ok((bitstream, keyframe))
    }
}

/// HD 4000 / weak iGPU: keep QSV in a sweet spot (Sunshine still does this class of clamp).
fn soften_for_old_igpu(w: u32, h: u32, fps: u32) -> (u32, u32, u32) {
    let long = w.max(h);
    // Cap 1280 long edge and 30 fps for ancient QSV unless already smaller
    let (w, h) = if long > 1280 {
        let s = 1280.0 / long as f32;
        (
            ((w as f32 * s) as u32).max(2) & !1,
            ((h as f32 * s) as u32).max(2) & !1,
        )
    } else {
        (w & !1, h & !1)
    };
    (w, h, fps.min(30))
}

impl VideoEncoder for MfHardwareH264Encoder {
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
        let nv12 = bgra_to_nv12(bgra, self.width, self.height)?;

        unsafe {
            if self.force_idr {
                if let Ok(api) = self.transform.cast::<ICodecAPI>() {
                    codec_set_u32(&api, &guid_force_kf(), 1);
                }
                self.force_idr = false;
            }

            let buffer = MFCreateMemoryBuffer(nv12.len() as u32).map_err(me)?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut max_len = 0u32;
            buffer
                .Lock(&mut ptr, Some(&mut max_len), None)
                .map_err(me)?;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(
                    nv12.as_ptr(),
                    ptr,
                    nv12.len().min(max_len as usize),
                );
            }
            let _ = buffer.Unlock();
            buffer
                .SetCurrentLength(nv12.len() as u32)
                .map_err(me)?;

            let sample = MFCreateSample().map_err(me)?;
            sample.AddBuffer(&buffer).map_err(me)?;
            sample
                .SetSampleTime((pts_us as i64).saturating_mul(10))
                .map_err(me)?;
            let _ = sample.SetSampleDuration(10_000_000 / 30);

            self.transform
                .ProcessInput(0, &sample, 0)
                .map_err(|e| format!("ProcessInput: {e}"))?;

            let (bitstream, keyframe) = self.drain_output()?;
            if bitstream.is_empty() {
                return Ok(None);
            }
            Ok(Some(EncodedFrame {
                data: bitstream,
                keyframe,
                pts_us,
            }))
        }
    }
}

impl Drop for MfHardwareH264Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}
