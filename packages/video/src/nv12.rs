//! Fast-ish BGRA → NV12 conversion for hardware encoders.

/// Convert tightly packed BGRA8 to NV12 (Y plane then interleaved UV).
pub fn bgra_to_nv12(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
        return Err(format!("NV12 needs even size, got {width}x{height}"));
    }
    let need = w * h * 4;
    if bgra.len() < need {
        return Err(format!("BGRA too small: {} < {need}", bgra.len()));
    }

    let y_size = w * h;
    let uv_size = w * h / 2;
    let mut out = vec![0u8; y_size + uv_size];

    // Y plane
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let b = bgra[i] as i32;
            let g = bgra[i + 1] as i32;
            let r = bgra[i + 2] as i32;
            // BT.601 full-ish
            let yy = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            out[y * w + x] = yy.clamp(0, 255) as u8;
        }
    }

    // UV plane (interleaved), 2x2 subsample
    let uv_base = y_size;
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut b_sum = 0i32;
            let mut g_sum = 0i32;
            let mut r_sum = 0i32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = ((y + dy) * w + (x + dx)) * 4;
                    b_sum += bgra[i] as i32;
                    g_sum += bgra[i + 1] as i32;
                    r_sum += bgra[i + 2] as i32;
                }
            }
            let b = b_sum / 4;
            let g = g_sum / 4;
            let r = r_sum / 4;
            let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
            let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
            let ui = uv_base + (y / 2) * w + x;
            out[ui] = u.clamp(0, 255) as u8;
            out[ui + 1] = v.clamp(0, 255) as u8;
        }
    }

    Ok(out)
}
