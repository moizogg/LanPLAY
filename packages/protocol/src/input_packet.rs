//! Fixed-size binary controller packet (Phase 2).
//!
//! Layout (little-endian, 32 bytes):
//! ```text
//! magic u32 | ver u8 | controller_id u8 | flags u8 | _pad u8
//! seq u32 | client_ts_us u64
//! buttons u16 | lt u8 | rt u8
//! lx i16 | ly i16 | rx i16 | ry i16
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

/// ASCII "LPIP" — LANPlay Input Packet.
pub const INPUT_PACKET_MAGIC: u32 = 0x4C50_4950;
pub const INPUT_PACKET_VERSION: u8 = 1;
pub const INPUT_PACKET_SIZE: usize = 32;

pub const FLAG_CONNECTED: u8 = 1 << 0;

/// One snapshot of a remote gamepad (XInput / X360 layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputPacket {
    pub controller_id: u8,
    pub flags: u8,
    pub seq: u32,
    pub client_ts_us: u64,
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

impl InputPacket {
    pub fn now_disconnected(controller_id: u8, seq: u32) -> Self {
        Self {
            controller_id,
            flags: 0,
            seq,
            client_ts_us: unix_micros(),
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
            thumb_lx: 0,
            thumb_ly: 0,
            thumb_rx: 0,
            thumb_ry: 0,
        }
    }

    pub fn stamp_now(&mut self) {
        self.client_ts_us = unix_micros();
    }

    pub fn is_connected(&self) -> bool {
        self.flags & FLAG_CONNECTED != 0
    }

    pub fn encode(&self) -> [u8; INPUT_PACKET_SIZE] {
        let mut buf = [0u8; INPUT_PACKET_SIZE];
        buf[0..4].copy_from_slice(&INPUT_PACKET_MAGIC.to_le_bytes());
        buf[4] = INPUT_PACKET_VERSION;
        buf[5] = self.controller_id;
        buf[6] = self.flags;
        buf[7] = 0;
        buf[8..12].copy_from_slice(&self.seq.to_le_bytes());
        buf[12..20].copy_from_slice(&self.client_ts_us.to_le_bytes());
        buf[20..22].copy_from_slice(&self.buttons.to_le_bytes());
        buf[22] = self.left_trigger;
        buf[23] = self.right_trigger;
        buf[24..26].copy_from_slice(&self.thumb_lx.to_le_bytes());
        buf[26..28].copy_from_slice(&self.thumb_ly.to_le_bytes());
        buf[28..30].copy_from_slice(&self.thumb_rx.to_le_bytes());
        buf[30..32].copy_from_slice(&self.thumb_ry.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < INPUT_PACKET_SIZE {
            return Err(DecodeError::TooShort);
        }
        let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if magic != INPUT_PACKET_MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let version = buf[4];
        if version != INPUT_PACKET_VERSION {
            return Err(DecodeError::BadVersion(version));
        }
        Ok(Self {
            controller_id: buf[5],
            flags: buf[6],
            seq: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
            client_ts_us: u64::from_le_bytes(buf[12..20].try_into().unwrap()),
            buttons: u16::from_le_bytes(buf[20..22].try_into().unwrap()),
            left_trigger: buf[22],
            right_trigger: buf[23],
            thumb_lx: i16::from_le_bytes(buf[24..26].try_into().unwrap()),
            thumb_ly: i16::from_le_bytes(buf[26..28].try_into().unwrap()),
            thumb_rx: i16::from_le_bytes(buf[28..30].try_into().unwrap()),
            thumb_ry: i16::from_le_bytes(buf[30..32].try_into().unwrap()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    TooShort,
    BadMagic,
    BadVersion(u8),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::TooShort => write!(f, "input packet too short"),
            DecodeError::BadMagic => write!(f, "bad input packet magic"),
            DecodeError::BadVersion(v) => write!(f, "unsupported input packet version {v}"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = InputPacket {
            controller_id: 0,
            flags: FLAG_CONNECTED,
            seq: 42,
            client_ts_us: 1_700_000_000_000_123,
            buttons: 0x1000, // A
            left_trigger: 10,
            right_trigger: 200,
            thumb_lx: -1000,
            thumb_ly: 2000,
            thumb_rx: 0,
            thumb_ry: -50,
        };
        let bytes = p.encode();
        assert_eq!(bytes.len(), INPUT_PACKET_SIZE);
        let q = InputPacket::decode(&bytes).unwrap();
        assert_eq!(p, q);
    }
}
