use crate::source::wasm_store::ImageData;
use anyhow::Result;
use webpx::{decode_rgb, decode_rgba};

/// Decode a WebP image to RGB using webpx (libwebp C binding).
pub fn decode_webp_to_rgb(data: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let (rgb, width, height) =
        decode_rgb(data).map_err(|e| anyhow::anyhow!("WebP decode failed: {}", e))?;
    Ok((rgb, width as usize, height as usize))
}

/// Decode a WebP image to ARGB u32 pixels (for `ImageData`).
pub fn decode_webp_to_argb(data: &[u8]) -> Result<ImageData> {
    let (rgba, width, height) =
        decode_rgba(data).map_err(|e| anyhow::anyhow!("WebP decode failed: {}", e))?;
    let w = width as i32;
    let h = height as i32;
    let pixels: Vec<u32> = rgba
        .chunks_exact(4)
        .map(|c| (c[3] as u32) << 24 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
        .collect();
    Ok(ImageData {
        width: w,
        height: h,
        data: pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_WEBP: &[u8] = &[
        0x52, 0x49, 0x46, 0x46, 0x30, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38,
        0x20, 0x24, 0x00, 0x00, 0x00, 0x90, 0x01, 0x00, 0x9D, 0x01, 0x2A, 0x04, 0x00, 0x04, 0x00,
        0x02, 0x00, 0x34, 0x25, 0xA4, 0x00, 0x02, 0xE7, 0x59, 0xB6, 0x00, 0x00, 0xFE, 0xD3, 0xEB,
        0xFF, 0x17, 0xFF, 0x77, 0xF5, 0xFF, 0xD7, 0xC0, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn test_decode_webp_to_rgb() {
        let (rgb, w, h) = decode_webp_to_rgb(TEST_WEBP).unwrap();
        assert_eq!(w, 4);
        assert_eq!(h, 4);
        assert_eq!(rgb.len(), 4 * 4 * 3);
    }
}
