//! Best-effort exclusive open of gamepad HID devices on the **client**.
//!
//! Goal: while LANPlay is streaming, the physical pad is "held" so local games
//! on the client are less likely to use it. Xbox XInput pads may still appear
//! to XInput (Windows limitation without HidHide); HID-class pads often block.

#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING,
};
use windows::Win32::UI::WindowsAndMessaging::HWND_MESSAGE;

// GUID_DEVINTERFACE_HID
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
    /// Try to exclusively open HID interfaces that look like game controllers.
    pub fn acquire() -> Self {
        let mut handles = Vec::new();
        let paths = enumerate_hid_paths();
        for path in paths {
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
    // Common Xbox / generic gamepad path markers
    u.contains("IG_")
        || u.contains("XUSB")
        || u.contains("XINPUT")
        || u.contains("GAMEPAD")
        || u.contains("JOYSTICK")
        || u.contains("VID_045E") // Microsoft
        || u.contains("VID_054C") // Sony
        || u.contains("VID_057E") // Nintendo
}

fn open_exclusive(path: &str) -> Option<HANDLE> {
    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        // Share mode 0 = exclusive; fails if another app has the device open.
        let h = CreateFileW(
            PCWSTR(wide.as_ptr()),
            (GENERIC_READ.0 | GENERIC_WRITE.0) as u32,
            FILE_SHARE_NONE,
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
        let hdev: HDEVINFO = match SetupDiGetClassDevsW(
            Some(&GUID_DEVINTERFACE_HID),
            PCWSTR::null(),
            HWND(ptr::null_mut()),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) {
            Ok(h) => h,
            Err(_) => return out,
        };

        let mut index = 0u32;
        loop {
            let mut ifdata = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                InterfaceClassGuid: GUID::default(),
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
            if required == 0 {
                index += 1;
                continue;
            }

            let mut buf = vec![0u8; required as usize];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = if cfg!(target_pointer_width = "64") {
                8
            } else {
                6
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
                // DevicePath is a flexible array at the end of the struct
                let path_ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
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
