//! Global low-level mouse hook to capture wheel scroll (not available via GetAsyncKeyState).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
use std::thread;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
};

static WHEEL_NOTCHES: AtomicI32 = AtomicI32::new(0);
static HOOK_RUNNING: AtomicBool = AtomicBool::new(false);
/// Raw HHOOK pointer stored as isize so the static can be Sync.
static HOOK_PTR: AtomicIsize = AtomicIsize::new(0);

/// Take accumulated vertical scroll notches since last sample (WHEEL_DELTA/120 units).
pub fn take_wheel_notches() -> i16 {
    let v = WHEEL_NOTCHES.swap(0, Ordering::AcqRel);
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

unsafe extern "system" fn mouse_ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let raw = ((info.mouseData >> 16) as i16) as i32;
            let notches = raw / 120;
            if notches != 0 {
                WHEEL_NOTCHES.fetch_add(notches, Ordering::Relaxed);
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

/// Start the wheel hook + message pump (idempotent).
pub fn ensure_wheel_hook() {
    if HOOK_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }

    let _ = thread::Builder::new()
        .name("lanplay-wheel-hook".into())
        .spawn(|| unsafe {
            let hmod = match GetModuleHandleW(None) {
                Ok(h) => HINSTANCE(h.0),
                Err(_) => {
                    HOOK_RUNNING.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_ll_proc), hmod, 0) {
                Ok(h) => h,
                Err(_) => {
                    HOOK_RUNNING.store(false, Ordering::SeqCst);
                    return;
                }
            };

            HOOK_PTR.store(hook.0 as isize, Ordering::SeqCst);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let ptr = HOOK_PTR.swap(0, Ordering::SeqCst);
            if ptr != 0 {
                let _ = UnhookWindowsHookEx(HHOOK(ptr as *mut _));
            }
            HOOK_RUNNING.store(false, Ordering::SeqCst);
        });
}

/// Stop hook (best-effort).
pub fn stop_wheel_hook() {
    let ptr = HOOK_PTR.swap(0, Ordering::SeqCst);
    if ptr != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(ptr as *mut _));
        }
    }
    HOOK_RUNNING.store(false, Ordering::SeqCst);
    WHEEL_NOTCHES.store(0, Ordering::SeqCst);
}
