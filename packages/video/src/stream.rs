//! Host video send + client video receive (Phase 6).

use crate::decode::VideoDecoder;
use crate::encode::EncodedFrame;
use lanplay_protocol::{fragment_access_unit, FrameReassembler};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Shared sink used by the capture/encode loop to push bitstream toward the client.
#[derive(Clone)]
pub struct VideoStreamSink {
    slot: Arc<Mutex<Option<OutgoingFrame>>>,
    frame_id: Arc<AtomicU32>,
    peer: Arc<Mutex<Option<SocketAddr>>>,
    force_keyframe: Arc<AtomicBool>,
    packets_sent: Arc<AtomicU64>,
}

struct OutgoingFrame {
    width: u32,
    height: u32,
    frame: EncodedFrame,
}

impl VideoStreamSink {
    pub fn new() -> Self {
        Self {
            slot: Arc::new(Mutex::new(None)),
            frame_id: Arc::new(AtomicU32::new(1)),
            peer: Arc::new(Mutex::new(None)),
            force_keyframe: Arc::new(AtomicBool::new(false)),
            packets_sent: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn peer_handle(&self) -> Arc<Mutex<Option<SocketAddr>>> {
        Arc::clone(&self.peer)
    }

    pub fn force_keyframe_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.force_keyframe)
    }

    pub fn set_peer(&self, addr: Option<SocketAddr>) {
        *self.peer.lock() = addr;
        if addr.is_some() {
            self.force_keyframe.store(true, Ordering::SeqCst);
        }
    }

    pub fn take_force_keyframe(&self) -> bool {
        self.force_keyframe.swap(false, Ordering::SeqCst)
    }

    /// True when a client peer is configured for video.
    pub fn has_peer(&self) -> bool {
        self.peer.lock().is_some()
    }

    /// Capture thread: publish latest encoded AU (replaces unread).
    pub fn publish(&self, width: u32, height: u32, frame: EncodedFrame) {
        *self.slot.lock() = Some(OutgoingFrame {
            width,
            height,
            frame,
        });
    }

    pub fn packets_sent(&self) -> u64 {
        self.packets_sent.load(Ordering::Relaxed)
    }

    /// Spawn host UDP sender (binds ephemeral local port).
    pub fn start_sender(self) -> lanplay_shared::Result<VideoSenderHandle> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let join = thread::Builder::new()
            .name("lanplay-video-send".into())
            .spawn(move || {
                let sock = match UdpSocket::bind("0.0.0.0:0") {
                    Ok(s) => {
                        let _ = s.set_nonblocking(false);
                        let _ = s.set_write_timeout(Some(Duration::from_millis(50)));
                        s
                    }
                    Err(e) => {
                        eprintln!("video send bind failed: {e}");
                        return;
                    }
                };

                while !stop_t.load(Ordering::Relaxed) {
                    let peer = *self.peer.lock();
                    let Some(addr) = peer else {
                        thread::sleep(Duration::from_millis(20));
                        continue;
                    };

                    let outgoing = self.slot.lock().take();
                    let Some(out) = outgoing else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };

                    let id = self.frame_id.fetch_add(1, Ordering::Relaxed);
                    let pkts = fragment_access_unit(
                        id,
                        out.width,
                        out.height,
                        out.frame.pts_us,
                        out.frame.keyframe,
                        &out.frame.data,
                    );
                    for p in pkts {
                        match sock.send_to(&p, addr) {
                            Ok(_) => {
                                self.packets_sent.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(_) => break,
                        }
                    }
                }
            })
            .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

        Ok(VideoSenderHandle {
            stop,
            join: Some(join),
        })
    }
}

pub struct VideoSenderHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl VideoSenderHandle {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for VideoSenderHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

// --- Client receive ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientVideoSnapshot {
    pub active: bool,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub frames: u64,
    pub packets: u64,
    /// JPEG base64 for UI preview (empty if none yet).
    pub jpeg_base64: String,
    pub detail: String,
}

struct ClientVideoInner {
    width: AtomicU32,
    height: AtomicU32,
    frames: AtomicU64,
    packets: AtomicU64,
    fps_x100: AtomicU32,
    jpeg_base64: Mutex<String>,
    detail: Mutex<String>,
    active: AtomicBool,
}

impl Default for ClientVideoInner {
    fn default() -> Self {
        Self {
            width: AtomicU32::new(0),
            height: AtomicU32::new(0),
            frames: AtomicU64::new(0),
            packets: AtomicU64::new(0),
            fps_x100: AtomicU32::new(0),
            jpeg_base64: Mutex::new(String::new()),
            detail: Mutex::new("Waiting for video…".into()),
            active: AtomicBool::new(false),
        }
    }
}

impl ClientVideoInner {
    fn snapshot(&self) -> ClientVideoSnapshot {
        ClientVideoSnapshot {
            active: self.active.load(Ordering::Relaxed),
            width: self.width.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
            fps: self.fps_x100.load(Ordering::Relaxed) as f32 / 100.0,
            frames: self.frames.load(Ordering::Relaxed),
            packets: self.packets.load(Ordering::Relaxed),
            jpeg_base64: self.jpeg_base64.lock().clone(),
            detail: self.detail.lock().clone(),
        }
    }
}

pub struct ClientVideoHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    state: Arc<ClientVideoInner>,
}

impl ClientVideoHandle {
    pub fn snapshot(&self) -> ClientVideoSnapshot {
        self.state.snapshot()
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for ClientVideoHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Bind UDP `video_port` and decode incoming stream for UI preview.
pub fn run_client_video_loop(video_port: u16) -> lanplay_shared::Result<ClientVideoHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(ClientVideoInner::default());
    let stop_t = Arc::clone(&stop);
    let state_t = Arc::clone(&state);

    let join = thread::Builder::new()
        .name("lanplay-video-recv".into())
        .spawn(move || {
            client_video_thread(video_port, stop_t, state_t);
        })
        .map_err(|e| lanplay_shared::LanPlayError::Message(e.to_string()))?;

    Ok(ClientVideoHandle {
        stop,
        join: Some(join),
        state,
    })
}

fn client_video_thread(video_port: u16, stop: Arc<AtomicBool>, state: Arc<ClientVideoInner>) {
    state.active.store(true, Ordering::Relaxed);
    let bind = format!("0.0.0.0:{video_port}");
    let sock = match UdpSocket::bind(&bind) {
        Ok(s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
            *state.detail.lock() = format!("Listening for video on {bind}");
            s
        }
        Err(e) => {
            *state.detail.lock() = format!("Video bind failed on {bind}: {e}");
            state.active.store(false, Ordering::Relaxed);
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(200));
            }
            return;
        }
    };

    let mut decoder = match VideoDecoder::new() {
        Ok(d) => d,
        Err(e) => {
            *state.detail.lock() = format!("Decoder init failed: {e}");
            state.active.store(false, Ordering::Relaxed);
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(200));
            }
            return;
        }
    };

    let mut reasm = FrameReassembler::new();
    let mut buf = vec![0u8; 2048];
    let mut frames_window = 0u32;
    let mut window_start = Instant::now();
    let mut got_keyframe = false;

    while !stop.load(Ordering::Relaxed) {
        match sock.recv_from(&mut buf) {
            Ok((n, _src)) => {
                state.packets.fetch_add(1, Ordering::Relaxed);
                if let Some(frame) = reasm.push(&buf[..n]) {
                    if frame.keyframe {
                        got_keyframe = true;
                    }
                    if !got_keyframe && !frame.keyframe {
                        // Wait for IDR after join.
                        continue;
                    }
                    match decoder.decode_to_rgba(&frame.data) {
                        Ok(Some((w, h, rgba))) => {
                            state.width.store(w, Ordering::Relaxed);
                            state.height.store(h, Ordering::Relaxed);
                            state.frames.fetch_add(1, Ordering::Relaxed);
                            frames_window += 1;

                            if let Some(jpeg) = rgba_to_jpeg_preview(&rgba, w, h, 640) {
                                *state.jpeg_base64.lock() = jpeg;
                                *state.detail.lock() =
                                    format!("Streaming {w}x{h} (JPEG preview ≤640)");
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            *state.detail.lock() = format!("Decode error: {e}");
                            // Request recovery by waiting for next keyframe
                            got_keyframe = false;
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                *state.detail.lock() = format!("recv error: {e}");
                thread::sleep(Duration::from_millis(50));
            }
        }

        if window_start.elapsed() >= Duration::from_secs(1) {
            let secs = window_start.elapsed().as_secs_f32().max(0.001);
            state
                .fps_x100
                .store(((frames_window as f32 / secs) * 100.0) as u32, Ordering::Relaxed);
            frames_window = 0;
            window_start = Instant::now();
        }
    }

    state.active.store(false, Ordering::Relaxed);
    *state.detail.lock() = "Video receive stopped.".into();
}

fn rgba_to_jpeg_preview(rgba: &[u8], w: u32, h: u32, max_edge: u32) -> Option<String> {
    use image::{ImageBuffer, RgbaImage};
    use std::io::Cursor;

    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return None;
    }

    let (dw, dh) = if w.max(h) <= max_edge {
        (w, h)
    } else {
        let scale = max_edge as f32 / w.max(h) as f32;
        (
            ((w as f32 * scale) as u32).max(2) & !1,
            ((h as f32 * scale) as u32).max(2) & !1,
        )
    };

    let img: RgbaImage = if dw == w && dh == h {
        ImageBuffer::from_raw(w, h, rgba.to_vec())?
    } else {
        let mut out = vec![0u8; (dw * dh * 4) as usize];
        for y in 0..dh {
            let sy = (y as u64 * h as u64 / dh as u64) as u32;
            for x in 0..dw {
                let sx = (x as u64 * w as u64 / dw as u64) as u32;
                let si = ((sy * w + sx) * 4) as usize;
                let di = ((y * dw + x) * 4) as usize;
                out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
            }
        }
        ImageBuffer::from_raw(dw, dh, out)?
    };

    let mut cursor = Cursor::new(Vec::new());
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 55);
    encoder.encode(rgb.as_raw(), dw, dh, image::ExtendedColorType::Rgb8).ok()?;
    Some(base64_encode(cursor.into_inner()))
}

fn base64_encode(data: Vec<u8>) -> String {
    // Minimal base64 without extra dep
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
