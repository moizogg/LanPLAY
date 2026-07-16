//! Host-side virtual Xbox 360 pad (ViGEm).
//!
//! Loads **bundled** `ViGEmClient.dll` from LANPlay resources (no manual PATH).
//! Kernel **ViGEmBus** driver is installed once via our bundled setup (UAC).

use crate::paths;
use lanplay_protocol::InputPacket;

#[derive(Debug, Clone)]
pub struct VigemStatus {
    pub available: bool,
    pub detail: String,
}

/// Backend that can apply remote gamepad state onto a virtual pad.
pub trait VirtualPadBackend: Send {
    fn apply(&mut self, packet: &InputPacket) -> Result<(), String>;
    fn unplug(&mut self) -> Result<(), String>;
    fn is_active(&self) -> bool;
    fn status(&self) -> VigemStatus;
}

/// Probe whether ViGEm can be used on this machine (does not plug a pad).
pub fn probe_vigem() -> VigemStatus {
    #[cfg(windows)]
    {
        vigem_ffi::probe_only()
    }
    #[cfg(not(windows))]
    {
        VigemStatus {
            available: false,
            detail: "ViGEm is only supported on Windows.".into(),
        }
    }
}

/// Create a virtual pad backend for the host loop.
pub fn create_virtual_pad() -> Result<Box<dyn VirtualPadBackend>, String> {
    #[cfg(windows)]
    {
        Ok(Box::new(VigemX360::try_open()?))
    }
    #[cfg(not(windows))]
    {
        Err("ViGEm is only supported on Windows.".into())
    }
}

/// No-op backend: still receives packets (useful if ViGEm missing).
pub struct NullVirtualPad {
    detail: String,
    active: bool,
}

impl NullVirtualPad {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            detail: reason.into(),
            active: false,
        }
    }
}

impl VirtualPadBackend for NullVirtualPad {
    fn apply(&mut self, packet: &InputPacket) -> Result<(), String> {
        self.active = packet.is_connected();
        Ok(())
    }

    fn unplug(&mut self) -> Result<(), String> {
        self.active = false;
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn status(&self) -> VigemStatus {
        VigemStatus {
            available: false,
            detail: self.detail.clone(),
        }
    }
}

#[cfg(windows)]
mod vigem_ffi {
    use super::*;
    use libloading::Library;
    use std::os::raw::{c_uint, c_void};
    use std::path::Path;

    type VigemError = c_uint;
    const VIGEM_ERROR_NONE: VigemError = 0x2000_0000;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct XusbReport {
        w_buttons: u16,
        b_left_trigger: u8,
        b_right_trigger: u8,
        s_thumb_lx: i16,
        s_thumb_ly: i16,
        s_thumb_rx: i16,
        s_thumb_ry: i16,
    }

    type FnAlloc = unsafe extern "C" fn() -> *mut c_void;
    type FnFree = unsafe extern "C" fn(*mut c_void);
    type FnConnect = unsafe extern "C" fn(*mut c_void) -> VigemError;
    type FnDisconnect = unsafe extern "C" fn(*mut c_void);
    type FnTargetX360Alloc = unsafe extern "C" fn() -> *mut c_void;
    type FnTargetFree = unsafe extern "C" fn(*mut c_void);
    type FnTargetAdd = unsafe extern "C" fn(*mut c_void, *mut c_void) -> VigemError;
    type FnTargetRemove = unsafe extern "C" fn(*mut c_void, *mut c_void) -> VigemError;
    type FnTargetX360Update =
        unsafe extern "C" fn(*mut c_void, *mut c_void, XusbReport) -> VigemError;

    struct Api {
        _lib: Library,
        free: FnFree,
        disconnect: FnDisconnect,
        target_free: FnTargetFree,
        target_remove: FnTargetRemove,
        target_x360_update: FnTargetX360Update,
    }

    pub struct VigemX360 {
        api: Api,
        client: *mut c_void,
        target: *mut c_void,
        plugged: bool,
    }

    unsafe impl Send for VigemX360 {}

    fn load_vigem_library() -> Result<(Library, String), String> {
        let mut errors = Vec::new();
        for candidate in paths::vigem_client_dll_candidates() {
            match unsafe { Library::new(&candidate) } {
                Ok(lib) => {
                    let label = candidate.display().to_string();
                    return Ok((lib, label));
                }
                Err(e) => {
                    if candidate.is_file() || candidate.as_os_str() == "ViGEmClient.dll" {
                        errors.push(format!("{} → {e}", candidate.display()));
                    }
                }
            }
        }
        let setup = paths::bundled_driver_setup()
            .map(|p| format!(" Bundled driver setup is at {} — use Install gamepad support.", p.display()))
            .unwrap_or_default();
        Err(format!(
            "Could not load ViGEmClient.dll (bundled with LANPlay).{setup} Details: {}",
            errors.join("; ")
        ))
    }

    fn resolve_api(lib: &Library) -> Result<(FnAlloc, FnFree, FnConnect, FnDisconnect, FnTargetX360Alloc, FnTargetFree, FnTargetAdd, FnTargetRemove, FnTargetX360Update), String> {
        unsafe {
            let alloc: FnAlloc = *lib
                .get(b"vigem_alloc")
                .map_err(|e| format!("vigem_alloc: {e}"))?;
            let free: FnFree = *lib
                .get(b"vigem_free")
                .map_err(|e| format!("vigem_free: {e}"))?;
            let connect: FnConnect = *lib
                .get(b"vigem_connect")
                .map_err(|e| format!("vigem_connect: {e}"))?;
            let disconnect: FnDisconnect = *lib
                .get(b"vigem_disconnect")
                .map_err(|e| format!("vigem_disconnect: {e}"))?;
            let target_x360_alloc: FnTargetX360Alloc = *lib
                .get(b"vigem_target_x360_alloc")
                .map_err(|e| format!("vigem_target_x360_alloc: {e}"))?;
            let target_free: FnTargetFree = *lib
                .get(b"vigem_target_free")
                .map_err(|e| format!("vigem_target_free: {e}"))?;
            let target_add: FnTargetAdd = *lib
                .get(b"vigem_target_add")
                .map_err(|e| format!("vigem_target_add: {e}"))?;
            let target_remove: FnTargetRemove = *lib
                .get(b"vigem_target_remove")
                .map_err(|e| format!("vigem_target_remove: {e}"))?;
            let target_x360_update: FnTargetX360Update = *lib
                .get(b"vigem_target_x360_update")
                .map_err(|e| format!("vigem_target_x360_update: {e}"))?;
            Ok((
                alloc,
                free,
                connect,
                disconnect,
                target_x360_alloc,
                target_free,
                target_add,
                target_remove,
                target_x360_update,
            ))
        }
    }

    pub fn probe_only() -> VigemStatus {
        let (lib, from) = match load_vigem_library() {
            Ok(v) => v,
            Err(e) => {
                return VigemStatus {
                    available: false,
                    detail: e,
                };
            }
        };

        let (alloc, free, connect, disconnect, ..) = match resolve_api(&lib) {
            Ok(v) => v,
            Err(e) => {
                return VigemStatus {
                    available: false,
                    detail: e,
                };
            }
        };

        let client = unsafe { alloc() };
        if client.is_null() {
            return VigemStatus {
                available: false,
                detail: "vigem_alloc returned null".into(),
            };
        }
        let err = unsafe { connect(client) };
        unsafe {
            if err == VIGEM_ERROR_NONE {
                disconnect(client);
            }
            free(client);
        }
        drop(lib);

        if err != VIGEM_ERROR_NONE {
            let hint = if paths::bundled_driver_setup().is_some() {
                " Click “Install gamepad support” in LANPlay (one-time, built-in installer)."
            } else {
                " Driver not installed and no bundled setup found."
            };
            return VigemStatus {
                available: false,
                detail: format!(
                    "ViGEm bus not ready (0x{err:08X}) using {from}.{hint}"
                ),
            };
        }
        VigemStatus {
            available: true,
            detail: format!("ViGEm ready (loaded from {from})."),
        }
    }

    impl VigemX360 {
        pub fn try_open() -> Result<Self, String> {
            let (lib, _from) = load_vigem_library()?;
            let (
                alloc,
                free,
                connect,
                disconnect,
                target_x360_alloc,
                target_free,
                target_add,
                target_remove,
                target_x360_update,
            ) = resolve_api(&lib)?;

            let client = unsafe { alloc() };
            if client.is_null() {
                return Err("vigem_alloc returned null".into());
            }

            let err = unsafe { connect(client) };
            if err != VIGEM_ERROR_NONE {
                unsafe { free(client) };
                let hint = if paths::bundled_driver_setup().is_some() {
                    " Use LANPlay → Install gamepad support (bundled, one-time UAC)."
                } else {
                    ""
                };
                return Err(format!(
                    "vigem_connect failed (0x{err:08X}). Virtual gamepad driver not installed.{hint}"
                ));
            }

            let target = unsafe { target_x360_alloc() };
            if target.is_null() {
                unsafe {
                    disconnect(client);
                    free(client);
                }
                return Err("vigem_target_x360_alloc returned null".into());
            }

            let err = unsafe { target_add(client, target) };
            if err != VIGEM_ERROR_NONE {
                unsafe {
                    target_free(target);
                    disconnect(client);
                    free(client);
                }
                return Err(format!(
                    "vigem_target_add failed (0x{err:08X}). Could not plug virtual Xbox 360."
                ));
            }

            Ok(Self {
                api: Api {
                    _lib: lib,
                    free,
                    disconnect,
                    target_free,
                    target_remove,
                    target_x360_update,
                },
                client,
                target,
                plugged: true,
            })
        }
    }

    impl VirtualPadBackend for VigemX360 {
        fn apply(&mut self, packet: &InputPacket) -> Result<(), String> {
            if !self.plugged {
                return Err("virtual pad not plugged".into());
            }

            let report = if packet.is_connected() {
                XusbReport {
                    w_buttons: packet.buttons,
                    b_left_trigger: packet.left_trigger,
                    b_right_trigger: packet.right_trigger,
                    s_thumb_lx: packet.thumb_lx,
                    s_thumb_ly: packet.thumb_ly,
                    s_thumb_rx: packet.thumb_rx,
                    s_thumb_ry: packet.thumb_ry,
                }
            } else {
                XusbReport::default()
            };

            let err = unsafe { (self.api.target_x360_update)(self.client, self.target, report) };
            if err != VIGEM_ERROR_NONE {
                return Err(format!("vigem_target_x360_update failed 0x{err:08X}"));
            }
            Ok(())
        }

        fn unplug(&mut self) -> Result<(), String> {
            if self.plugged && !self.client.is_null() && !self.target.is_null() {
                unsafe {
                    let _ = (self.api.target_remove)(self.client, self.target);
                    (self.api.target_free)(self.target);
                    (self.api.disconnect)(self.client);
                    (self.api.free)(self.client);
                }
                self.target = std::ptr::null_mut();
                self.client = std::ptr::null_mut();
                self.plugged = false;
            }
            Ok(())
        }

        fn is_active(&self) -> bool {
            self.plugged
        }

        fn status(&self) -> VigemStatus {
            VigemStatus {
                available: self.plugged,
                detail: if self.plugged {
                    "ViGEm Xbox 360 virtual pad active.".into()
                } else {
                    "ViGEm pad inactive.".into()
                },
            }
        }
    }

    impl Drop for VigemX360 {
        fn drop(&mut self) {
            let _ = self.unplug();
        }
    }

    #[allow(dead_code)]
    fn _path_hint(p: &Path) -> String {
        p.display().to_string()
    }
}

#[cfg(windows)]
pub use vigem_ffi::VigemX360;
