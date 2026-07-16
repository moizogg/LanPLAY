//! Global low-level mouse hook to capture wheel scroll (not available via GetAsyncKeyState).

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;
use std::thread;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
};

static WHEEL_NOTCHES: AtomicI32 = AtomicI32::new(0);
static HOOK_RUNNING: AtomicBool = AtomicBool::new(false);
static HOOK_HANDLE: OnceLock<std::sync::Mutex<Option<HHOOK>>> = OnceLock::new();

fn hook_slot() -> &'static std::sync::Mutex<Option<HHOOK>> {
    HOOK_HANDLE.get_or_init(|| std::sync::Mutex::new(None))
}

/// Take accumulated vertical scroll notches since last sample (WHEEL_DELTA/120 units).
pub fn take_wheel_notches() -> i16 {
    let v = WHEEL_NOTCHES.swap(0, Ordering::AcqRel);
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

unsafe extern "system" fn mouse_ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
            // mouseData high word = signed delta (usually ±120 per notch)
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let raw = ((info.mouseData >> 16) as i16) as i32;
            // Convert to notch count; keep fractional accumulation via raw/120
            let notches = raw / 120;
            if notches != 0 {
                // Vertical only for now (host applies MOUSEEVENTF_WHEEL)
                if msg == WM_MOUSEWHEEL {
                    WHEEL_NOTCHES.fetch_add(notches, Ordering::Relaxed);
                }
                // Horizontal: map to vertical host scroll for basic support
                if msg == WM_MOUSEHWHEEL {
                    WHEEL_NOTCHES.fetch_add(notches, Ordering::Relaxed);
                }
            }
        }
    }
    CallNextHookEx(
        HHOOK::default(),
        code,
        wparam,
        lparam,
    )
}

/// Start the wheel hook + message pump (idempotent).
pub fn ensure_wheel_hook() {
    if HOOK_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }

    let _ = thread::Builder::new()
        .name("lanplay-wheel-hook".into())
        .spawn(|| {
            unsafe {
                let hook = match SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(mouse_ll_proc),
                    None, // process-local for LL mouse usually needs hmod; None often works for LL
                    0,
                ) {
                    Ok(h) => h,
                    Err(_) => {
                        // Retry with module handle of current process
                        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
                        let hmod = GetModuleHandleW(None).ok();
                        match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_ll_proc), hmod, 0) {
                            Ok(h) => h,
                            Err(_) => {
                                HOOK_RUNNING.store(false, Ordering::SeqCst);
                                return;
                            }
                        }
                    }
                };

                if let Ok(mut g) = hook_slot().lock() {
                    *g = Some(hook);
                }

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                if let Ok(mut g) = hook_slot().lock() {
                    if let Some(h) = g.take() {
                        let _ = UnhookWindowsHookEx(h);
                    }
                }
                HOOK_RUNNING.store(false, Ordering::SeqCst);
            }
        });
}

/// Stop hook (best-effort; process exit also cleans up).
pub fn stop_wheel_hook() {
    // Post quit to the hook thread by unhooking; LL hooks don't need GetMessage forever if we unhook from elsewhere
    if let Ok(mut g) = hook_slot().lock() {
        if let Some(h) = g.take() {
            unsafe {
                let _ = UnhookWindowsHookEx(h);
            }
        }
    }
    HOOK_RUNNING.store(false, Ordering::SeqCst);
    WHEEL_NOTCHES.store(0, Ordering::SeqCst);
}
