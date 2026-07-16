//! FFmpeg-based H.264 encode (Sunshine-class path).
//!
//! Sunshine uses FFmpeg `h264_qsv` / NVENC / AMF — **not** Media Foundation MFTs.
//! On Intel HD 4000, QSV works in Sunshine while MF often has no HW encoder.
//!
//! We spawn a bundled/system `ffmpeg` process:
//!   NV12 frames → stdin → h264_qsv|nvenc|amf|libx264 → Annex-B on stdout.
//!
//! Not full D3D11 zero-copy yet (that needs in-process libavcodec). Still a
//! massive win over OpenH264 because encode runs on the GPU (or x264 ultrafast).

use crate::encode::{EncodedFrame, EncoderSettings, VideoEncoder};
use crate::nv12::bgra_to_nv12;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static LAST_FFMPEG_PROBE: Mutex<String> = Mutex::new(String::new());
static FF_PATH_CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
static FF_ENCODERS_CACHE: OnceLock<FfmpegCaps> = OnceLock::new();
static EXTRA_FFMPEG_ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Call from Tauri setup so packaged `resources/ffmpeg/ffmpeg.exe` is found.
pub fn configure_ffmpeg_search_paths(roots: Vec<PathBuf>) {
    if let Ok(mut g) = EXTRA_FFMPEG_ROOTS.lock() {
        for r in roots {
            if !g.iter().any(|x| x == &r) {
                g.push(r);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FfmpegCaps {
    pub path: Option<PathBuf>,
    pub qsv: bool,
    pub nvenc: bool,
    pub amf: bool,
    pub x264: bool,
    pub detail: String,
}

pub fn last_ffmpeg_probe() -> String {
    LAST_FFMPEG_PROBE
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

fn set_probe(s: impl Into<String>) {
    if let Ok(mut g) = LAST_FFMPEG_PROBE.lock() {
        *g = s.into();
    }
}

/// Locate ffmpeg.exe (bundled resource, env, or PATH).
pub fn find_ffmpeg() -> Option<PathBuf> {
    FF_PATH_CACHE
        .get_or_init(|| {
            if let Ok(p) = std::env::var("LANPLAY_FFMPEG") {
                let pb = PathBuf::from(p.trim());
                if pb.is_file() {
                    return Some(pb);
                }
            }
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Ok(roots) = EXTRA_FFMPEG_ROOTS.lock() {
                for root in roots.iter() {
                    candidates.push(root.join("ffmpeg.exe"));
                    candidates.push(root.join("ffmpeg").join("ffmpeg.exe"));
                }
            }
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    candidates.push(dir.join("ffmpeg.exe"));
                    candidates.push(dir.join("ffmpeg").join("ffmpeg.exe"));
                    candidates.push(dir.join("resources").join("ffmpeg").join("ffmpeg.exe"));
                    candidates.push(dir.join("resources").join("ffmpeg.exe"));
                }
            }
            // Dev / repo layout
            candidates.push(PathBuf::from(
                "apps/desktop/src-tauri/resources/ffmpeg/ffmpeg.exe",
            ));
            candidates.push(PathBuf::from("resources/ffmpeg/ffmpeg.exe"));
            if let Ok(cwd) = std::env::current_dir() {
                candidates.push(cwd.join("resources").join("ffmpeg").join("ffmpeg.exe"));
                candidates.push(
                    cwd.join("apps")
                        .join("desktop")
                        .join("src-tauri")
                        .join("resources")
                        .join("ffmpeg")
                        .join("ffmpeg.exe"),
                );
            }
            for c in candidates {
                if c.is_file() {
                    return Some(c);
                }
            }
            // PATH
            which_ffmpeg()
        })
        .clone()
}

fn which_ffmpeg() -> Option<PathBuf> {
    let mut cmd = Command::new(if cfg!(windows) { "where" } else { "which" });
    cmd.arg("ffmpeg");
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let p = PathBuf::from(line);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Probe which encoders this ffmpeg build exposes.
pub fn probe_ffmpeg_caps() -> FfmpegCaps {
    FF_ENCODERS_CACHE
        .get_or_init(|| {
            let Some(path) = find_ffmpeg() else {
                let c = FfmpegCaps {
                    detail: "ffmpeg not found (bundle resources/ffmpeg or install + PATH / LANPLAY_FFMPEG)".into(),
                    ..Default::default()
                };
                set_probe(c.detail.clone());
                return c;
            };
            let mut cmd = Command::new(&path);
            cmd.args(["-hide_banner", "-encoders"]);
            #[cfg(windows)]
            {
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let out = match cmd.output() {
                Ok(o) => o,
                Err(e) => {
                    let c = FfmpegCaps {
                        path: Some(path),
                        detail: format!("ffmpeg -encoders failed: {e}"),
                        ..Default::default()
                    };
                    set_probe(c.detail.clone());
                    return c;
                }
            };
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let qsv = text.contains("h264_qsv");
            let nvenc = text.contains("h264_nvenc");
            let amf = text.contains("h264_amf");
            let x264 = text.contains("libx264");
            let mut bits = Vec::new();
            if qsv {
                bits.push("h264_qsv");
            }
            if nvenc {
                bits.push("h264_nvenc");
            }
            if amf {
                bits.push("h264_amf");
            }
            if x264 {
                bits.push("libx264");
            }
            let detail = if bits.is_empty() {
                format!("ffmpeg @ {} — no H.264 encoders listed", path.display())
            } else {
                format!("ffmpeg @ {} — {}", path.display(), bits.join(", "))
            };
            set_probe(detail.clone());
            FfmpegCaps {
                path: Some(path),
                qsv,
                nvenc,
                amf,
                x264,
                detail,
            }
        })
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegCodec {
    Qsv,
    Nvenc,
    Amf,
    X264,
}

impl FfmpegCodec {
    fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Qsv => "h264_qsv",
            Self::Nvenc => "h264_nvenc",
            Self::Amf => "h264_amf",
            Self::X264 => "libx264",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Qsv => "FFmpeg QSV",
            Self::Nvenc => "FFmpeg NVENC",
            Self::Amf => "FFmpeg AMF",
            Self::X264 => "FFmpeg x264",
        }
    }

    fn is_hw(self) -> bool {
        !matches!(self, Self::X264)
    }
}

/// Preferred codec order for `auto`.
pub fn auto_codec_order(caps: &FfmpegCaps) -> Vec<FfmpegCodec> {
    let mut v = Vec::new();
    if caps.qsv {
        v.push(FfmpegCodec::Qsv);
    }
    if caps.nvenc {
        v.push(FfmpegCodec::Nvenc);
    }
    if caps.amf {
        v.push(FfmpegCodec::Amf);
    }
    v
}

pub struct FfmpegEncoder {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<AnnexFrame>,
    width: u32,
    height: u32,
    fps: u32,
    name: String,
    pts_queue: VecDeque<u64>,
    frames_in: u64,
    frames_out: u64,
    started: Instant,
}

struct AnnexFrame {
    data: Vec<u8>,
    keyframe: bool,
}

impl FfmpegEncoder {
    pub fn new(settings: EncoderSettings, codec: FfmpegCodec) -> Result<Self, String> {
        let path = find_ffmpeg().ok_or_else(|| {
            "ffmpeg not found — place ffmpeg.exe in resources/ffmpeg or set LANPLAY_FFMPEG"
                .to_string()
        })?;
        let caps = probe_ffmpeg_caps();
        match codec {
            FfmpegCodec::Qsv if !caps.qsv => {
                return Err("ffmpeg build has no h264_qsv".into());
            }
            FfmpegCodec::Nvenc if !caps.nvenc => {
                return Err("ffmpeg build has no h264_nvenc".into());
            }
            FfmpegCodec::Amf if !caps.amf => {
                return Err("ffmpeg build has no h264_amf".into());
            }
            FfmpegCodec::X264 if !caps.x264 => {
                return Err("ffmpeg build has no libx264".into());
            }
            _ => {}
        }

        let w = settings.width.max(16) & !1;
        let h = settings.height.max(16) & !1;
        let fps = settings.fps.max(1).min(120);
        let bitrate = settings.bitrate_bps.max(1_000_000);
        let gop = fps; // IDR ~1/sec for recovery (pipe can't force mid-stream IDR easily)

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "nv12".into(),
            "-s".into(),
            format!("{w}x{h}"),
            "-r".into(),
            fps.to_string(),
            "-i".into(),
            "pipe:0".into(),
            "-an".into(),
            "-c:v".into(),
            codec.ffmpeg_name().into(),
            "-b:v".into(),
            bitrate.to_string(),
            "-maxrate".into(),
            bitrate.to_string(),
            // ~1 frame VBV — Sunshine-style low latency
            "-bufsize".into(),
            (bitrate / fps.max(1)).max(500_000).to_string(),
            "-g".into(),
            gop.to_string(),
            "-bf".into(),
            "0".into(),
            "-fps_mode".into(),
            "cfr".into(),
        ];

        match codec {
            FfmpegCodec::Qsv => {
                // HD 4000 / older QSV: keep options conservative
                args.extend([
                    "-preset".into(),
                    "veryfast".into(),
                    "-look_ahead".into(),
                    "0".into(),
                    "-async_depth".into(),
                    "1".into(),
                ]);
            }
            FfmpegCodec::Nvenc => {
                args.extend([
                    "-preset".into(),
                    "p1".into(), // fastest / low latency family
                    "-tune".into(),
                    "ull".into(),
                    "-rc".into(),
                    "cbr".into(),
                    "-zerolatency".into(),
                    "1".into(),
                ]);
            }
            FfmpegCodec::Amf => {
                args.extend([
                    "-usage".into(),
                    "ultralowlatency".into(),
                    "-quality".into(),
                    "speed".into(),
                    "-rc".into(),
                    "cbr".into(),
                ]);
            }
            FfmpegCodec::X264 => {
                args.extend([
                    "-preset".into(),
                    "ultrafast".into(),
                    "-tune".into(),
                    "zerolatency".into(),
                    "-profile:v".into(),
                    "baseline".into(),
                    "-x264-params".into(),
                    format!("keyint={gop}:min-keyint={gop}:scenecut=0:rc-lookahead=0:sync-lookahead=0:sliced-threads=0:bframes=0"),
                ]);
            }
        }

        args.extend([
            "-f".into(),
            "h264".into(),
            "pipe:1".into(),
        ]);

        let mut cmd = Command::new(&path);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn ffmpeg ({}): {e}", path.display()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ffmpeg stdin missing".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "ffmpeg stdout missing".to_string())?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| "ffmpeg stderr missing".to_string())?;

        let (tx, rx) = mpsc::sync_channel::<AnnexFrame>(8);

        // Drain stderr so the process never blocks on a full pipe.
        thread::Builder::new()
            .name("lanplay-ffmpeg-err".into())
            .spawn(move || {
                let mut buf = [0u8; 512];
                let mut acc = String::new();
                while let Ok(n) = stderr.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    acc.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if acc.len() > 400 {
                        let tail: String = acc.chars().skip(acc.len().saturating_sub(300)).collect();
                        set_probe(format!("ffmpeg: {tail}"));
                    }
                }
            })
            .ok();

        thread::Builder::new()
            .name("lanplay-ffmpeg-out".into())
            .spawn(move || {
                let mut reader = stdout;
                let mut raw = vec![0u8; 64 * 1024];
                let mut splitter = AnnexBSplitter::new();
                loop {
                    match reader.read(&mut raw) {
                        Ok(0) => break,
                        Ok(n) => {
                            for fr in splitter.feed(&raw[..n]) {
                                if tx.send(fr).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                for fr in splitter.flush() {
                    let _ = tx.send(fr);
                }
            })
            .map_err(|e| format!("ffmpeg reader thread: {e}"))?;

        // Smoke-test: give the process a moment to die on bad args
        thread::sleep(Duration::from_millis(40));
        if let Some(status) = child.try_wait().ok().flatten() {
            // Retry once with a minimal arg set (older QSV / driver pickiness)
            drop(stdin);
            let _ = child.wait();
            return Self::new_minimal(settings, codec, &path, status.to_string());
        }

        let name = format!(
            "{} {}x{}@{} ({})",
            codec.label(),
            w,
            h,
            fps,
            codec.ffmpeg_name()
        );
        set_probe(format!("Using {name} via {}", path.display()));

        Ok(Self {
            child,
            stdin,
            rx,
            width: w,
            height: h,
            fps,
            name,
            pts_queue: VecDeque::new(),
            frames_in: 0,
            frames_out: 0,
            started: Instant::now(),
        })
    }

    /// Minimal flags for picky Intel HD 4000-era QSV.
    fn new_minimal(
        settings: EncoderSettings,
        codec: FfmpegCodec,
        path: &PathBuf,
        first_err: String,
    ) -> Result<Self, String> {
        let w = settings.width.max(16) & !1;
        let h = settings.height.max(16) & !1;
        let fps = settings.fps.max(1).min(60);
        let bitrate = settings.bitrate_bps.max(1_000_000);

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "nv12".into(),
            "-s".into(),
            format!("{w}x{h}"),
            "-r".into(),
            fps.to_string(),
            "-i".into(),
            "pipe:0".into(),
            "-an".into(),
            "-c:v".into(),
            codec.ffmpeg_name().into(),
            "-b:v".into(),
            bitrate.to_string(),
            "-g".into(),
            fps.to_string(),
            "-bf".into(),
            "0".into(),
            "-f".into(),
            "h264".into(),
            "pipe:1".into(),
        ];
        if matches!(codec, FfmpegCodec::X264) {
            args.extend([
                "-preset".into(),
                "ultrafast".into(),
                "-tune".into(),
                "zerolatency".into(),
            ]);
        }

        let mut cmd = Command::new(path);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn ffmpeg minimal: {e} (first: {first_err})"))?;
        let stdin = child.stdin.take().ok_or("ffmpeg stdin missing")?;
        let stdout = child.stdout.take().ok_or("ffmpeg stdout missing")?;
        let (tx, rx) = mpsc::sync_channel::<AnnexFrame>(8);
        thread::Builder::new()
            .name("lanplay-ffmpeg-out".into())
            .spawn(move || {
                let mut reader = stdout;
                let mut raw = vec![0u8; 64 * 1024];
                let mut splitter = AnnexBSplitter::new();
                loop {
                    match reader.read(&mut raw) {
                        Ok(0) => break,
                        Ok(n) => {
                            for fr in splitter.feed(&raw[..n]) {
                                if tx.send(fr).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                for fr in splitter.flush() {
                    let _ = tx.send(fr);
                }
            })
            .map_err(|e| format!("ffmpeg reader: {e}"))?;

        thread::sleep(Duration::from_millis(40));
        if let Some(status) = child.try_wait().ok().flatten() {
            return Err(format!(
                "ffmpeg {} failed full+minimal (exit {status}); first: {first_err}",
                codec.ffmpeg_name()
            ));
        }

        let name = format!(
            "{} {}x{}@{} ({} minimal)",
            codec.label(),
            w,
            h,
            fps,
            codec.ffmpeg_name()
        );
        set_probe(format!("Using {name}"));

        Ok(Self {
            child,
            stdin,
            rx,
            width: w,
            height: h,
            fps,
            name,
            pts_queue: VecDeque::new(),
            frames_in: 0,
            frames_out: 0,
            started: Instant::now(),
        })
    }

    fn drain_one(&mut self, wait: Duration) -> Option<AnnexFrame> {
        let deadline = Instant::now() + wait;
        loop {
            match self.rx.try_recv() {
                Ok(f) => return Some(f),
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

impl VideoEncoder for FfmpegEncoder {
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
        // Pipe mode: GOP handles periodic IDR. Optional future: restart or zmq.
    }

    fn encode_bgra(&mut self, bgra: &[u8], pts_us: u64) -> Result<Option<EncodedFrame>, String> {
        let nv12 = bgra_to_nv12(bgra, self.width, self.height)?;
        self.stdin
            .write_all(&nv12)
            .map_err(|e| format!("ffmpeg stdin write: {e}"))?;
        // Flush each frame so QSV sees it immediately
        self.stdin
            .flush()
            .map_err(|e| format!("ffmpeg stdin flush: {e}"))?;
        self.pts_queue.push_back(pts_us);
        self.frames_in += 1;

        // First frames may buffer; wait a bit longer until we have output.
        let wait = if self.frames_out == 0 {
            Duration::from_millis(250)
        } else {
            Duration::from_millis(80)
        };
        let Some(fr) = self.drain_one(wait) else {
            // Still warming up / async depth — not fatal
            if self.started.elapsed() > Duration::from_secs(3) && self.frames_out == 0 {
                return Err(
                    "ffmpeg produced no output for 3s — encoder may have failed (check Probe)"
                        .into(),
                );
            }
            return Ok(None);
        };

        self.frames_out += 1;
        let pts = self.pts_queue.pop_front().unwrap_or(pts_us);
        Ok(Some(EncodedFrame {
            data: fr.data,
            keyframe: fr.keyframe,
            pts_us: pts,
        }))
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        // Close stdin → ffmpeg drains and exits
        // ChildStdin dropped when replaced; take ownership via mem::replace pattern:
        // stdin is field — dropping Self drops stdin.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// --- Annex-B access unit splitter (single-slice frames from ffmpeg -f h264) ---

struct AnnexBSplitter {
    buf: Vec<u8>,
    pending: Vec<u8>,
    pending_key: bool,
}

impl AnnexBSplitter {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(256 * 1024),
            pending: Vec::new(),
            pending_key: false,
        }
    }

    fn feed(&mut self, data: &[u8]) -> Vec<AnnexFrame> {
        self.buf.extend_from_slice(data);
        self.extract(false)
    }

    fn flush(&mut self) -> Vec<AnnexFrame> {
        self.extract(true)
    }

    fn extract(&mut self, final_pass: bool) -> Vec<AnnexFrame> {
        let mut out = Vec::new();
        loop {
            let Some((sc_at, sc_len)) = find_start_code(&self.buf, 0) else {
                if final_pass {
                    self.buf.clear();
                } else if self.buf.len() > 4 {
                    // keep last 3 bytes (possible partial start code)
                    let keep = 3.min(self.buf.len());
                    let start = self.buf.len() - keep;
                    self.buf.drain(..start);
                }
                break;
            };
            if sc_at > 0 {
                // garbage before start code
                self.buf.drain(..sc_at);
                continue;
            }
            // Find next start code to bound this NAL
            let search_from = sc_len;
            let next = find_start_code(&self.buf, search_from);
            let nal_end = match next {
                Some((at, _)) => at,
                None if final_pass => self.buf.len(),
                None => break, // incomplete NAL
            };
            if nal_end <= sc_len {
                break;
            }
            let nal = self.buf[..nal_end].to_vec();
            self.buf.drain(..nal_end);

            let nal_type = nal_unit_type(&nal);
            let is_vcl = matches!(nal_type, 1 | 5);

            if is_vcl {
                self.pending.extend_from_slice(&nal);
                let key = self.pending_key || nal_type == 5;
                out.push(AnnexFrame {
                    data: std::mem::take(&mut self.pending),
                    keyframe: key,
                });
                self.pending_key = false;
            } else {
                // Parameter sets / SEI — attach to next VCL
                if nal_type == 7 || nal_type == 8 {
                    self.pending_key = true;
                }
                self.pending.extend_from_slice(&nal);
            }
        }
        out
    }
}

fn nal_unit_type(nal_with_sc: &[u8]) -> u8 {
    // skip start code
    let sc = if nal_with_sc.len() >= 4
        && nal_with_sc[0] == 0
        && nal_with_sc[1] == 0
        && nal_with_sc[2] == 0
        && nal_with_sc[3] == 1
    {
        4
    } else if nal_with_sc.len() >= 3
        && nal_with_sc[0] == 0
        && nal_with_sc[1] == 0
        && nal_with_sc[2] == 1
    {
        3
    } else {
        0
    };
    if nal_with_sc.len() > sc {
        nal_with_sc[sc] & 0x1f
    } else {
        0
    }
}

fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    if from >= data.len() {
        return None;
    }
    let mut i = from;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                return Some((i, 3));
            }
            if i + 3 < data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                return Some((i, 4));
            }
        }
        i += 1;
    }
    None
}

/// Try opening the best available FFmpeg HW encoder (or a specific one).
pub fn try_create(
    settings: EncoderSettings,
    prefer: Option<FfmpegCodec>,
) -> Result<FfmpegEncoder, String> {
    let caps = probe_ffmpeg_caps();
    if caps.path.is_none() {
        return Err(caps.detail);
    }

    let order: Vec<FfmpegCodec> = if let Some(c) = prefer {
        vec![c]
    } else {
        let mut o = auto_codec_order(&caps);
        if o.is_empty() {
            return Err(format!(
                "ffmpeg has no HW H.264 encoder ({})",
                caps.detail
            ));
        }
        o
    };

    let mut errors = Vec::new();
    for codec in order {
        match FfmpegEncoder::new(settings.clone(), codec) {
            Ok(enc) => return Ok(enc),
            Err(e) => errors.push(format!("{}: {e}", codec.ffmpeg_name())),
        }
    }
    Err(errors.join(" | "))
}

pub fn try_create_x264(settings: EncoderSettings) -> Result<FfmpegEncoder, String> {
    FfmpegEncoder::new(settings, FfmpegCodec::X264)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annexb_finds_start_codes() {
        let data = [0, 0, 0, 1, 0x65, 0xaa, 0, 0, 1, 0x41, 0xbb];
        assert_eq!(find_start_code(&data, 0), Some((0, 4)));
        assert_eq!(find_start_code(&data, 4), Some((6, 3)));
    }
}
