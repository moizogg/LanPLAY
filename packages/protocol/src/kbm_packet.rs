//! Keyboard + mouse packet (client → host).
//!
//! Layout (little-endian, 40 bytes):
//! ```text
//! magic u32 | ver u8 | flags u8 | key_count u8 | _pad u8
//! seq u32 | client_ts_us u64
//! mouse_dx i16 | mouse_dy i16 | wheel i16 | _pad2 i16
//! keys [u8; 8]   // up to 8 currently held virtual-key codes
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

/// ASCII "LPKB"
pub const KBM_PACKET_MAGIC: u32 = 0x4C50_4B42;
pub const KBM_PACKET_VERSION: u8 = 1;
pub const KBM_PACKET_SIZE: usize = 40;

pub const KBM_FLAG_LBUTTON: u8 = 1 << 0;
pub const KBM_FLAG_RBUTTON: u8 = 1 << 1;
pub const KBM_FLAG_MBUTTON: u8 = 1 << 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KbmPacket {
    pub flags: u8,
    pub seq: u32,
    pub client_ts_us: u64,
    pub mouse_dx: i16,
    pub mouse_dy: i16,
    pub wheel: i16,
    pub keys: [u8; 8],
    pub key_count: u8,
}

impl KbmPacket {
    pub fn stamp_now(&mut self) {
        self.client_ts_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
    }

    pub fn encode(&self) -> [u8; KBM_PACKET_SIZE] {
        let mut buf = [0u8; KBM_PACKET_SIZE];
        buf[0..4].copy_from_slice(&KBM_PACKET_MAGIC.to_le_bytes());
        buf[4] = KBM_PACKET_VERSION;
        buf[5] = self.flags;
        buf[6] = self.key_count.min(8);
        buf[7] = 0;
        buf[8..12].copy_from_slice(&self.seq.to_le_bytes());
        buf[12..20].copy_from_slice(&self.client_ts_us.to_le_bytes());
        buf[20..22].copy_from_slice(&self.mouse_dx.to_le_bytes());
        buf[22..24].copy_from_slice(&self.mouse_dy.to_le_bytes());
        buf[24..26].copy_from_slice(&self.wheel.to_le_bytes());
        buf[26..28].copy_from_slice(&0i16.to_le_bytes());
        let n = self.key_count.min(8) as usize;
        buf[28..28 + n].copy_from_slice(&self.keys[..n]);
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < KBM_PACKET_SIZE {
            return Err(DecodeError::TooShort);
        }
        let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if magic != KBM_PACKET_MAGIC {
            return Err(DecodeError::BadMagic);
        }
        if buf[4] != KBM_PACKET_VERSION {
            return Err(DecodeError::BadVersion(buf[4]));
        }
        let key_count = buf[6].min(8);
        let mut keys = [0u8; 8];
        keys.copy_from_slice(&buf[28..36]);
        Ok(Self {
            flags: buf[5],
            seq: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
            client_ts_us: u64::from_le_bytes(buf[12..20].try_into().unwrap()),
            mouse_dx: i16::from_le_bytes(buf[20..22].try_into().unwrap()),
            mouse_dy: i16::from_le_bytes(buf[22..24].try_into().unwrap()),
            wheel: i16::from_le_bytes(buf[24..26].try_into().unwrap()),
            keys,
            key_count,
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
            DecodeError::TooShort => write!(f, "kbm packet too short"),
            DecodeError::BadMagic => write!(f, "bad kbm packet magic"),
            DecodeError::BadVersion(v) => write!(f, "unsupported kbm packet version {v}"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut p = KbmPacket {
            flags: KBM_FLAG_LBUTTON,
            seq: 9,
            client_ts_us: 123,
            mouse_dx: -3,
            mouse_dy: 7,
            wheel: 1,
            keys: [0x41, 0x42, 0, 0, 0, 0, 0, 0],
            key_count: 2,
        };
        p.client_ts_us = 999;
        let bytes = p.encode();
        let q = KbmPacket::decode(&bytes).unwrap();
        assert_eq!(p.flags, q.flags);
        assert_eq!(p.mouse_dx, q.mouse_dx);
        assert_eq!(p.key_count, q.key_count);
        assert_eq!(p.keys[0], q.keys[0]);
    }
}
