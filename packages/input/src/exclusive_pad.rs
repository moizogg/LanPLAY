//! Best-effort exclusive open of gamepad HID devices on the **client**.
//!
//! While the session is active we try to open matching HID paths with exclusive
//! share mode so local apps are less likely to use the pad.
//! Note: pure XInput Xbox pads may still show in XInput without HidHide.

#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING,
};

/// GUID_DEVINTERFACE_HID {4D1E55B2-F16F-11CF-88CB-001111000030}
const GUID_DEVINTERFACE_HID: GUID = GUID {
    data1: 0x4D1E55B2,
    data2: 0xF16F,
    data3: 0x11CF,
    data4: [0x88, 0xCB, 0x00, 0x11, 0x11, 0x00, 0x00, 0x30],
};

/// Holds exclusive HID handles until dropped.
pub struct ExclusivePadGuard {
    handles: Vec<HANDLE>,
}

impl ExclusivePadGuard {
    pub fn acquire() -> Self {
        let mut handles = Vec::new();
        for path in enumerate_hid_paths() {
            if !looks_like_gamepad(&path) {
                continue;
            }
            if let Some(h) = open_exclusive(&path) {
                handles.push(h);
            }
        }
        Self { handles }
    }

    pub fn count(&self) -> usize {
        self.handles.len()
    }
}

impl Drop for ExclusivePadGuard {
    fn drop(&mut self) {
        for h in self.handles.drain(..) {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

fn looks_like_gamepad(path: &str) -> bool {
    let u = path.to_ascii_uppercase();
    u.contains("IG_")
        || u.contains("XUSB")
        || u.contains("XINPUT")
        || u.contains("GAMEPAD")
        || u.contains("JOYSTICK")
        || u.contains("VID_045E")
        || u.contains("VID_054C")
        || u.contains("VID_057E")
}

fn open_exclusive(path: &str) -> Option<HANDLE> {
    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        // FILE_SHARE_MODE(0) = exclusive
        let h = CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .ok()?;
        if h.is_invalid() {
            None
        } else {
            Some(h)
        }
    }
}

fn enumerate_hid_paths() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let Ok(hdev) = SetupDiGetClassDevsW(
            Some(&GUID_DEVINTERFACE_HID),
            PCWSTR::null(),
            HWND::default(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) else {
            return out;
        };

        let mut index = 0u32;
        loop {
            let mut ifdata = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                InterfaceClassGuid: GUID::zeroed(),
                Flags: 0,
                Reserved: 0,
            };
            if SetupDiEnumDeviceInterfaces(
                hdev,
                None,
                &GUID_DEVINTERFACE_HID,
                index,
                &mut ifdata,
            )
            .is_err()
            {
                break;
            }

            let mut required = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &ifdata,
                None,
                0,
                Some(&mut required),
                None,
            );
            if required < 8 {
                index += 1;
                continue;
            }

            let mut buf = vec![0u8; required as usize];
            // SAFETY: buffer is large enough for SP_DEVICE_INTERFACE_DETAIL_DATA_W + path
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = if cfg!(target_pointer_width = "64") {
                8
            } else {
                std::mem::size_of::<u32>() as u32 + 2
            };

            if SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &ifdata,
                Some(detail),
                required,
                None,
                None,
            )
            .is_ok()
            {
                let path_ptr = (*detail).DevicePath.as_ptr();
                let path = wide_cstr(path_ptr);
                if !path.is_empty() {
                    out.push(path);
                }
            }
            index += 1;
        }

        let _ = SetupDiDestroyDeviceInfoList(hdev);
    }
    out
}

unsafe fn wide_cstr(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
        if len > 1024 {
            break;
        }
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
}
