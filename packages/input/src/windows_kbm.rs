//! Windows client sampling + host injection via SendInput.

use lanplay_protocol::{
    KbmPacket, KBM_FLAG_LBUTTON, KBM_FLAG_MBUTTON, KBM_FLAG_RBUTTON,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
    KEYBDINPUT, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, VIRTUAL_KEY, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::Foundation::POINT;

/// Tracks client cursor + previous key snapshot for deltas.
#[derive(Default)]
pub struct ClientKbmState {
    last_x: i32,
    last_y: i32,
    has_pos: bool,
}

/// Tracks which keys host thinks are down so we can emit KEYUP.
#[derive(Default)]
pub struct HostKbmState {
    keys_down: [u8; 8],
    key_count: u8,
    mouse_buttons: u8,
}

/// Sample mouse delta + held keys on the client PC.
pub fn sample_kbm_on_client(state: &mut ClientKbmState, seq: u32) -> KbmPacket {
    let mut flags = 0u8;
    if async_down(VK_LBUTTON.0 as i32) {
        flags |= KBM_FLAG_LBUTTON;
    }
    if async_down(VK_RBUTTON.0 as i32) {
        flags |= KBM_FLAG_RBUTTON;
    }
    if async_down(VK_MBUTTON.0 as i32) {
        flags |= KBM_FLAG_MBUTTON;
    }

    let (dx, dy) = mouse_delta(state);

    let mut keys = [0u8; 8];
    let mut key_count = 0u8;
    // Scan common VK range; pack up to 8 currently held keys (skip mouse buttons).
    for vk in 0x08u8..=0xFEu8 {
        if vk == VK_LBUTTON.0 as u8 || vk == VK_RBUTTON.0 as u8 || vk == VK_MBUTTON.0 as u8 {
            continue;
        }
        if async_down(i32::from(vk)) {
            if (key_count as usize) < 8 {
                keys[key_count as usize] = vk;
                key_count += 1;
            }
        }
    }

    let mut packet = KbmPacket {
        flags,
        seq,
        client_ts_us: 0,
        mouse_dx: dx,
        mouse_dy: dy,
        wheel: 0,
        keys,
        key_count,
    };
    packet.stamp_now();
    packet
}

/// Apply remote KBM on the host desktop.
pub fn apply_kbm_on_host(state: &mut HostKbmState, packet: &KbmPacket) {
    // Relative mouse move
    if packet.mouse_dx != 0 || packet.mouse_dy != 0 {
        send_mouse_move(packet.mouse_dx as i32, packet.mouse_dy as i32);
    }
    if packet.wheel != 0 {
        send_mouse_wheel(packet.wheel as i32);
    }

    // Mouse buttons edge detection
    let prev = state.mouse_buttons;
    let now = packet.flags;
    edge_mouse(prev, now, KBM_FLAG_LBUTTON, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP);
    edge_mouse(prev, now, KBM_FLAG_RBUTTON, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP);
    edge_mouse(
        prev,
        now,
        KBM_FLAG_MBUTTON,
        MOUSEEVENTF_MIDDLEDOWN,
        MOUSEEVENTF_MIDDLEUP,
    );
    state.mouse_buttons = now;

    // Keyboard: release keys no longer held, press new ones
    let new_keys = &packet.keys[..packet.key_count.min(8) as usize];
    let old_keys = &state.keys_down[..state.key_count.min(8) as usize];

    for &vk in old_keys {
        if vk != 0 && !new_keys.contains(&vk) {
            send_key(vk, false);
        }
    }
    for &vk in new_keys {
        if vk != 0 && !old_keys.contains(&vk) {
            send_key(vk, true);
        }
    }

    state.keys_down = packet.keys;
    state.key_count = packet.key_count.min(8);
}

fn async_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

fn mouse_delta(state: &mut ClientKbmState) -> (i16, i16) {
    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt) }.is_err() {
        return (0, 0);
    }
    if !state.has_pos {
        state.last_x = pt.x;
        state.last_y = pt.y;
        state.has_pos = true;
        return (0, 0);
    }
    let dx = (pt.x - state.last_x).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let dy = (pt.y - state.last_y).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    state.last_x = pt.x;
    state.last_y = pt.y;
    (dx, dy)
}

fn edge_mouse(prev: u8, now: u8, flag: u8, down: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS, up: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) {
    let was = prev & flag != 0;
    let is = now & flag != 0;
    if !was && is {
        send_mouse_button(down);
    } else if was && !is {
        send_mouse_button(up);
    }
}

fn send_mouse_move(dx: i32, dy: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(std::slice::from_ref(&input), std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_mouse_wheel(delta: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: (delta.wrapping_mul(120)) as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(std::slice::from_ref(&input), std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_mouse_button(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(std::slice::from_ref(&input), std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_key(vk: u8, down: bool) {
    let flags = if down {
        Default::default()
    } else {
        KEYEVENTF_KEYUP
    };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk as u16),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(std::slice::from_ref(&input), std::mem::size_of::<INPUT>() as i32);
    }
}

// silence unused if keyboard state used later
#[allow(dead_code)]
fn _keyboard_state_snapshot() -> [u8; 256] {
    let mut state = [0u8; 256];
    unsafe {
        let _ = GetKeyboardState(&mut state);
    }
    state
}
