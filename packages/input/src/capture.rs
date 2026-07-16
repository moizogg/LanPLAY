//! Moonlight-style input capture state (client).
//!
//! - Capture ON: relative mouse + keyboard/mouse sent to host; **local cursor hidden**
//! - Capture OFF: local desktop works; empty KBM packets (raise-all-keys on host)
//! - Hotkey: Ctrl+Shift+Alt+Z to release capture (same spirit as Moonlight ungrab)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared capture flag between UI and client input thread.
#[derive(Clone, Debug)]
pub struct CaptureState {
    active: Arc<AtomicBool>,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl CaptureState {
    pub fn new(initially_active: bool) -> Self {
        Self {
            active: Arc::new(AtomicBool::new(initially_active)),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn set_active(&self, on: bool) {
        let was = self.active.swap(on, Ordering::SeqCst);
        if was != on {
            #[cfg(windows)]
            {
                if on {
                    relative_mouse::enter_relative_mode();
                } else {
                    relative_mouse::leave_relative_mode();
                }
            }
        }
    }

    pub fn toggle(&self) -> bool {
        let next = !self.is_active();
        self.set_active(next);
        next
    }

    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.active)
    }
}

/// Status for the UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub active: bool,
    pub hint: String,
}

impl CaptureStatus {
    pub fn from_state(s: &CaptureState) -> Self {
        let active = s.is_active();
        Self {
            active,
            hint: if active {
                "Capture ON — local cursor hidden; only host cursor in the stream. Ctrl+Shift+Alt+Z to release."
                    .into()
            } else {
                "Capture OFF — local desktop free. Click Capture / focus Stream window to control host."
                    .into()
            },
        }
    }
}

/// Moonlight-style relative mouse: hide local cursor, clip, recenter.
/// Host paints its own cursor into the video — client must never show a second one.
#[cfg(windows)]
pub mod relative_mouse {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        ClipCursor, GetCursorPos, GetSystemMetrics, SetCursor, SetCursorPos, ShowCursor,
        SM_CXSCREEN, SM_CYSCREEN,
    };

    static mut CENTER_X: i32 = 0;
    static mut CENTER_Y: i32 = 0;
    static mut RELATIVE: bool = false;
    static mut HIDE_DEPTH: i32 = 0;

    fn hide_cursor_deep() {
        unsafe {
            // ShowCursor is refcounted — drive it negative so nothing re-shows easily.
            loop {
                let c = ShowCursor(false);
                HIDE_DEPTH = c;
                if c < 0 {
                    break;
                }
            }
            // Blank system cursor for this thread / focused window.
            use windows::Win32::UI::WindowsAndMessaging::HCURSOR;
            let _ = SetCursor(HCURSOR(std::ptr::null_mut()));
        }
    }

    fn show_cursor_restore() {
        unsafe {
            loop {
                let c = ShowCursor(true);
                HIDE_DEPTH = c;
                if c >= 0 {
                    break;
                }
            }
            let _ = ClipCursor(None);
        }
    }

    /// Clip pointer to a 2×2 pixel box at screen center (can't wander / double-cursor).
    fn clip_to_center() {
        unsafe {
            let r = RECT {
                left: CENTER_X,
                top: CENTER_Y,
                right: CENTER_X + 2,
                bottom: CENTER_Y + 2,
            };
            let _ = ClipCursor(Some(&r));
        }
    }

    pub fn enter_relative_mode() {
        unsafe {
            let cx = GetSystemMetrics(SM_CXSCREEN);
            let cy = GetSystemMetrics(SM_CYSCREEN);
            CENTER_X = cx / 2;
            CENTER_Y = cy / 2;
            let _ = SetCursorPos(CENTER_X, CENTER_Y);
            clip_to_center();
            hide_cursor_deep();
            RELATIVE = true;
        }
    }

    pub fn leave_relative_mode() {
        unsafe {
            RELATIVE = false;
            show_cursor_restore();
        }
    }

    /// Call every input tick while captured (Moonlight re-asserts cursor hide).
    pub fn maintain_capture_cursor() {
        unsafe {
            if !RELATIVE {
                return;
            }
            hide_cursor_deep();
            clip_to_center();
            let _ = SetCursorPos(CENTER_X, CENTER_Y);
        }
    }

    /// Relative delta since last sample; recenters cursor when in relative mode.
    pub fn sample_relative_delta() -> (i16, i16) {
        unsafe {
            if !RELATIVE {
                return (0, 0);
            }
            // Re-hide every sample — Windows / minifb can resurrect the cursor.
            maintain_capture_cursor();

            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return (0, 0);
            }
            let dx = (pt.x - CENTER_X).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            let dy = (pt.y - CENTER_Y).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            let _ = SetCursorPos(CENTER_X, CENTER_Y);
            (dx, dy)
        }
    }

    pub fn is_relative() -> bool {
        unsafe { RELATIVE }
    }

    /// Hide cursor on a specific HWND (stream window class cursor → null).
    pub fn hide_on_hwnd(hwnd: isize) {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetClassLongPtrW, GCLP_HCURSOR,
        };
        if hwnd == 0 {
            return;
        }
        unsafe {
            let h = HWND(hwnd as *mut _);
            // Null class cursor so WM_SETCURSOR doesn't restore arrow.
            let _ = SetClassLongPtrW(h, GCLP_HCURSOR, 0);
            use windows::Win32::UI::WindowsAndMessaging::HCURSOR;
            let _ = SetCursor(HCURSOR(std::ptr::null_mut()));
            hide_cursor_deep();
        }
    }
}

/// Detect Moonlight-style ungrab combo: Ctrl+Shift+Alt+Z
#[cfg(windows)]
pub fn ungrab_hotkey_pressed() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe {
        let ctrl = GetAsyncKeyState(0x11) as u16 & 0x8000 != 0; // VK_CONTROL
        let shift = GetAsyncKeyState(0x10) as u16 & 0x8000 != 0; // VK_SHIFT
        let alt = GetAsyncKeyState(0x12) as u16 & 0x8000 != 0; // VK_MENU
        let z = GetAsyncKeyState(0x5A) as u16 & 0x8000 != 0; // Z
        ctrl && shift && alt && z
    }
}

#[cfg(not(windows))]
pub fn ungrab_hotkey_pressed() -> bool {
    false
}

#[cfg(not(windows))]
pub mod relative_mouse {
    pub fn enter_relative_mode() {}
    pub fn leave_relative_mode() {}
    pub fn maintain_capture_cursor() {}
    pub fn sample_relative_delta() -> (i16, i16) {
        (0, 0)
    }
    pub fn is_relative() -> bool {
        false
    }
    pub fn hide_on_hwnd(_hwnd: isize) {}
}
