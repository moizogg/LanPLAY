//! Wire protocol definitions for LANPlay.

mod input_packet;
mod kbm_packet;
mod video_packet;

pub use input_packet::{
    DecodeError as InputDecodeError, InputPacket, FLAG_CONNECTED, INPUT_PACKET_MAGIC,
    INPUT_PACKET_SIZE, INPUT_PACKET_VERSION,
};
pub use kbm_packet::{
    DecodeError as KbmDecodeError, KbmPacket, KBM_FLAG_LBUTTON, KBM_FLAG_MBUTTON, KBM_FLAG_RBUTTON,
    KBM_PACKET_MAGIC, KBM_PACKET_SIZE, KBM_PACKET_VERSION,
};
pub use video_packet::{
    fragment_access_unit, FrameReassembler, ReassembledFrame, VideoFragmentHeader,
    VIDEO_FLAG_KEYFRAME, VIDEO_HEADER_SIZE, VIDEO_MAX_PAYLOAD, VIDEO_PACKET_MAGIC,
    VIDEO_PACKET_VERSION,
};

/// Protocol major — bump when packets become incompatible.
pub const PROTOCOL_VERSION: u16 = 1;

/// Human-readable protocol label for handshake logs.
pub const PROTOCOL_NAME: &str = "lanplay";

/// Logical channels (multiplexed later over the media socket).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Channel {
    Control = 0,
    Input = 1,
    Video = 2,
    Audio = 3,
    Metrics = 4,
}

/// Peek magic of a UDP payload.
pub fn packet_magic(buf: &[u8]) -> Option<u32> {
    if buf.len() < 4 {
        return None;
    }
    Some(u32::from_le_bytes(buf[0..4].try_into().ok()?))
}
