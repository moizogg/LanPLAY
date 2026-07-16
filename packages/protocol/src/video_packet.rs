//! Fragmented H.264 video over UDP (Phase 6).
//!
//! One encoded access unit is split into fragments that fit under typical MTU.
//!
//! Layout (little-endian header, then payload):
//! ```text
//! magic u32 | ver u8 | flags u8 | _pad u16
//! frame_id u32 | frag_idx u16 | frag_count u16
//! width u16 | height u16 | pts_us u64
//! payload...
//! ```

/// ASCII "LPVD" — LANPlay Video (fragmented H.264).
pub const VIDEO_PACKET_MAGIC: u32 = 0x4C50_5644;
/// ASCII "LPVH" — client hello / keep-alive so host learns return path.
pub const VIDEO_HELLO_MAGIC: u32 = 0x4C50_5648;
pub const VIDEO_PACKET_VERSION: u8 = 1;
pub const VIDEO_HEADER_SIZE: usize = 28;
/// Keep UDP datagrams under ~1200 bytes on typical paths.
pub const VIDEO_MAX_PAYLOAD: usize = 1100;

pub const VIDEO_FLAG_KEYFRAME: u8 = 1 << 0;

/// 8-byte hello: magic + version + reserved.
pub fn encode_video_hello() -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0..4].copy_from_slice(&VIDEO_HELLO_MAGIC.to_le_bytes());
    b[4] = VIDEO_PACKET_VERSION;
    b
}

pub fn is_video_hello(buf: &[u8]) -> bool {
    if buf.len() < 4 {
        return false;
    }
    u32::from_le_bytes(buf[0..4].try_into().unwrap_or([0; 4])) == VIDEO_HELLO_MAGIC
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFragmentHeader {
    pub flags: u8,
    pub frame_id: u32,
    pub frag_idx: u16,
    pub frag_count: u16,
    pub width: u16,
    pub height: u16,
    pub pts_us: u64,
}

impl VideoFragmentHeader {
    pub fn is_keyframe(&self) -> bool {
        self.flags & VIDEO_FLAG_KEYFRAME != 0
    }

    pub fn encode_into(&self, buf: &mut [u8]) {
        debug_assert!(buf.len() >= VIDEO_HEADER_SIZE);
        buf[0..4].copy_from_slice(&VIDEO_PACKET_MAGIC.to_le_bytes());
        buf[4] = VIDEO_PACKET_VERSION;
        buf[5] = self.flags;
        buf[6] = 0;
        buf[7] = 0;
        buf[8..12].copy_from_slice(&self.frame_id.to_le_bytes());
        buf[12..14].copy_from_slice(&self.frag_idx.to_le_bytes());
        buf[14..16].copy_from_slice(&self.frag_count.to_le_bytes());
        buf[16..18].copy_from_slice(&self.width.to_le_bytes());
        buf[18..20].copy_from_slice(&self.height.to_le_bytes());
        buf[20..28].copy_from_slice(&self.pts_us.to_le_bytes());
    }

    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < VIDEO_HEADER_SIZE {
            return None;
        }
        let magic = u32::from_le_bytes(buf[0..4].try_into().ok()?);
        if magic != VIDEO_PACKET_MAGIC || buf[4] != VIDEO_PACKET_VERSION {
            return None;
        }
        Some(Self {
            flags: buf[5],
            frame_id: u32::from_le_bytes(buf[8..12].try_into().ok()?),
            frag_idx: u16::from_le_bytes(buf[12..14].try_into().ok()?),
            frag_count: u16::from_le_bytes(buf[14..16].try_into().ok()?),
            width: u16::from_le_bytes(buf[16..18].try_into().ok()?),
            height: u16::from_le_bytes(buf[18..20].try_into().ok()?),
            pts_us: u64::from_le_bytes(buf[20..28].try_into().ok()?),
        })
    }
}

/// Build UDP datagrams for one Annex-B access unit.
pub fn fragment_access_unit(
    frame_id: u32,
    width: u32,
    height: u32,
    pts_us: u64,
    keyframe: bool,
    data: &[u8],
) -> Vec<Vec<u8>> {
    if data.is_empty() {
        return Vec::new();
    }
    let chunk = VIDEO_MAX_PAYLOAD;
    let frag_count = ((data.len() + chunk - 1) / chunk) as u16;
    let frag_count = frag_count.max(1);
    let flags = if keyframe { VIDEO_FLAG_KEYFRAME } else { 0 };
    let mut out = Vec::with_capacity(frag_count as usize);
    for (i, piece) in data.chunks(chunk).enumerate() {
        let header = VideoFragmentHeader {
            flags,
            frame_id,
            frag_idx: i as u16,
            frag_count,
            width: width.min(u16::MAX as u32) as u16,
            height: height.min(u16::MAX as u32) as u16,
            pts_us,
        };
        let mut pkt = vec![0u8; VIDEO_HEADER_SIZE + piece.len()];
        header.encode_into(&mut pkt);
        pkt[VIDEO_HEADER_SIZE..].copy_from_slice(piece);
        out.push(pkt);
    }
    out
}

/// Reassembler for one in-flight frame (simple — keeps a few frame ids).
#[derive(Debug, Default)]
pub struct FrameReassembler {
    current_id: Option<u32>,
    expected: u16,
    width: u16,
    height: u16,
    pts_us: u64,
    keyframe: bool,
    parts: Vec<Option<Vec<u8>>>,
    /// Drop incomplete frames older than this many new frame_ids.
    pub max_incomplete: u32,
}

impl FrameReassembler {
    pub fn new() -> Self {
        Self {
            max_incomplete: 4,
            ..Default::default()
        }
    }

    /// Push a datagram; returns a complete access unit when ready.
    pub fn push(&mut self, buf: &[u8]) -> Option<ReassembledFrame> {
        let header = VideoFragmentHeader::decode(buf)?;
        if header.frag_count == 0 || header.frag_idx >= header.frag_count {
            return None;
        }
        let payload = buf[VIDEO_HEADER_SIZE..].to_vec();

        if self.current_id != Some(header.frame_id) {
            // Start new frame (drop incomplete previous).
            self.current_id = Some(header.frame_id);
            self.expected = header.frag_count;
            self.width = header.width;
            self.height = header.height;
            self.pts_us = header.pts_us;
            self.keyframe = header.is_keyframe();
            self.parts = vec![None; header.frag_count as usize];
        } else if header.frag_count != self.expected {
            return None;
        }

        if let Some(slot) = self.parts.get_mut(header.frag_idx as usize) {
            *slot = Some(payload);
        }

        if self.parts.iter().all(|p| p.is_some()) {
            let mut data = Vec::new();
            for p in self.parts.drain(..) {
                if let Some(chunk) = p {
                    data.extend_from_slice(&chunk);
                }
            }
            let frame = ReassembledFrame {
                frame_id: header.frame_id,
                width: self.width as u32,
                height: self.height as u32,
                pts_us: self.pts_us,
                keyframe: self.keyframe,
                data,
            };
            self.current_id = None;
            self.parts.clear();
            Some(frame)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReassembledFrame {
    pub frame_id: u32,
    pub width: u32,
    pub height: u32,
    pub pts_us: u64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_and_reassemble() {
        let data: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let pkts = fragment_access_unit(7, 1280, 720, 99, true, &data);
        assert!(pkts.len() > 1);
        let mut ra = FrameReassembler::new();
        let mut got = None;
        for p in pkts {
            if let Some(f) = ra.push(&p) {
                got = Some(f);
            }
        }
        let f = got.expect("frame");
        assert_eq!(f.frame_id, 7);
        assert_eq!(f.width, 1280);
        assert!(f.keyframe);
        assert_eq!(f.data, data);
    }
}
