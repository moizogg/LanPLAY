//! H.264 decode (OpenH264) for client receive path.

use openh264::decoder::Decoder;
use openh264::OpenH264API;

pub struct VideoDecoder {
    decoder: Decoder,
}

impl VideoDecoder {
    pub fn new() -> Result<Self, String> {
        let api = OpenH264API::from_source();
        let decoder = Decoder::with_api_config(api, openh264::decoder::DecoderConfig::new())
            .map_err(|e| format!("OpenH264 decoder init: {e:?}"))?;
        Ok(Self { decoder })
    }

    /// Decode one Annex-B access unit → tight RGBA8 (or None if need more data).
    pub fn decode_to_rgba(&mut self, annex_b: &[u8]) -> Result<Option<(u32, u32, Vec<u8>)>, String> {
        let yuv = self
            .decoder
            .decode(annex_b)
            .map_err(|e| format!("decode: {e:?}"))?;
        let Some(yuv) = yuv else {
            return Ok(None);
        };
        let (w, h) = yuv.dimensions();
        let mut rgba = vec![0u8; w * h * 4];
        yuv.write_rgba8(&mut rgba);
        Ok(Some((w as u32, h as u32, rgba)))
    }
}
