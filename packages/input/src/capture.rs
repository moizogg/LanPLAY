//! Moonlight-style input capture state (client).
//!
//! - Capture ON: relative mouse + keyboard/mouse sent to host
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
                "Input capture ON — mouse/keyboard go to host. Press Ctrl+Shift+Alt+Z to release."
                    .into()
            } else {
                "Input capture OFF — use this PC normally. Click Capture or the window to control host."
                    .into()
            },
        }
    }
}

#[cfg(windows)]
pub mod relative_mouse {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SetCursorPos, ShowCursor, SM_CXSCREEN, SM_CYSCREEN,
    };

    static mut CENTER_X: i32 = 0;
    static mut CENTER_Y: i32 = 0;
    static mut RELATIVE: bool = false;

    pub fn enter_relative_mode() {
        unsafe {
            let cx = GetSystemMetrics(SM_CXSCREEN);
            let cy = GetSystemMetrics(SM_CYSCREEN);
            CENTER_X = cx / 2;
            CENTER_Y = cy / 2;
            let _ = SetCursorPos(CENTER_X, CENTER_Y);
            // Hide cursor (reference count style API)
            while ShowCursor(false) >= 0 {}
            RELATIVE = true;
        }
    }

    pub fn leave_relative_mode() {
        unsafe {
            RELATIVE = false;
            while ShowCursor(true) < 0 {}
        }
    }

    /// Relative delta since last sample; recenters cursor when in relative mode.
    pub fn sample_relative_delta() -> (i16, i16) {
        unsafe {
            if !RELATIVE {
                return (0, 0);
            }
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
    pub fn sample_relative_delta() -> (i16, i16) {
        (0, 0)
    }
    pub fn is_relative() -> bool {
        false
    }
}
