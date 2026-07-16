//! Desktop capture + encode loop (Phase 4–6).
//!
//! Windows: DXGI Desktop Duplication → staging BGRA → H.264 encode → optional stream sink.

use crate::encode::{
    choose_encode_size, create_encoder, scale_bgra_nn, EncoderSettings, VideoEncoder,
};
use crate::stats::AtomicCaptureStats;
use crate::stream::VideoStreamSink;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub output_index: u32,
    pub target_fps: u32,
    /// Max long-edge when `fixed_width`/`fixed_height` are unset.
    pub encode_max_edge: u32,
    /// Optional fixed encode size (even dimensions preferred).
    pub fixed_width: Option<u32>,
    pub fixed_height: Option<u32>,
    pub bitrate_bps: u32,
    /// Encoder id (`openh264`, later nvenc/amf/qsv).
    pub encoder_id: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            output_index: 0,
            target_fps: 30,
            encode_max_edge: 1280,
            fixed_width: None,
            fixed_height: None,
            bitrate_bps: 8_000_000,
            encoder_id: "openh264".into(),
        }
    }
}

impl CaptureConfig {
    /// Resolve encode size from capture dimensions + settings.
    pub fn resolve_encode_size(&self, cap_w: u32, cap_h: u32) -> (u32, u32) {
        match (self.fixed_width, self.fixed_height) {
            (Some(w), Some(h)) if w >= 16 && h >= 16 => (w.max(16) & !1, h.max(16) & !1),
            _ => choose_encode_size(cap_w, cap_h, self.encode_max_edge),
        }
    }
}

pub trait CaptureBackend: Send {
    fn open(&mut self) -> Result<(), String>;
    fn capture_frame(&mut self) -> Result<Option<(u32, u32)>, String>;
    fn close(&mut self);
}

pub struct HostCaptureHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    stats: Arc<AtomicCaptureStats>,
}

impl HostCaptureHandle {
    pub fn stats(&self) -> Arc<AtomicCaptureStats> {
        Arc::clone(&self.stats)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for HostCaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Start host capture + encode loop; optional `video_sink` streams H.264 to client (Phase 6).
pub fn run_host_capture_loop(
    config: CaptureConfig,
    video_sink: Option<VideoStreamSink>,
) -> lanplay_shared::Result<HostCaptureHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(AtomicCaptureStats::default());
    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);

    let join = thread::Builder::new()
        .name("lanplay-host-capture-encode".into())
        .spawn(move || {
            capture_encode_loop(config, video_sink, stop_t, stats_t);
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(HostCaptureHandle {
        stop,
        join: Some(join),
        stats,
    })
}

fn capture_encode_loop(
    config: CaptureConfig,
    video_sink: Option<VideoStreamSink>,
    stop: Arc<AtomicBool>,
    stats: Arc<AtomicCaptureStats>,
) {
    stats.set_active(true);
    stats.set_detail("Starting capture + encode…");

    #[cfg(windows)]
    {
        windows_dxgi::run(config, video_sink, stop, stats);
    }

    #[cfg(not(windows))]
    {
        let _ = (config, video_sink);
        stats.set_detail("Capture/encode only supported on Windows.");
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
        }
        stats.set_active(false);
    }
}

#[cfg(windows)]
mod windows_dxgi {
    use super::*;
    use std::time::Instant;
    use windows::core::Interface;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
        D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
        DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    };

    struct DxgiCapture {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        duplication: IDXGIOutputDuplication,
        staging: Option<ID3D11Texture2D>,
        width: u32,
        height: u32,
        output_index: u32,
        bgra: Vec<u8>,
    }

    impl DxgiCapture {
        fn open(output_index: u32) -> Result<Self, String> {
            unsafe {
                let mut device: Option<ID3D11Device> = None;
                let mut context: Option<ID3D11DeviceContext> = None;
                let mut level = D3D_FEATURE_LEVEL_11_0;

                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_HARDWARE,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&[D3D_FEATURE_LEVEL_11_0]),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    Some(&mut level),
                    Some(&mut context),
                )
                .map_err(|e| format!("D3D11CreateDevice failed: {e}"))?;

                let device = device.ok_or("no D3D11 device")?;
                let context = context.ok_or("no D3D11 context")?;

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
                    .map_err(|e| format!("IDXGIOutput1 cast: {e}"))?;

                let desc = output
                    .GetDesc()
                    .map_err(|e| format!("GetDesc: {e}"))?;
                let width =
                    (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(1) as u32;
                let height =
                    (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(1) as u32;

                let duplication = output1
                    .DuplicateOutput(&device)
                    .map_err(|e| format!("DuplicateOutput: {e}"))?;

                Ok(Self {
                    device,
                    context,
                    duplication,
                    staging: None,
                    width,
                    height,
                    output_index,
                    bgra: vec![0u8; (width * height * 4) as usize],
                })
            }
        }

        fn ensure_staging(&mut self, format: DXGI_FORMAT) -> Result<(), String> {
            if self.staging.is_some() {
                return Ok(());
            }
            unsafe {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: self.width,
                    Height: self.height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_STAGING,
                    // Staging textures must not bind to the pipeline.
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut tex: Option<ID3D11Texture2D> = None;
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut tex))
                    .map_err(|e| format!("CreateTexture2D staging: {e}"))?;
                self.staging = tex;
            }
            Ok(())
        }

        /// Capture one frame into `self.bgra` (BGRA8 tightly packed).
        fn capture_bgra(&mut self) -> Result<Option<(u32, u32)>, String> {
            unsafe {
                let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut resource: Option<IDXGIResource> = None;

                let hr = self
                    .duplication
                    .AcquireNextFrame(16, &mut frame_info, &mut resource);

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

                // Read texture desc for format
                let mut src_desc = D3D11_TEXTURE2D_DESC::default();
                src_tex.GetDesc(&mut src_desc);
                self.width = src_desc.Width;
                self.height = src_desc.Height;
                let needed = (self.width * self.height * 4) as usize;
                if self.bgra.len() != needed {
                    self.bgra.resize(needed, 0);
                    self.staging = None; // recreate for new size
                }

                self.ensure_staging(src_desc.Format)?;
                let staging = self.staging.as_ref().unwrap();

                self.context.CopyResource(staging, &src_tex);

                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                self.context
                    .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .map_err(|e| format!("Map staging: {e}"))?;

                let pitch = mapped.RowPitch as usize;
                let src_ptr = mapped.pData as *const u8;
                for y in 0..self.height as usize {
                    let src_row = src_ptr.add(y * pitch);
                    let dst_row = self.bgra.as_mut_ptr().add(y * self.width as usize * 4);
                    std::ptr::copy_nonoverlapping(src_row, dst_row, self.width as usize * 4);
                }

                self.context.Unmap(staging, 0);
                let _ = self.duplication.ReleaseFrame();

                // DXGI desktop image has no cursor — composite OS cursor (Moonlight/Sunshine-style).
                draw_cursor_bgra(&mut self.bgra, self.width, self.height);

                Ok(Some((self.width, self.height)))
            }
        }
    }

    /// Draw the system cursor into a tight BGRA buffer (primary display / output 0).
    fn draw_cursor_bgra(bgra: &mut [u8], width: u32, height: u32) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
            PatBlt, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLACKNESS,
            DIB_RGB_COLORS, HGDIOBJ,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, ICONINFO,
        };

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

            let x = ci.ptScreenPos.x - hot_x;
            let y = ci.ptScreenPos.y - hot_y;
            let cw = 32i32;
            let ch = 32i32;
            if x + cw < 0 || y + ch < 0 || x >= width as i32 || y >= height as i32 {
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

            for row in 0..ch {
                let dy = y + row;
                if dy < 0 || dy >= height as i32 {
                    continue;
                }
                for col in 0..cw {
                    let dx = x + col;
                    if dx < 0 || dx >= width as i32 {
                        continue;
                    }
                    let si = ((row * cw + col) * 4) as usize;
                    let b = cursor_bgra[si];
                    let g = cursor_bgra[si + 1];
                    let r = cursor_bgra[si + 2];
                    let a = cursor_bgra[si + 3];
                    if r == 0 && g == 0 && b == 0 && a == 0 {
                        continue;
                    }
                    let di = ((dy as u32 * width + dx as u32) * 4) as usize;
                    if di + 3 >= bgra.len() {
                        continue;
                    }
                    if (r | g | b) != 0 || a > 0 {
                        bgra[di] = b;
                        bgra[di + 1] = g;
                        bgra[di + 2] = r;
                        bgra[di + 3] = 255;
                    }
                }
            }
        }
    }

    pub fn run(
        config: CaptureConfig,
        video_sink: Option<VideoStreamSink>,
        stop: Arc<AtomicBool>,
        stats: Arc<AtomicCaptureStats>,
    ) {
        let mut backend = match DxgiCapture::open(config.output_index) {
            Ok(b) => {
                stats.set_detail(format!(
                    "DXGI capture OK — {}x{}",
                    b.width, b.height
                ));
                b
            }
            Err(e) => {
                stats.set_detail(format!("Capture open failed: {e}"));
                stats.set_active(false);
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(200));
                }
                return;
            }
        };

        let (ew, eh) = config.resolve_encode_size(backend.width, backend.height);
        let mut encoder: Option<Box<dyn VideoEncoder>> = match create_encoder(EncoderSettings {
            width: ew,
            height: eh,
            fps: config.target_fps,
            bitrate_bps: config.bitrate_bps,
            encoder_id: config.encoder_id.clone(),
        }) {
            Ok(e) => {
                stats.set_encoder_name(e.name().to_string());
                stats.set_detail(format!(
                    "Capture {}x{} → encode {}x{} @ {}fps / {} kbps ({})",
                    backend.width,
                    backend.height,
                    e.width(),
                    e.height(),
                    config.target_fps,
                    config.bitrate_bps / 1000,
                    e.name()
                ));
                Some(e)
            }
            Err(e) => {
                stats.set_encoder_name("none");
                stats.set_detail(format!("Encoder init failed: {e} (capture-only)"));
                None
            }
        };

        let mut frames_window = 0u32;
        let mut encode_window = 0u32;
        let mut bytes_window = 0u64;
        let mut window_start = Instant::now();
        // Cap encode rate to target_fps — capture can be 100–180 FPS on active desktop.
        let encode_interval = Duration::from_secs_f64(1.0 / config.target_fps.max(1) as f64);
        let mut last_encode = Instant::now() - encode_interval;
        let mut last_idr = Instant::now();
        // Prefer low latency when a client is connected: IDR more often for recovery.

        while !stop.load(Ordering::Relaxed) {
            let t0 = Instant::now();
            match backend.capture_bgra() {
                Ok(Some((w, h))) => {
                    let capture_us = t0.elapsed().as_micros() as u64;
                    stats.record_frame(w, h, capture_us);
                    frames_window += 1;

                    let should_encode = last_encode.elapsed() >= encode_interval;
                    if should_encode {
                        if let Some(ref mut enc) = encoder {
                            if let Some(ref sink) = video_sink {
                                // IDR on Accept + periodic IDR while streaming (lost-packet recovery).
                                let need_idr = sink.take_force_keyframe()
                                    || (sink.has_peer()
                                        && last_idr.elapsed() >= Duration::from_secs(1));
                                if need_idr {
                                    enc.force_keyframe();
                                    last_idr = Instant::now();
                                }
                            }

                            // Recreate encoder if desired encode size changed
                            let (want_w, want_h) = config.resolve_encode_size(w, h);
                            if want_w != enc.width() || want_h != enc.height() {
                                match create_encoder(EncoderSettings {
                                    width: want_w,
                                    height: want_h,
                                    fps: config.target_fps,
                                    bitrate_bps: config.bitrate_bps,
                                    encoder_id: config.encoder_id.clone(),
                                }) {
                                    Ok(e) => {
                                        stats.set_encoder_name(e.name().to_string());
                                        *enc = e;
                                    }
                                    Err(_) => {}
                                }
                            }

                            let t1 = Instant::now();
                            let scaled = if w == enc.width() && h == enc.height() {
                                None
                            } else {
                                Some(scale_bgra_nn(
                                    &backend.bgra,
                                    w,
                                    h,
                                    enc.width(),
                                    enc.height(),
                                ))
                            };
                            let pixels: &[u8] = scaled.as_deref().unwrap_or(&backend.bgra);

                            match enc.encode_bgra(pixels, t0.elapsed().as_micros() as u64) {
                                Ok(Some(frame)) => {
                                    let encode_us = t1.elapsed().as_micros() as u64;
                                    stats.record_encode(
                                        enc.width(),
                                        enc.height(),
                                        encode_us,
                                        frame.data.len(),
                                    );
                                    encode_window += 1;
                                    bytes_window += frame.data.len() as u64;
                                    last_encode = Instant::now();
                                    if let Some(ref sink) = video_sink {
                                        sink.publish(enc.width(), enc.height(), frame);
                                    }
                                }
                                Ok(None) => {
                                    last_encode = Instant::now();
                                }
                                Err(e) => {
                                    stats.set_detail(format!("Encode error: {e}"));
                                    last_encode = Instant::now();
                                }
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(e) if e == "ACCESS_LOST" => {
                    stats.set_detail("Display mode changed — reopening capture…");
                    let idx = backend.output_index;
                    match DxgiCapture::open(idx).or_else(|_| {
                        thread::sleep(Duration::from_millis(100));
                        DxgiCapture::open(config.output_index)
                    }) {
                        Ok(b) => {
                            stats.set_detail(format!("Reopened capture {}x{}", b.width, b.height));
                            backend = b;
                        }
                        Err(e2) => {
                            stats.set_detail(format!("Reopen failed: {e2}"));
                            thread::sleep(Duration::from_millis(500));
                        }
                    }
                }
                Err(e) => {
                    stats.set_detail(format!("Capture error: {e}"));
                    thread::sleep(Duration::from_millis(50));
                }
            }

            if window_start.elapsed() >= Duration::from_secs(1) {
                let secs = window_start.elapsed().as_secs_f32().max(0.001);
                stats.set_fps(frames_window as f32 / secs);
                stats.set_encode_fps(encode_window as f32 / secs);
                let kbps = ((bytes_window * 8) as f32 / secs / 1000.0) as u32;
                stats.set_bitrate_kbps(kbps);
                frames_window = 0;
                encode_window = 0;
                bytes_window = 0;
                window_start = Instant::now();
            }
        }

        stats.set_active(false);
        stats.set_detail("Capture/encode stopped.");
    }
}
