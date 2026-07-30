use crate::source::wasm_store::ImageData;
use anyhow::Result;

pub fn decode_jxl_to_rgb(data: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let mut decoder = jpegxl_rs::decoder_builder()
        .build()
        .map_err(|e| anyhow::anyhow!("JXL decoder creation failed: {}", e))?;
    decoder.pixel_format = Some(jpegxl_rs::decode::PixelFormat {
        num_channels: 3,
        endianness: jpegxl_rs::Endianness::Native,
        align: 0,
    });
    let (meta, pixels) = decoder
        .decode_with::<u8>(data)
        .map_err(|e| anyhow::anyhow!("JXL decode failed: {}", e))?;
    Ok((pixels, meta.width as usize, meta.height as usize))
}

pub fn decode_jxl_to_argb(data: &[u8]) -> Result<ImageData> {
    let mut decoder = jpegxl_rs::decoder_builder()
        .build()
        .map_err(|e| anyhow::anyhow!("JXL decoder creation failed: {}", e))?;
    decoder.pixel_format = Some(jpegxl_rs::decode::PixelFormat {
        num_channels: 4,
        endianness: jpegxl_rs::Endianness::Native,
        align: 0,
    });
    let (meta, rgba) = decoder
        .decode_with::<u8>(data)
        .map_err(|e| anyhow::anyhow!("JXL decode failed: {}", e))?;
    let w = meta.width as i32;
    let h = meta.height as i32;
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
