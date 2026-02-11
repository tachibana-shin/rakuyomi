use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;
use zune_png::PngDecoder;

use anyhow::{Context, Result};

use crate::source::wasm_store::ImageData;

pub fn decode_image_fast(data: &[u8]) -> Option<Result<ImageData>> {
    // Detect format
    let is_png = data.starts_with(b"\x89PNG\r\n\x1a\n");
    let is_jpeg = data.starts_with(b"\xFF\xD8\xFF");

    if is_png {
        return Some(decode_png(data));
    } else if is_jpeg {
        return Some(decode_jpeg(data));
    }

    None
}

fn decode_png(data: &[u8]) -> Result<ImageData> {
    let options = DecoderOptions::default()
        .png_set_strip_to_8bit(true)
        .png_set_add_alpha_channel(true);

    let mut decoder = PngDecoder::new_with_options(ZCursor::new(data), options);

    // raw decode RGBA8
    let raw = decoder.decode_raw()?;

    let info = decoder.info().context("invalid info png")?;

    let width = info.width as i32;
    let height = info.height as i32;

    // RGBA8 -> ARGB u32
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for chunk in raw.chunks_exact(4) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        let a = chunk[3] as u32;
        pixels.push((a << 24) | (r << 16) | (g << 8) | b);
    }

    Ok(ImageData {
        width,
        height,
        data: pixels,
    })
}

fn decode_jpeg(data: &[u8]) -> Result<ImageData> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);

    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);

    let raw = decoder.decode()?;
    let info = decoder.info().context("invalid info png")?;

    let width = info.width as i32;
    let height = info.height as i32;

    let mut pixels = Vec::with_capacity((width * height) as usize);
    for chunk in raw.chunks_exact(4) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        let a = chunk[3] as u32;
        pixels.push((a << 24) | (r << 16) | (g << 8) | b);
    }

    Ok(ImageData {
        width: info.width as i32,
        height: info.height as i32,
        data: pixels,
    })
}
