use image::{DynamicImage, GenericImage, GenericImageView, ImageFormat};
use serde::Deserialize;
use std::io::Cursor;

fn load_image(image: Vec<u8>) -> DynamicImage {
    return image::load_from_memory(image.as_slice()).expect("Failed to load image");
}

#[derive(Debug, Deserialize, Clone)]
pub struct Block {
    #[serde(default = "default_minus_one")]
    pub sx: i32,
    #[serde(default = "default_minus_one")]
    pub sy: i32,
    #[serde(default)]
    pub dx: u32,
    #[serde(default)]
    pub dy: u32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

fn default_minus_one() -> i32 {
    -1
}
pub fn unscrable_image(image_data: Vec<u8>, blocks: Vec<Block>) -> Result<Vec<u8>, String> {
    let img = load_image(image_data);
    let (img_width, img_height) = img.dimensions();
    let mut dst = DynamicImage::new_rgba8(img_width, img_height);

    let mut current_x = 0i32;
    let mut current_y = 0i32;

    for mut block in blocks {
        if block.width == 0 {
            block.width = img_width;
        }
        if block.height == 0 {
            block.height = img_height;
        }

        if block.sx == -1 {
            block.sx = current_x;
        }
        if block.sy == -1 {
            block.sy = current_y;
        }

        let sx = block.sx as u32;
        let sy = block.sy as u32;
        let dx = block.dx;
        let dy = block.dy;
        let width = block.width;
        let height = block.height;

        // Check bounds

        // println!("w out = {} + {} vs {}", sx, width, img_width);
        // println!("h out = {} + {} vs {}", sy, height, img_height);

        // if sx + width > img_width || sy + height > img_height {
        //     return Err(format!("Source block out of bounds"));
        // }
        // if dx + width > img_width || dy + height > img_height {
        //     return Err(format!("Destination block out of bounds"));
        // }

        let cropped = img.crop_imm(sx, sy, width, height);
        dst.copy_from(&cropped, dx, dy)
            .map_err(|e| format!("copy_from error: {:?}", e))?;

        current_x += width as i32;
        current_y += height as i32;
    }

    let mut out = Vec::new();
    dst.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| e.to_string())?;

    Ok(out)
}
