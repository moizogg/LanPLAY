//! D3D11 GPU capture convert path (Sunshine `display_vram`-class).
//!
//! Sunshine keeps DXGI frames on the GPU, converts RGB→NV12 with shaders/VPP,
//! then feeds the hardware encoder. We do the same family of work with the
//! D3D11 Video Processor:
//!
//! ```text
//! DXGI desktop texture (GPU)
//!   → VideoProcessor scale + color convert
//!   → NV12 at encode size (GPU)
//!   → Map only NV12 (≈1.5 B/px, encode-sized — not full desktop BGRA)
//!   → FFmpeg QSV / MF / etc.
//! ```
//!
//! Still not in-process libavcodec zero-copy into QSV (Sunshine's last mile),
//! but removes the two biggest taxes: full-desktop Map + CPU scale/YUV.

#![cfg(windows)]

use windows::core::Interface;
use windows::Win32::Foundation::{BOOL, HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEX2D_VPIV,
    D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};

fn me(e: windows::core::Error) -> String {
    e.to_string()
}

/// GPU desktop → encode-sized NV12 readback.
pub struct GpuDesktopPipeline {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    duplication: IDXGIOutputDuplication,
    output_index: u32,

    cap_w: u32,
    cap_h: u32,
    enc_w: u32,
    enc_h: u32,

    /// Last desktop format (for staging recreation).
    desktop_format: DXGI_FORMAT,

    /// Intermediate: full desktop copy on GPU (DEFAULT) when we need a stable surface.
    desktop_gpu: Option<ID3D11Texture2D>,

    /// Encode-sized NV12 on GPU (DEFAULT + RENDER_TARGET for VP output).
    nv12_gpu: Option<ID3D11Texture2D>,
    /// Encode-sized NV12 staging for Map.
    nv12_staging: Option<ID3D11Texture2D>,

    enumerator: Option<ID3D11VideoProcessorEnumerator>,
    processor: Option<ID3D11VideoProcessor>,

    /// Tight NV12 buffer (Y + UV interleaved), last good frame for idle re-encode.
    pub last_nv12: Vec<u8>,
    pub has_frame: bool,
}

// COM used only on encode thread.
unsafe impl Send for GpuDesktopPipeline {}

impl GpuDesktopPipeline {
    pub fn open(output_index: u32, enc_w: u32, enc_h: u32) -> Result<Self, String> {
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut level = D3D_FEATURE_LEVEL_11_0;

            // VIDEO_SUPPORT required for ID3D11VideoDevice (VPP).
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut level),
                Some(&mut context),
            )
            .map_err(|e| format!("D3D11CreateDevice (video): {e}"))?;

            let device = device.ok_or("no D3D11 device")?;
            let context = context.ok_or("no D3D11 context")?;

            // QSV / multi-thread safety (Sunshine does this).
            if let Ok(mt) = device.cast::<ID3D11Multithread>() {
                mt.SetMultithreadProtected(true);
            }

            let video_device: ID3D11VideoDevice = device
                .cast()
                .map_err(|e| format!("ID3D11VideoDevice (GPU VPP unavailable): {e}"))?;
            let video_context: ID3D11VideoContext = context
                .cast()
                .map_err(|e| format!("ID3D11VideoContext: {e}"))?;

            let factory: IDXGIFactory1 =
                CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))?;
            let adapter = factory
                .EnumAdapters1(0)
                .map_err(|e| format!("EnumAdapters1: {e}"))?;
            let output = adapter
                .EnumOutputs(output_index)
                .map_err(|e| format!("EnumOutputs({output_index}): {e}"))?;
            let output1: IDXGIOutput1 = output
                .cast()
                .map_err(|e| format!("IDXGIOutput1: {e}"))?;

            let desc = output.GetDesc().map_err(me)?;
            let cap_w =
                (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(1) as u32;
            let cap_h =
                (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(1) as u32;

            let duplication = output1
                .DuplicateOutput(&device)
                .map_err(|e| format!("DuplicateOutput: {e}"))?;

            let enc_w = enc_w.max(16) & !1;
            let enc_h = enc_h.max(16) & !1;

            let mut pipe = Self {
                device,
                context,
                video_device,
                video_context,
                duplication,
                output_index,
                cap_w,
                cap_h,
                enc_w,
                enc_h,
                desktop_format: DXGI_FORMAT_B8G8R8A8_UNORM,
                desktop_gpu: None,
                nv12_gpu: None,
                nv12_staging: None,
                enumerator: None,
                processor: None,
                last_nv12: vec![0u8; (enc_w * enc_h * 3 / 2) as usize],
                has_frame: false,
            };
            pipe.ensure_nv12_targets()?;
            pipe.ensure_processor()?;
            Ok(pipe)
        }
    }

    pub fn output_index(&self) -> u32 {
        self.output_index
    }
    pub fn cap_size(&self) -> (u32, u32) {
        (self.cap_w, self.cap_h)
    }
    pub fn enc_size(&self) -> (u32, u32) {
        (self.enc_w, self.enc_h)
    }

    pub fn set_encode_size(&mut self, w: u32, h: u32) -> Result<(), String> {
        let w = w.max(16) & !1;
        let h = h.max(16) & !1;
        if w == self.enc_w && h == self.enc_h {
            return Ok(());
        }
        self.enc_w = w;
        self.enc_h = h;
        self.nv12_gpu = None;
        self.nv12_staging = None;
        self.enumerator = None;
        self.processor = None;
        self.last_nv12
            .resize((w * h * 3 / 2) as usize, 0);
        self.ensure_nv12_targets()?;
        self.ensure_processor()?;
        Ok(())
    }

    fn ensure_nv12_targets(&mut self) -> Result<(), String> {
        if self.nv12_gpu.is_some() && self.nv12_staging.is_some() {
            return Ok(());
        }
        unsafe {
            let desc_gpu = D3D11_TEXTURE2D_DESC {
                Width: self.enc_w,
                Height: self.enc_h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut gpu: Option<ID3D11Texture2D> = None;
            self.device
                .CreateTexture2D(&desc_gpu, None, Some(&mut gpu))
                .map_err(|e| format!("CreateTexture2D NV12 GPU: {e}"))?;
            self.nv12_gpu = gpu;

            let desc_st = D3D11_TEXTURE2D_DESC {
                Width: self.enc_w,
                Height: self.enc_h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut st: Option<ID3D11Texture2D> = None;
            self.device
                .CreateTexture2D(&desc_st, None, Some(&mut st))
                .map_err(|e| format!("CreateTexture2D NV12 staging: {e}"))?;
            self.nv12_staging = st;
        }
        Ok(())
    }

    fn ensure_desktop_gpu(&mut self, format: DXGI_FORMAT) -> Result<(), String> {
        let need_new = match &self.desktop_gpu {
            None => true,
            Some(_) => format != self.desktop_format,
        };
        if !need_new {
            // size may change
            if let Some(tex) = &self.desktop_gpu {
                unsafe {
                    let mut d = D3D11_TEXTURE2D_DESC::default();
                    tex.GetDesc(&mut d);
                    if d.Width == self.cap_w && d.Height == self.cap_h {
                        return Ok(());
                    }
                }
            }
        }
        unsafe {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: self.cap_w,
                Height: self.cap_h,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: 0, // VP input doesn't need RT
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut tex: Option<ID3D11Texture2D> = None;
            self.device
                .CreateTexture2D(&desc, None, Some(&mut tex))
                .map_err(|e| format!("CreateTexture2D desktop GPU: {e}"))?;
            self.desktop_gpu = tex;
            self.desktop_format = format;
            // Processor is tied to content sizes — rebuild
            self.enumerator = None;
            self.processor = None;
            self.ensure_processor()?;
        }
        Ok(())
    }

    fn ensure_processor(&mut self) -> Result<(), String> {
        if self.processor.is_some() {
            return Ok(());
        }
        unsafe {
            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                InputWidth: self.cap_w,
                InputHeight: self.cap_h,
                OutputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                OutputWidth: self.enc_w,
                OutputHeight: self.enc_h,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let enumerator = self
                .video_device
                .CreateVideoProcessorEnumerator(&content)
                .map_err(|e| format!("CreateVideoProcessorEnumerator: {e}"))?;

            // Check NV12 output support
            let out_flags = enumerator
                .CheckVideoProcessorFormat(DXGI_FORMAT_NV12)
                .map_err(|e| format!("CheckVideoProcessorFormat NV12: {e}"))?;
            if (out_flags & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32) == 0 {
                return Err("GPU Video Processor cannot output NV12 on this device".into());
            }
            let in_flags = enumerator
                .CheckVideoProcessorFormat(self.desktop_format)
                .unwrap_or(0);
            if (in_flags & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32) == 0 {
                // try BGRA
                let in2 = enumerator
                    .CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM)
                    .unwrap_or(0);
                if (in2 & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32) == 0 {
                    return Err("GPU Video Processor cannot take desktop format as input".into());
                }
            }

            let processor = self
                .video_device
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(|e| format!("CreateVideoProcessor: {e}"))?;

            self.enumerator = Some(enumerator);
            self.processor = Some(processor);
        }
        Ok(())
    }

    /// Capture one frame into `last_nv12` (encode-sized NV12).
    /// `do_convert`: if false, only drain DXGI without GPU convert/map.
    pub fn capture_nv12(&mut self, do_convert: bool) -> Result<Option<()>, String> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let wait = if do_convert { 8u32 } else { 4u32 };
            let hr = self
                .duplication
                .AcquireNextFrame(wait, &mut frame_info, &mut resource);

            if let Err(e) = hr {
                let code = e.code();
                if code == DXGI_ERROR_WAIT_TIMEOUT {
                    return Ok(None);
                }
                if code == DXGI_ERROR_ACCESS_LOST {
                    return Err("ACCESS_LOST".into());
                }
                return Err(format!("AcquireNextFrame: {e}"));
            }

            let resource = resource.ok_or("null resource")?;
            let src_tex: ID3D11Texture2D = resource
                .cast()
                .map_err(|e| format!("texture cast: {e}"))?;

            if !do_convert {
                let _ = self.duplication.ReleaseFrame();
                return Ok(None);
            }

            let mut src_desc = D3D11_TEXTURE2D_DESC::default();
            src_tex.GetDesc(&mut src_desc);
            if src_desc.Width != self.cap_w || src_desc.Height != self.cap_h {
                self.cap_w = src_desc.Width;
                self.cap_h = src_desc.Height;
                self.desktop_gpu = None;
                self.enumerator = None;
                self.processor = None;
            }
            self.desktop_format = src_desc.Format;
            self.ensure_desktop_gpu(src_desc.Format)?;
            self.ensure_nv12_targets()?;
            self.ensure_processor()?;

            let desktop = self.desktop_gpu.as_ref().unwrap();
            // Stay on GPU: copy desktop image into our DEFAULT texture.
            self.context.CopyResource(desktop, &src_tex);

            self.vpp_convert(desktop)?;

            let _ = self.duplication.ReleaseFrame();
            self.has_frame = true;
            Ok(Some(()))
        }
    }

    unsafe fn vpp_convert(&mut self, desktop: &ID3D11Texture2D) -> Result<(), String> {
        let enumerator = self.enumerator.as_ref().ok_or("no enumerator")?;
        let processor = self.processor.as_ref().ok_or("no processor")?;
        let nv12_gpu = self.nv12_gpu.as_ref().ok_or("no nv12 gpu")?;
        let nv12_staging = self.nv12_staging.as_ref().ok_or("no nv12 staging")?;

        let in_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut in_view: Option<ID3D11VideoProcessorInputView> = None;
        self.video_device
            .CreateVideoProcessorInputView(desktop, enumerator, &in_desc, Some(&mut in_view))
            .map_err(|e| format!("CreateVideoProcessorInputView: {e}"))?;
        let in_view = in_view.ok_or("null input view")?;

        let out_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut out_view: Option<ID3D11VideoProcessorOutputView> = None;
        self.video_device
            .CreateVideoProcessorOutputView(nv12_gpu, enumerator, &out_desc, Some(&mut out_view))
            .map_err(|e| format!("CreateVideoProcessorOutputView: {e}"))?;
        let out_view = out_view.ok_or("null output view")?;

        let src_rect = RECT {
            left: 0,
            top: 0,
            right: self.cap_w as i32,
            bottom: self.cap_h as i32,
        };
        let dst_rect = RECT {
            left: 0,
            top: 0,
            right: self.enc_w as i32,
            bottom: self.enc_h as i32,
        };

        self.video_context.VideoProcessorSetStreamFrameFormat(
            processor,
            0,
            D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        );
        self.video_context.VideoProcessorSetStreamSourceRect(
            processor,
            0,
            true,
            Some(&src_rect as *const RECT),
        );
        self.video_context.VideoProcessorSetStreamDestRect(
            processor,
            0,
            true,
            Some(&dst_rect as *const RECT),
        );
        self.video_context.VideoProcessorSetOutputTargetRect(
            processor,
            true,
            Some(&dst_rect as *const RECT),
        );

        let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: BOOL(1),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(in_view)),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: std::mem::ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };

        let blt = self
            .video_context
            .VideoProcessorBlt(processor, &out_view, 0, std::slice::from_ref(&stream));

        // Release COM refs held in ManuallyDrop so we don't leak.
        let _ = std::mem::ManuallyDrop::take(&mut stream.pInputSurface);
        let _ = std::mem::ManuallyDrop::take(&mut stream.pInputSurfaceRight);
        blt.map_err(|e| format!("VideoProcessorBlt: {e}"))?;

        // GPU NV12 → staging → CPU tight buffer (only encode-sized readback).
        self.context.CopyResource(nv12_staging, nv12_gpu);
        self.map_nv12_staging()?;
        Ok(())
    }

    unsafe fn map_nv12_staging(&mut self) -> Result<(), String> {
        let staging = self.nv12_staging.as_ref().ok_or("no staging")?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        self.context
            .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| format!("Map NV12 staging: {e}"))?;

        let w = self.enc_w as usize;
        let h = self.enc_h as usize;
        let pitch = mapped.RowPitch as usize;
        let ptr = mapped.pData as *const u8;
        let need = w * h * 3 / 2;
        if self.last_nv12.len() != need {
            self.last_nv12.resize(need, 0);
        }

        // Y plane
        for y in 0..h {
            let src = ptr.add(y * pitch);
            let dst = self.last_nv12.as_mut_ptr().add(y * w);
            std::ptr::copy_nonoverlapping(src, dst, w);
        }
        // UV plane (height/2 rows, width bytes each — interleaved UV)
        let uv_dst_base = w * h;
        let y_plane_rows = h; // NV12 UV starts after Y in texture; RowPitch applies
                              // For NV12 staging, UV is at offset RowPitch * Height in the mapped resource.
        let uv_src_base = ptr.add(pitch * h);
        for y in 0..(h / 2) {
            let src = uv_src_base.add(y * pitch);
            let dst = self.last_nv12.as_mut_ptr().add(uv_dst_base + y * w);
            std::ptr::copy_nonoverlapping(src, dst, w);
        }
        let _ = y_plane_rows;

        self.context.Unmap(staging, 0);
        Ok(())
    }

    pub fn reopen(&mut self) -> Result<(), String> {
        let idx = self.output_index;
        let (ew, eh) = (self.enc_w, self.enc_h);
        *self = Self::open(idx, ew, eh)?;
        Ok(())
    }
}

/// Stamp OS cursor into encode-sized NV12 (approx YUV). Avoids full BGRA path.
pub fn stamp_cursor_nv12(nv12: &mut [u8], enc_w: u32, enc_h: u32, cap_w: u32, cap_h: u32) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        PatBlt, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLACKNESS,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, ICONINFO,
    };

    if enc_w == 0 || enc_h == 0 || cap_w == 0 || cap_h == 0 {
        return;
    }
    let need = (enc_w * enc_h * 3 / 2) as usize;
    if nv12.len() < need {
        return;
    }

    unsafe {
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if GetCursorInfo(&mut ci).is_err() {
            return;
        }
        if (ci.flags.0 & CURSOR_SHOWING.0) == 0 || ci.hCursor.is_invalid() {
            return;
        }
        let mut ii = ICONINFO::default();
        if GetIconInfo(ci.hCursor, &mut ii).is_err() {
            return;
        }
        let hot_x = ii.xHotspot as i32;
        let hot_y = ii.yHotspot as i32;
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }

        let sx = ((ci.ptScreenPos.x - hot_x) as f32 * enc_w as f32 / cap_w as f32).round() as i32;
        let sy = ((ci.ptScreenPos.y - hot_y) as f32 * enc_h as f32 / cap_h as f32).round() as i32;
        let cw = ((32.0f32 * enc_w as f32 / cap_w as f32).round() as i32).max(8);
        let ch = ((32.0f32 * enc_h as f32 / cap_h as f32).round() as i32).max(8);
        if sx + cw < 0 || sy + ch < 0 || sx >= enc_w as i32 || sy >= enc_h as i32 {
            return;
        }

        let hdc_screen = GetDC(HWND::default());
        if hdc_screen.is_invalid() {
            return;
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbmp = CreateCompatibleBitmap(hdc_screen, cw, ch);
        let old = SelectObject(hdc_mem, HGDIOBJ(hbmp.0));
        let _ = PatBlt(hdc_mem, 0, 0, cw, ch, BLACKNESS);
        let _ = DrawIconEx(hdc_mem, 0, 0, ci.hCursor, cw, ch, 0, None, DI_NORMAL);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: cw,
                biHeight: -ch,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cursor_bgra = vec![0u8; (cw * ch * 4) as usize];
        let got = GetDIBits(
            hdc_mem,
            hbmp,
            0,
            ch as u32,
            Some(cursor_bgra.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        let _ = SelectObject(hdc_mem, old);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(HWND::default(), hdc_screen);
        if got == 0 {
            return;
        }

        let w = enc_w as usize;
        let h = enc_h as usize;
        let y_size = w * h;
        for row in 0..ch {
            let dy = sy + row;
            if dy < 0 || dy >= enc_h as i32 {
                continue;
            }
            for col in 0..cw {
                let dx = sx + col;
                if dx < 0 || dx >= enc_w as i32 {
                    continue;
                }
                let si = ((row * cw + col) * 4) as usize;
                let b = cursor_bgra[si] as i32;
                let g = cursor_bgra[si + 1] as i32;
                let r = cursor_bgra[si + 2] as i32;
                let a = cursor_bgra[si + 3];
                if r | g | b == 0 && a == 0 {
                    continue;
                }
                let yy = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8;
                let yi = dy as usize * w + dx as usize;
                nv12[yi] = yy;
                // UV 2x2
                let ux = (dx as usize) & !1;
                let uy = (dy as usize) & !1;
                let u = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                let v = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                let ui = y_size + (uy / 2) * w + ux;
                if ui + 1 < nv12.len() {
                    nv12[ui] = u;
                    nv12[ui + 1] = v;
                }
            }
        }
    }
}
