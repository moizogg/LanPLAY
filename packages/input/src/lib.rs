//! Keyboard + mouse: client capture → packets → host `SendInput`.

#[cfg(windows)]
mod windows_kbm;

#[cfg(windows)]
pub use windows_kbm::{apply_kbm_on_host, sample_kbm_on_client, ClientKbmState, HostKbmState};

#[cfg(not(windows))]
pub mod stub {
    use lanplay_protocol::KbmPacket;

    #[derive(Default)]
    pub struct ClientKbmState;
    #[derive(Default)]
    pub struct HostKbmState;

    pub fn sample_kbm_on_client(_state: &mut ClientKbmState, _seq: u32) -> KbmPacket {
        KbmPacket {
            flags: 0,
            seq: 0,
            client_ts_us: 0,
            mouse_dx: 0,
            mouse_dy: 0,
            wheel: 0,
            keys: [0; 8],
            key_count: 0,
        }
    }

    pub fn apply_kbm_on_host(_state: &mut HostKbmState, _packet: &KbmPacket) {}
}

#[cfg(not(windows))]
pub use stub::*;
