use crate::source::wasm_store::ImageData;
use anyhow::Result;

pub(crate) mod jpeg;
pub(crate) mod jxl;
pub(crate) mod png;
pub(crate) mod webp;

pub fn is_jpeg(data: &[u8]) -> bool {
    data.starts_with(b"\xFF\xD8\xFF")
}

#[derive(Debug, PartialEq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Webp,
    Jxl,
}

/// Detect the image format from magic bytes and return `Some(format_tag)`.
fn detect_format(data: &[u8]) -> Option<ImageFormat> {
    if is_jpeg(data) {
        Some(ImageFormat::Jpeg)
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if data.len() >= 12 && &data[4..8] == b"JXL " || data.starts_with(b"\xFF\x0A") {
        Some(ImageFormat::Jxl)
    } else {
        None
    }
}

pub fn decode_image_fast(data: &[u8]) -> Option<Result<ImageData>> {
    let img_type = detect_format(data);

    match img_type {
        Some(ImageFormat::Png) => Some(png::decode_png(data)),
        Some(ImageFormat::Jpeg) => Some(jpeg::decode_jpeg(data)),
        Some(ImageFormat::Webp) => Some(webp::decode_webp_to_argb(data)),
        Some(ImageFormat::Jxl) => Some(jxl::decode_jxl_to_argb(data)),
        _ => None,
    }
}

/// Decode an image directly to RGB bytes. Avoids the ARGB round-trip of `decode_image_fast`.
pub fn decode_image_to_rgb(data: &[u8]) -> Option<Result<(Vec<u8>, usize, usize)>> {
    let img_type = detect_format(data);

    match img_type {
        Some(ImageFormat::Png) => Some(png::decode_png_to_rgb(data)),
        Some(ImageFormat::Jpeg) => Some(jpeg::decode_jpeg_to_rgb(data)),
        Some(ImageFormat::Webp) => Some(webp::decode_webp_to_rgb(data)),
        Some(ImageFormat::Jxl) => Some(jxl::decode_jxl_to_rgb(data)),
        _ => None,
    }
}

/// Convert ARGB `u32` pixel data to RGB bytes for JPEG encoding.
/// Validates dimensions with checked arithmetic and length matching.
pub fn decode_argb_to_rgb(width: i32, height: i32, data: &[u32]) -> Result<Vec<u8>> {
    let pixel_count = (width as u64)
        .checked_mul(height as u64)
        .and_then(|p| p.checked_mul(3))
        .ok_or_else(|| anyhow::anyhow!("image dimensions overflow: {}x{}", width, height))?;

    let rgb_len = usize::try_from(pixel_count)
        .map_err(|_| anyhow::anyhow!("RGB buffer too large for usize: {}", pixel_count))?;
    let mut rgb_pixels = vec![0u8; rgb_len];

    for (i, px) in data.iter().enumerate() {
        let base = i * 3;
        rgb_pixels[base] = ((px >> 16) & 0xFF) as u8;
        rgb_pixels[base + 1] = ((px >> 8) & 0xFF) as u8;
        rgb_pixels[base + 2] = (px & 0xFF) as u8;
    }

    Ok(rgb_pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_rgb(w: usize, h: usize) -> Vec<u8> {
        let mut rgb = vec![200u8; w * h * 3];
        let margin_x = w / 4;
        let margin_y = h / 4;
        for y in margin_y..h - margin_y {
            for x in margin_x..w - margin_x {
                let idx = (y * w + x) * 3;
                rgb[idx] = 50;
                rgb[idx + 1] = 100;
                rgb[idx + 2] = 150;
            }
        }
        rgb
    }

    #[test]
    fn test_is_jpeg() {
        assert!(is_jpeg(b"\xFF\xD8\xFF\xE0"));
        assert!(is_jpeg(b"\xFF\xD8\xFF\xDB"));
        assert!(is_jpeg(b"\xFF\xD8\xFF\xC0"));
        assert!(!is_jpeg(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_jpeg(b""));
        assert!(!is_jpeg(b"not jpeg"));
    }

    #[test]
    fn test_detect_format_jpeg() {
        assert_eq!(detect_format(b"\xFF\xD8\xFF\xE0"), Some(ImageFormat::Jpeg));
        assert_eq!(detect_format(b"\xFF\xD8\xFF\xDB"), Some(ImageFormat::Jpeg));
    }

    #[test]
    fn test_detect_format_png() {
        assert_eq!(detect_format(b"\x89PNG\r\n\x1a\n"), Some(ImageFormat::Png));
    }

    #[test]
    fn test_detect_format_webp() {
        let header = b"\x52\x49\x46\x46\x00\x00\x00\x00\x57\x45\x42\x50";
        assert_eq!(detect_format(header), Some(ImageFormat::Webp));
    }

    #[test]
    fn test_detect_format_jxl_bmff() {
        let header = b"\x00\x00\x00\x0C\x4A\x58\x4C\x20\x0D\x0A\x87\x0A";
        assert_eq!(detect_format(header), Some(ImageFormat::Jxl));
    }

    #[test]
    fn test_detect_format_jxl_codestream() {
        assert_eq!(detect_format(b"\xFF\x0A"), Some(ImageFormat::Jxl));
    }

    #[test]
    fn test_detect_format_unknown() {
        assert!(detect_format(b"random data").is_none());
        assert!(detect_format(b"").is_none());
    }

    #[test]
    fn test_decode_argb_to_rgb() {
        let argb: Vec<u32> = vec![0xFF_10_20_30, 0xFF_40_50_60, 0xFF_70_80_90];
        let rgb = decode_argb_to_rgb(3, 1, &argb).unwrap();
        assert_eq!(
            rgb,
            vec![0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90]
        );
    }

    #[test]
    fn test_decode_image_to_rgb_roundtrip() {
        let rgb = make_test_rgb(16, 16);
        let jpeg = super::jpeg::encode_jpeg(&rgb, 16, 16, 80).unwrap();
        let result = decode_image_to_rgb(&jpeg);
        assert!(result.is_some());
        let (decoded, w, h) = result.unwrap().unwrap();
        assert_eq!(w, 16);
        assert_eq!(h, 16);
        assert_eq!(decoded.len(), 16 * 16 * 3);
    }

    #[test]
    fn test_decode_image_fast_roundtrip() {
        let rgb = make_test_rgb(8, 8);
        let jpeg = super::jpeg::encode_jpeg(&rgb, 8, 8, 80).unwrap();
        let result = decode_image_fast(&jpeg);
        assert!(result.is_some());
        let img = result.unwrap().unwrap();
        assert_eq!(img.width, 8);
        assert_eq!(img.height, 8);
        assert_eq!(img.data.len(), 64);
    }

    #[test]
    fn test_decode_image_to_rgb_no_such_format() {
        assert!(decode_image_to_rgb(b"not an image").is_none());
        assert!(decode_image_fast(b"not an image").is_none());
    }
}
