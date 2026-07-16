//! Wire protocol definitions for LANPlay.
//!
//! Phase 1 only defines versioning. Real packet codecs land in later phases.

/// Protocol major.minor — bump when packets become incompatible.
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
