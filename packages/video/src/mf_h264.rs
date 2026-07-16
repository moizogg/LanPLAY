//! Windows Media Foundation **hardware** H.264 encoder.
//!
//! Enumerates `MFT_ENUM_FLAG_HARDWARE` H.264 encoders — on real machines this is
//! typically **NVENC / AMF / QSV** via the vendor MFT (Sunshine uses native SDKs;
//! MF is the portable Windows entry that still hits the same silicon).
//!
//! Low-latency knobs via `ICodecAPI` when supported. Falls through to caller if
//! no HW MFT exists (OpenH264 remains available).

#![cfg(windows)]

use crate::encode::{EncodedFrame, EncoderSettings, VideoEncoder};
use crate::nv12::bgra_to_nv12;
use std::sync::Once;
use windows::core::{Interface, GUID, VARIANT};
use windows::Win32::Media::MediaFoundation::{
    ICodecAPI, IMFActivate, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFSampleExtension_CleanPoint, MFStartup, MFTEnumEx,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_FRIENDLY_NAME_Attribute, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_INFO, MFT_REGISTER_TYPE_INFO, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MF_API_VERSION, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFSTARTUP_NOSOCKET,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};

static MF_INIT: Once = Once::new();
static mut MF_OK: bool = false;

fn ensure_mf() -> Result<(), String> {
    MF_INIT.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        match MFStartup(MF_API_VERSION, MFSTARTUP_NOSOCKET) {
            Ok(()) => MF_OK = true,
            Err(_) => MF_OK = false,
        }
    });
    // SAFETY: written once in call_once
    if unsafe { MF_OK } {
        Ok(())
    } else {
        Err("Media Foundation startup failed".into())
    }
}

/// True if a hardware H.264 encoder MFT is registered (NVENC/AMF/QSV class).
pub fn hardware_h264_available() -> bool {
    find_hw_h264_activate().is_ok()
}

fn find_hw_h264_activate() -> Result<IMFActivate, String> {
    ensure_mf()?;
    unsafe {
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            None,
            Some(&output as *const _),
            &mut activates,
            &mut count,
        )
        .map_err(|e| format!("MFTEnumEx HW H264: {e}"))?;

        if count == 0 || activates.is_null() {
            return Err(
                "No hardware H.264 encoder MFT (need NVIDIA NVENC / AMD AMF / Intel QSV)"
                    .into(),
            );
        }

        let first = (*activates)
            .clone()
            .ok_or_else(|| "null IMFActivate".to_string())?;

        // Free MFTEnumEx array (docs: CoTaskMemFree the pointer array; each activate released by drop)
        for i in 1..count as isize {
            let slot = activates.offset(i);
            // drop extras by taking
            let _ = (*slot).take();
        }
        CoTaskMemFree(Some(activates as *const _));
        Ok(first)
    }
}

fn pack_frame_size(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | (h as u64)
}
fn pack_frame_rate(num: u32, den: u32) -> u64 {
    ((num as u64) << 32) | (den as u64)
}

// codecapi.h GUIDs
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
    force_idr: bool,
    name: String,
}

impl MfHardwareH264Encoder {
    pub fn new(settings: EncoderSettings) -> Result<Self, String> {
        ensure_mf()?;
        let w = settings.width.max(16) & !1;
        let h = settings.height.max(16) & !1;
        let fps = settings.fps.max(1);
        let bitrate = settings.bitrate_bps.max(1_000_000);

        let activate = find_hw_h264_activate()?;
        let friendly = "Hardware H.264 (MF/NVENC/AMF/QSV)".to_string();
        let _ = MFT_FRIENDLY_NAME_Attribute; // available for future name probe

        let transform: IMFTransform = unsafe {
            activate
                .ActivateObject()
                .map_err(|e| format!("ActivateObject: {e}"))?
        };

        unsafe {
            // --- Output H.264 ---
            let out_ty = MFCreateMediaType().map_err(|e| e.to_string())?;
            out_ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out_ty.SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(w, h))?;
            out_ty.SetUINT64(&MF_MT_FRAME_RATE, pack_frame_rate(fps, 1))?;
            out_ty.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            out_ty.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out_ty.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 0)?;
            transform
                .SetOutputType(0, &out_ty, 0)
                .map_err(|e| format!("SetOutputType: {e}"))?;

            // --- Input NV12 ---
            let in_ty = MFCreateMediaType().map_err(|e| e.to_string())?;
            in_ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            in_ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            in_ty.SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(w, h))?;
            in_ty.SetUINT64(&MF_MT_FRAME_RATE, pack_frame_rate(fps, 1))?;
            in_ty.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            in_ty.SetUINT32(&MF_MT_DEFAULT_STRIDE, w)?;
            transform
                .SetInputType(0, &in_ty, 0)
                .map_err(|e| format!("SetInputType NV12: {e}"))?;

            // Sunshine-style low latency where the MFT exposes ICodecAPI
            if let Ok(api) = transform.cast::<ICodecAPI>() {
                codec_set_bool(&api, &guid_low_latency(), true);
                codec_set_u32(&api, &guid_mean_bitrate(), bitrate);
                // eAVEncCommonRateControlMode_CBR = 1
                codec_set_u32(&api, &guid_rc_mode(), 1);
            }

            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }

        Ok(Self {
            transform,
            width: w,
            height: h,
            force_idr: true,
            name: format!("{friendly} {w}x{h}@{fps} HW-MF"),
        })
    }

    unsafe fn drain_output(&mut self) -> Result<(Vec<u8>, bool), String> {
        let mut bitstream = Vec::new();
        let mut keyframe = false;

        loop {
            let mut info = MFT_OUTPUT_STREAM_INFO::default();
            self.transform
                .GetOutputStreamInfo(0, &mut info)
                .map_err(|e| e.to_string())?;
            let out_size = info.cbSize.max(4096);
            let out_buf = MFCreateMemoryBuffer(out_size).map_err(|e| e.to_string())?;
            let out_sample = MFCreateSample().map_err(|e| e.to_string())?;
            out_sample.AddBuffer(&out_buf).map_err(|e| e.to_string())?;

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
                        let buf = s.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
                        let mut p: *mut u8 = std::ptr::null_mut();
                        let mut len = 0u32;
                        buf.Lock(&mut p, None, Some(&mut len))
                            .map_err(|e| e.to_string())?;
                        if !p.is_null() && len > 0 {
                            bitstream.extend_from_slice(std::slice::from_raw_parts(p, len as usize));
                        }
                        let _ = buf.Unlock();
                    }
                    // drop sample
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

            let buffer = MFCreateMemoryBuffer(nv12.len() as u32).map_err(|e| e.to_string())?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut max_len = 0u32;
            buffer
                .Lock(&mut ptr, Some(&mut max_len), None)
                .map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;

            let sample = MFCreateSample().map_err(|e| e.to_string())?;
            sample.AddBuffer(&buffer).map_err(|e| e.to_string())?;
            sample
                .SetSampleTime((pts_us as i64).saturating_mul(10))
                .map_err(|e| e.to_string())?;
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
