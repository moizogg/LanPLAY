//! Read local physical pads (client side).

/// Snapshot of one XInput-compatible pad.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicalPadState {
    pub connected: bool,
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

/// Poll XInput user index `user` (0..3). Non-Windows → always disconnected.
pub fn poll_xinput(user: u32) -> PhysicalPadState {
    #[cfg(windows)]
    {
        windows_xinput(user)
    }
    #[cfg(not(windows))]
    {
        let _ = user;
        PhysicalPadState::default()
    }
}

#[cfg(windows)]
fn windows_xinput(user: u32) -> PhysicalPadState {
    use windows::Win32::UI::Input::XboxController::{
        XInputGetState, XINPUT_STATE, XUSER_MAX_COUNT,
    };

    if user >= XUSER_MAX_COUNT {
        return PhysicalPadState::default();
    }

    let mut state = XINPUT_STATE::default();
    // ERROR_SUCCESS = 0; ERROR_DEVICE_NOT_CONNECTED = 1167
    let result = unsafe { XInputGetState(user, &mut state) };
    if result != 0 {
        return PhysicalPadState::default();
    }

    let g = state.Gamepad;
    PhysicalPadState {
        connected: true,
        // XINPUT_GAMEPAD_BUTTONS is a transparent u16 newtype.
        buttons: g.wButtons.0,
        left_trigger: g.bLeftTrigger,
        right_trigger: g.bRightTrigger,
        thumb_lx: g.sThumbLX,
        thumb_ly: g.sThumbLY,
        thumb_rx: g.sThumbRX,
        thumb_ry: g.sThumbRY,
    }
}
