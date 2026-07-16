//! Moonlight-style native stream window (low-latency present).
//!
//! Decoded RGBA is drawn here immediately — no JPEG / React poll on the hot path.
//! Local cursor is suppressed on this window (host cursor lives in the video).

#[cfg(windows)]
mod win {
    use minifb::{Key, ScaleMode, Window, WindowOptions};

    pub struct StreamWindow {
        window: Window,
        width: usize,
        height: usize,
        buf: Vec<u32>,
        cursor_hidden: bool,
    }

    impl StreamWindow {
        pub fn open(width: u32, height: u32) -> Result<Self, String> {
            let w = width.max(16) as usize;
            let h = height.max(16) as usize;
            let mut window = Window::new(
                "LANPlay Stream — host cursor only · Ctrl+Shift+Alt+Z release",
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
            window.set_cursor_visibility(false);
            let mut s = Self {
                window,
                width: w,
                height: h,
                buf: vec![0u32; w * h],
                cursor_hidden: false,
            };
            s.apply_cursor_hide();
            Ok(s)
        }

        fn apply_cursor_hide(&mut self) {
            self.window.set_cursor_visibility(false);
            // Also wipe Win32 class cursor + show-count (Moonlight SDL_DISABLE equivalent).
            let hwnd = self.window.get_window_handle() as isize;
            #[cfg(windows)]
            {
                // Best-effort; lanplay-input also hides globally while capture is ON.
                hide_hwnd_cursor(hwnd);
            }
            self.cursor_hidden = true;
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
                *self = Self::open(width, height)?;
            }
            self.apply_cursor_hide();
            for i in 0..(w * h) {
                let r = rgba[i * 4] as u32;
                let g = rgba[i * 4 + 1] as u32;
                let b = rgba[i * 4 + 2] as u32;
                self.buf[i] = (r << 16) | (g << 8) | b;
            }
            self.window
                .update_with_buffer(&self.buf, w, h)
                .map_err(|e| format!("present: {e}"))?;
            Ok(())
        }

        pub fn pump(&mut self) {
            self.apply_cursor_hide();
            let _ = self
                .window
                .update_with_buffer(&self.buf, self.width, self.height);
        }
    }

    fn hide_hwnd_cursor(hwnd: isize) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetClassLongPtrW, SetCursor, ShowCursor, GCLP_HCURSOR,
        };
        if hwnd == 0 {
            return;
        }
        unsafe {
            let h = HWND(hwnd as *mut _);
            let _ = SetClassLongPtrW(h, GCLP_HCURSOR, 0);
            use windows::Win32::UI::WindowsAndMessaging::HCURSOR;
            let _ = SetCursor(HCURSOR(std::ptr::null_mut()));
            while ShowCursor(false) >= 0 {}
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
