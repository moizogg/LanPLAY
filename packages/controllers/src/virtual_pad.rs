//! Host-side virtual Xbox 360 pad (ViGEm).
//!
//! **ViGEmClient is statically linked** (Sunshine-style) — no ViGEmClient.dll beside the exe.
//! The **ViGEmBus kernel driver** still must be installed once (bundled setup + UAC).

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

/// Probe whether ViGEm bus is usable (does not keep a pad plugged).
pub fn probe_vigem() -> VigemStatus {
    #[cfg(windows)]
    {
        vigem_static::probe_only()
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

/// No-op backend when the bus is missing (still receives packets for metrics).
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
mod vigem_static {
    use super::*;
    use std::os::raw::{c_uint, c_void};

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

    // Linked from static lib built by build.rs (ViGEmClient.cpp)
    unsafe extern "C" {
        fn vigem_alloc() -> *mut c_void;
        fn vigem_free(vigem: *mut c_void);
        fn vigem_connect(vigem: *mut c_void) -> VigemError;
        fn vigem_disconnect(vigem: *mut c_void);
        fn vigem_target_x360_alloc() -> *mut c_void;
        fn vigem_target_free(target: *mut c_void);
        fn vigem_target_add(vigem: *mut c_void, target: *mut c_void) -> VigemError;
        fn vigem_target_remove(vigem: *mut c_void, target: *mut c_void) -> VigemError;
        fn vigem_target_x360_update(
            vigem: *mut c_void,
            target: *mut c_void,
            report: XusbReport,
        ) -> VigemError;
    }

    pub struct VigemX360 {
        client: *mut c_void,
        target: *mut c_void,
        plugged: bool,
    }

    unsafe impl Send for VigemX360 {}

    pub fn probe_only() -> VigemStatus {
        let client = unsafe { vigem_alloc() };
        if client.is_null() {
            return VigemStatus {
                available: false,
                detail: "vigem_alloc failed (out of memory?).".into(),
            };
        }
        let err = unsafe { vigem_connect(client) };
        unsafe {
            if err == VIGEM_ERROR_NONE {
                vigem_disconnect(client);
            }
            vigem_free(client);
        }

        if err != VIGEM_ERROR_NONE {
            let hint = if paths::bundled_driver_setup().is_some() {
                " Click “Install gamepad support” (one-time UAC). Driver is bundled with LANPlay."
            } else {
                " Install ViGEmBus driver, or re-download the full portable package."
            };
            return VigemStatus {
                available: false,
                detail: format!(
                    "ViGEmBus not ready (0x{err:08X}). Client lib is built into LANPlay.{hint}"
                ),
            };
        }

        VigemStatus {
            available: true,
            detail: "ViGEmBus ready (ViGEmClient statically linked).".into(),
        }
    }

    impl VigemX360 {
        pub fn try_open() -> Result<Self, String> {
            let client = unsafe { vigem_alloc() };
            if client.is_null() {
                return Err("vigem_alloc returned null".into());
            }

            let err = unsafe { vigem_connect(client) };
            if err != VIGEM_ERROR_NONE {
                unsafe { vigem_free(client) };
                let hint = if paths::bundled_driver_setup().is_some() {
                    " Use Host → Install gamepad support (bundled installer, one-time UAC)."
                } else {
                    ""
                };
                return Err(format!(
                    "vigem_connect failed (0x{err:08X}). ViGEmBus driver not installed.{hint}"
                ));
            }

            let target = unsafe { vigem_target_x360_alloc() };
            if target.is_null() {
                unsafe {
                    vigem_disconnect(client);
                    vigem_free(client);
                }
                return Err("vigem_target_x360_alloc returned null".into());
            }

            let err = unsafe { vigem_target_add(client, target) };
            if err != VIGEM_ERROR_NONE {
                unsafe {
                    vigem_target_free(target);
                    vigem_disconnect(client);
                    vigem_free(client);
                }
                return Err(format!(
                    "vigem_target_add failed (0x{err:08X}). Could not plug virtual Xbox 360."
                ));
            }

            Ok(Self {
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

            let err = unsafe { vigem_target_x360_update(self.client, self.target, report) };
            if err != VIGEM_ERROR_NONE {
                return Err(format!("vigem_target_x360_update failed 0x{err:08X}"));
            }
            Ok(())
        }

        fn unplug(&mut self) -> Result<(), String> {
            if self.plugged && !self.client.is_null() && !self.target.is_null() {
                unsafe {
                    let _ = vigem_target_remove(self.client, self.target);
                    vigem_target_free(self.target);
                    vigem_disconnect(self.client);
                    vigem_free(self.client);
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
                    "ViGEm Xbox 360 virtual pad active (static client).".into()
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
}

#[cfg(windows)]
pub use vigem_static::VigemX360;
