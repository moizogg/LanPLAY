//! Desktop capture backends (Phase 4).
//!
//! Windows: DXGI Desktop Duplication (GPU path, no GDI).

use crate::stats::AtomicCaptureStats;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Capture configuration for host.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Prefer this output index (0 = primary).
    pub output_index: u32,
    /// Target max FPS for the capture loop (sleep budget).
    pub target_fps: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            output_index: 0,
            target_fps: 60,
        }
    }
}

/// Trait for replaceable capture backends.
pub trait CaptureBackend: Send {
    fn open(&mut self) -> Result<(), String>;
    /// Capture one frame. Returns (width, height) when a new frame is available.
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

/// Start host desktop capture loop (Phase 4 — measure only, no stream yet).
pub fn run_host_capture_loop(config: CaptureConfig) -> lanplay_shared::Result<HostCaptureHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(AtomicCaptureStats::default());
    let stop_t = Arc::clone(&stop);
    let stats_t = Arc::clone(&stats);

    let join = thread::Builder::new()
        .name("lanplay-host-capture".into())
        .spawn(move || {
            capture_loop(config, stop_t, stats_t);
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(HostCaptureHandle {
        stop,
        join: Some(join),
        stats,
    })
}

fn capture_loop(config: CaptureConfig, stop: Arc<AtomicBool>, stats: Arc<AtomicCaptureStats>) {
    stats.set_active(true);
    stats.set_detail("Starting desktop capture…");

    #[cfg(windows)]
    {
        windows_dxgi::run(config, stop, stats);
    }

    #[cfg(not(windows))]
    {
        let _ = config;
        stats.set_detail("Desktop capture is only supported on Windows.");
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
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
        DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTPUT_DESC,
    };

    struct DxgiCapture {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        duplication: IDXGIOutputDuplication,
        width: u32,
        height: u32,
        output_index: u32,
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
                    width,
                    height,
                    output_index,
                })
            }
        }

        /// Acquire next frame; returns Some((w,h)) if a new desktop frame was produced.
        fn capture_frame(&mut self) -> Result<Option<(u32, u32)>, String> {
            unsafe {
                let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut resource: Option<IDXGIResource> = None;

                // ~16ms wait for ~60 FPS budget
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

                // We only need timing + metadata for Phase 4; still map resource briefly
                // to force GPU work and validate path (no CPU copy of full frame yet).
                let _tex: Option<ID3D11Texture2D> = resource.and_then(|r| r.cast().ok());

                let _ = self.duplication.ReleaseFrame();

                // Update size if desktop mode changed
                if frame_info.TotalMetadataBufferSize > 0 {
                    // keep previous size; reinit on ACCESS_LOST handles mode change
                }

                Ok(Some((self.width, self.height)))
            }
        }
    }

    pub fn run(config: CaptureConfig, stop: Arc<AtomicBool>, stats: Arc<AtomicCaptureStats>) {
        let mut backend = match DxgiCapture::open(config.output_index) {
            Ok(b) => {
                stats.set_detail(format!(
                    "DXGI Desktop Duplication OK — {}x{} (output {})",
                    b.width, b.height, b.output_index
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

        let mut frames_window = 0u32;
        let mut window_start = Instant::now();
        let frame_budget = Duration::from_micros(1_000_000u64 / config.target_fps.max(1) as u64);

        while !stop.load(Ordering::Relaxed) {
            let t0 = Instant::now();
            match backend.capture_frame() {
                Ok(Some((w, h))) => {
                    let us = t0.elapsed().as_micros() as u64;
                    stats.record_frame(w, h, us);
                    frames_window += 1;
                }
                Ok(None) => {
                    // timeout — desktop idle; still count as loop tick for pacing
                }
                Err(e) if e == "ACCESS_LOST" => {
                    stats.set_detail("Display mode changed — reopening capture…");
                    let idx = backend.output_index;
                    // Replace backend without leaving it moved across loop iterations
                    match DxgiCapture::open(idx).or_else(|_| {
                        thread::sleep(Duration::from_millis(100));
                        DxgiCapture::open(config.output_index)
                    }) {
                        Ok(b) => {
                            stats.set_detail(format!(
                                "Reopened DXGI capture {}x{}",
                                b.width, b.height
                            ));
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
                let fps = frames_window as f32 / window_start.elapsed().as_secs_f32();
                stats.set_fps(fps);
                frames_window = 0;
                window_start = Instant::now();
            }

            // Light pacing if we're faster than target (AcquireNextFrame already waits)
            let elapsed = t0.elapsed();
            if elapsed < frame_budget / 4 {
                thread::sleep(Duration::from_millis(1));
            }
        }

        stats.set_active(false);
        stats.set_detail("Capture stopped.");
    }
}
