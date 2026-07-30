use anyhow::{Context, Result};
use turbojpeg::{self, compress, Decompressor, Image, PixelFormat, Subsamp};

use crate::source::wasm_store::ImageData;

pub fn decode_jpeg_to_rgb(data: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let img =
        turbojpeg::decompress(data, PixelFormat::RGB).context("failed to decode JPEG to RGB")?;
    Ok((img.pixels, img.width as usize, img.height as usize))
}

pub fn encode_jpeg(rgb: &[u8], width: usize, height: usize, quality: u8) -> Result<Vec<u8>> {
    let q = quality.clamp(50, 95) as i32;
    let img = Image {
        pixels: rgb,
        width,
        pitch: width * 3,
        height,
        format: PixelFormat::RGB,
    };
    compress(img, q, Subsamp::Sub2x2)
        .context("failed to compress RGB JPEG")
        .map(|b| b.to_vec())
}

pub fn decode_jpeg(data: &[u8]) -> Result<ImageData> {
    let mut decomp = Decompressor::new().context("failed to create JPEG decompressor")?;
    let hdr = decomp
        .read_header(data)
        .context("failed to read JPEG header")?;
    let width = hdr.width as i32;
    let height = hdr.height as i32;
    let mut rgb = vec![0u8; width as usize * height as usize * 3];
    let img = Image {
        pixels: rgb.as_mut_slice(),
        width: hdr.width,
        pitch: hdr.width * 3,
        height: hdr.height,
        format: PixelFormat::RGB,
    };
    decomp
        .decompress(data, img)
        .context("failed to decompress JPEG")?;

    let mut pixels = Vec::with_capacity((width * height) as usize);
    for chunk in rgb.chunks_exact(3) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        pixels.push((255 << 24) | (r << 16) | (g << 8) | b);
    }

    Ok(ImageData {
        width,
        height,
        data: pixels,
    })
}
