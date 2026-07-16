//! Wire protocol definitions for LANPlay.

mod input_packet;

pub use input_packet::{
    InputPacket, DecodeError, FLAG_CONNECTED, INPUT_PACKET_MAGIC, INPUT_PACKET_SIZE,
    INPUT_PACKET_VERSION,
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
