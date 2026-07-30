use anyhow::{Context, Result};
use png::{BitDepth, ColorType, Decoder, Transformations};

use crate::source::wasm_store::ImageData;

fn expand_bpp(color_type: ColorType, bit_depth: BitDepth) -> Result<usize> {
    match (color_type, bit_depth) {
        (ColorType::Rgba, BitDepth::Eight) => Ok(4),
        (ColorType::Rgb, BitDepth::Eight) => Ok(3),
        (ColorType::GrayscaleAlpha, BitDepth::Eight) => Ok(2),
        (ColorType::Grayscale, BitDepth::Eight) => Ok(1),
        _ => anyhow::bail!(
            "unsupported PNG color type {:?} bit depth {:?}",
            color_type,
            bit_depth,
        ),
    }
}

pub fn decode_png(data: &[u8]) -> Result<ImageData> {
    let mut decoder = Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().context("failed to read PNG info")?;
    let width = reader.info().width as i32;
    let height = reader.info().height as i32;
    let color_type = reader.info().color_type;
    let bit_depth = reader.info().bit_depth;
    let bpp = expand_bpp(color_type, bit_depth)?;

    let buf_size = (width as usize) * (height as usize) * bpp;
    let mut buf = vec![0; buf_size];
    reader
        .next_frame(&mut buf)
        .context("failed to decode PNG frame")?;

    let mut pixels = Vec::with_capacity((width * height) as usize);
    match color_type {
        ColorType::Rgba => {
            for c in buf.chunks_exact(4) {
                pixels.push(
                    (c[3] as u32) << 24 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32,
                );
            }
        }
        ColorType::Rgb => {
            for c in buf.chunks_exact(3) {
                pixels.push(255 << 24 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32);
            }
        }
        ColorType::GrayscaleAlpha => {
            for c in buf.chunks_exact(2) {
                let g = c[0] as u32;
                pixels.push((c[1] as u32) << 24 | g << 16 | g << 8 | g);
            }
        }
        ColorType::Grayscale => {
            for &g in &buf {
                let g = g as u32;
                pixels.push(255 << 24 | g << 16 | g << 8 | g);
            }
        }
        _ => unreachable!(),
    }

    Ok(ImageData {
        width,
        height,
        data: pixels,
    })
}

pub fn decode_png_to_rgb(data: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let mut decoder = Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().context("failed to read PNG info")?;
    let width = reader.info().width as usize;
    let height = reader.info().height as usize;
    let color_type = reader.info().color_type;
    let bit_depth = reader.info().bit_depth;
    let bpp = expand_bpp(color_type, bit_depth)?;

    let mut buf = vec![0; width * height * bpp];
    reader
        .next_frame(&mut buf)
        .context("failed to decode PNG frame")?;

    let rgb = match color_type {
        ColorType::Rgb => buf,
        ColorType::Rgba => {
            let mut rgb = Vec::with_capacity(width * height * 3);
            for chunk in buf.chunks_exact(4) {
                rgb.push(chunk[0]);
                rgb.push(chunk[1]);
                rgb.push(chunk[2]);
            }
            rgb
        }
        ColorType::Grayscale => {
            let mut rgb = Vec::with_capacity(width * height * 3);
            for &g in &buf {
                rgb.push(g);
                rgb.push(g);
                rgb.push(g);
            }
            rgb
        }
        ColorType::GrayscaleAlpha => {
            let mut rgb = Vec::with_capacity(width * height * 3);
            for chunk in buf.chunks_exact(2) {
                let g = chunk[0];
                rgb.push(g);
                rgb.push(g);
                rgb.push(g);
            }
            rgb
        }
        _ => unreachable!(),
    };

    Ok((rgb, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_png_rgb(w: u32, h: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut buf), w, h);
        encoder.set_color(ColorType::Rgb);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let data = vec![200u8; w as usize * h as usize * 3];
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        buf
    }

    #[test]
    fn test_decode_png_rgb() {
        let png_bytes = make_test_png_rgb(4, 4);
        let img = decode_png(&png_bytes).unwrap();
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 4);
        assert_eq!(img.data.len(), 16);
        assert_eq!(img.data[0], 0xFF_C8_C8_C8);
    }

    #[test]
    fn test_decode_png_to_rgb_roundtrip() {
        let png_bytes = make_test_png_rgb(8, 8);
        let (rgb, w, h) = decode_png_to_rgb(&png_bytes).unwrap();
        assert_eq!(w, 8);
        assert_eq!(h, 8);
        assert_eq!(rgb.len(), 8 * 8 * 3);
    }
}
