//! Moonlight-style native stream window (low-latency present).
//!
//! Decoded RGBA is drawn here immediately — no JPEG / React poll on the hot path.

#[cfg(windows)]
mod win {
    use minifb::{Key, ScaleMode, Window, WindowOptions};

    pub struct StreamWindow {
        window: Window,
        width: usize,
        height: usize,
        buf: Vec<u32>,
    }

    impl StreamWindow {
        pub fn open(width: u32, height: u32) -> Result<Self, String> {
            let w = width.max(16) as usize;
            let h = height.max(16) as usize;
            let mut window = Window::new(
                "LANPlay Stream — click here to control · Ctrl+Shift+Alt+Z release",
                w,
                h,
                WindowOptions {
                    resize: true,
                    scale_mode: ScaleMode::AspectRatioStretch,
                    topmost: false,
                    ..WindowOptions::default()
                },
            )
            .map_err(|e| format!("stream window: {e}"))?;
            window.set_target_fps(0); // present ASAP
            Ok(Self {
                window,
                width: w,
                height: h,
                buf: vec![0u32; w * h],
            })
        }

        pub fn is_open(&self) -> bool {
            self.window.is_open() && !self.window.is_key_down(Key::Escape)
        }

        /// Present tight RGBA8 (R,G,B,A).
        pub fn present_rgba(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
            let w = width as usize;
            let h = height as usize;
            if w == 0 || h == 0 || rgba.len() < w * h * 4 {
                return Ok(());
            }
            if w != self.width || h != self.height {
                // Recreate window at new size
                *self = Self::open(width, height)?;
            }
            for i in 0..(w * h) {
                let r = rgba[i * 4] as u32;
                let g = rgba[i * 4 + 1] as u32;
                let b = rgba[i * 4 + 2] as u32;
                // minifb 0RGB
                self.buf[i] = (r << 16) | (g << 8) | b;
            }
            self.window
                .update_with_buffer(&self.buf, w, h)
                .map_err(|e| format!("present: {e}"))?;
            Ok(())
        }

        pub fn pump(&mut self) {
            // Keep OS events flowing even without a new frame.
            let _ = self.window.update_with_buffer(&self.buf, self.width, self.height);
        }
    }
}

#[cfg(windows)]
pub use win::StreamWindow;

#[cfg(not(windows))]
pub struct StreamWindow;

#[cfg(not(windows))]
impl StreamWindow {
    pub fn open(_w: u32, _h: u32) -> Result<Self, String> {
        Err("Stream window only on Windows".into())
    }
    pub fn is_open(&self) -> bool {
        false
    }
    pub fn present_rgba(&mut self, _: &[u8], _: u32, _: u32) -> Result<(), String> {
        Ok(())
    }
    pub fn pump(&mut self) {}
}
