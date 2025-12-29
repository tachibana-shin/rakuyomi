#![allow(clippy::too_many_arguments)]

use aidoku::canvas::{Angle, FontWeight, PathOp};
use anyhow::{bail, Result};
use font_kit::properties::{Properties, Weight};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use raqote::{DrawOptions, LineCap, LineJoin, Point, Source, Transform, Vector};
use wasm_shared::get_memory;
use wasmi::{Caller, Linker};

use crate::source::{
    next_reader::read_next,
    wasm_store::{Value, WasmStore},
};
use wasm_macros::{aidoku_wasm_function, register_wasm_function};

pub fn register_canvas_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    // Context
    register_wasm_function!(linker, "canvas", "new_context", new_context)?; // check
    register_wasm_function!(linker, "canvas", "set_transform", set_transform)?; // check

    // Drawing
    register_wasm_function!(linker, "canvas", "draw_image", draw_image)?; // check
    register_wasm_function!(linker, "canvas", "copy_image", copy_image)?; // check
    register_wasm_function!(linker, "canvas", "fill", fill)?; // check
    register_wasm_function!(linker, "canvas", "stroke", stroke)?; // check
    register_wasm_function!(linker, "canvas", "draw_text", draw_text)?; // check

    // Font
    register_wasm_function!(linker, "canvas", "new_font", new_font)?; // check
    register_wasm_function!(linker, "canvas", "system_font", system_font)?; // check
    register_wasm_function!(linker, "canvas", "load_font", load_font)?; // check

    // Image
    register_wasm_function!(linker, "canvas", "new_image", new_image)?; // check
    register_wasm_function!(linker, "canvas", "get_image", get_image)?; // check
    register_wasm_function!(linker, "canvas", "get_image_data", get_image_data)?; // check
    register_wasm_function!(linker, "canvas", "get_image_width", get_image_width)?; // check
    register_wasm_function!(linker, "canvas", "get_image_height", get_image_height)?; // check

    Ok(())
}

// ----------------- Implementation -----------------

enum ResultContext {
    Success,
    InvalidContext,
    InvalidImagePointer,
    #[allow(dead_code)]
    InvalidImage,
    // InvalidSrcRec,
    // InvalidResult,
    // InvalidBounds,
    InvalidPath,
    InvalidStyle,
    InvalidString,
    InvalidFont,
    #[allow(dead_code)]
    InvalidData,
    FontLoadFailed,
}
impl From<ResultContext> for i32 {
    fn from(result: ResultContext) -> i32 {
        match result {
            ResultContext::Success => 0,
            ResultContext::InvalidContext => -1,
            ResultContext::InvalidImagePointer => -2,
            ResultContext::InvalidImage => -3,
            // Result::InvalidSrcRec => -4,
            // Result::InvalidResult => -5,
            // Result::InvalidBounds => -6,
            ResultContext::InvalidPath => -7,
            ResultContext::InvalidStyle => -8,
            ResultContext::InvalidString => -9,
            ResultContext::InvalidFont => -10,
            ResultContext::InvalidData => -11,
            ResultContext::FontLoadFailed => -12,
        }
    }
}

#[aidoku_wasm_function]
fn new_context(mut caller: Caller<'_, WasmStore>, width: f32, height: f32) -> Result<i32> {
    let store = caller.data_mut();
    if width <= 0.0 || height <= 0.0 {
        bail!("Invalid bougus")
    }

    Ok(store.create_canvas(width, height))
}

#[aidoku_wasm_function]
fn set_transform(
    mut caller: Caller<'_, WasmStore>,
    ctx_id: i32,
    translate_x: f32,
    translate_y: f32,
    scale_x: f32,
    scale_y: f32,
    rotate: f32,
) -> Result<i32> {
    let store = caller.data_mut();
    let Some(canvas) = &mut store.get_mut_canvas(ctx_id) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    canvas.set_transform(
        &Transform::translation(translate_x, translate_y)
            .then_scale(scale_x, scale_y)
            .then_rotate(Angle { radians: rotate }),
    );

    store.set_canvas(ctx_id, canvas);
    Ok(ResultContext::Success.into())
}
#[aidoku_wasm_function]
fn draw_image(
    mut caller: Caller<'_, WasmStore>,
    ctx_id: i32,
    img_id: i32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Result<i32> {
    let store = caller.data_mut();
    let Some(canvas) = &mut store.get_mut_canvas(ctx_id) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    let image = {
        let Some(image) = store.get_image(img_id) else {
            return Ok(ResultContext::InvalidImagePointer.into());
        };
        image
    };

    let rq_img: raqote::Image<'_> = raqote::Image {
        width: image.width,
        height: image.height,
        data: &image.data.clone(),
    };

    {
        // NOTE: draw_image_with_size_at expects f32 and a Transform
        canvas.draw_image_with_size_at(width, height, x, y, &rq_img, &raqote::DrawOptions::new());

        store.set_canvas(ctx_id, canvas);
    }

    Ok(ResultContext::Success.into())
}

#[aidoku_wasm_function]
fn copy_image(
    mut caller: Caller<'_, WasmStore>,
    ctx_id: i32,
    img_id: i32,
    src_x: f32,
    src_y: f32,
    src_width: f32,
    src_height: f32,
    dst_x: f32,
    dst_y: f32,
    dst_width: f32,
    dst_height: f32,
) -> Result<i32> {
    let store = caller.data_mut();
    let Some(canvas) = &mut store.get_mut_canvas(ctx_id) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    let image = {
        let Some(image) = store.get_image(img_id) else {
            return Ok(ResultContext::InvalidImagePointer.into());
        };
        image
    };

    let rq_img: raqote::Image<'_> = raqote::Image {
        width: image.width,
        height: image.height,
        data: &image.data.clone(),
    };

    {
        let scale_x = dst_width / src_width;
        let scale_y = dst_height / src_height;

        let mut clip_path = raqote::PathBuilder::new();
        clip_path.rect(dst_x, dst_y, dst_width, dst_height);
        let clip_path = clip_path.finish();

        // Push clip
        canvas.push_clip(&clip_path);

        //      その後に拡大縮小して、最後に描画先位置へ移動。
        let transform = raqote::Transform::translation(-src_x, -src_y)
            .then_scale(scale_x, scale_y)
            .then_translate(Vector::new(dst_x, dst_y));

        canvas.set_transform(&transform);

        canvas.draw_image_at(0.0, 0.0, &rq_img, &raqote::DrawOptions::new());

        // Reset
        canvas.set_transform(&raqote::Transform::default());
        canvas.pop_clip();

        store.set_canvas(ctx_id, canvas);
    }

    Ok(ResultContext::Success.into())
}

fn path_to_raqote_path(path: aidoku::canvas::Path) -> raqote::Path {
    let mut result = raqote::PathBuilder::new();
    for op in path.ops.iter() {
        match op {
            PathOp::MoveTo(point) => result.move_to(point.x, point.y),
            PathOp::LineTo(point) => result.line_to(point.x, point.y),
            PathOp::QuadTo(to, control) => result.quad_to(control.x, control.y, to.x, to.y),
            PathOp::CubicTo(to, c1, c2) => result.cubic_to(c1.x, c1.y, c2.x, c2.y, to.x, to.y),
            PathOp::Arc(to, radius, start, sweep) => {
                result.arc(to.x, to.y, *radius, *start, *sweep)
            }
            PathOp::Close => result.close(),
        }
    }
    result.finish()
}
#[aidoku_wasm_function]
fn fill(
    mut caller: Caller<'_, WasmStore>,
    ctx_id: i32,
    path_ptr: i32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> Result<i32> {
    let memory = {
        let Some(memory) = get_memory(&mut caller) else {
            eprintln!("get memory failed");

            return Ok(-1);
        };
        memory
    };
    let path = {
        let Some(path) = read_next::<aidoku::canvas::Path>(&memory, &caller, path_ptr).ok() else {
            return Ok(ResultContext::InvalidPath.into());
        };
        path
    };

    let store = caller.data_mut();
    let Some(canvas) = &mut store.get_mut_canvas(ctx_id) else {
        return Ok(ResultContext::InvalidContext.into());
    };
    let final_path = path_to_raqote_path(path);
    canvas.fill(
        &final_path,
        &Source::Solid(raqote::SolidSource {
            r: r as u8,
            g: g as u8,
            b: b as u8,
            a: (a * 255.0) as u8,
        }),
        &DrawOptions::default(),
    );

    store.set_canvas(ctx_id, canvas);
    Ok(ResultContext::Success.into())
}
#[aidoku_wasm_function]
fn stroke(
    mut caller: Caller<'_, WasmStore>,
    ctx_id: i32,
    path_ptr: i32,
    style_ptr: i32,
) -> Result<i32> {
    let memory = {
        let Some(memory) = get_memory(&mut caller) else {
            eprintln!("get memory failed");

            return Ok(-1);
        };
        memory
    };
    let path = {
        let Some(path) = read_next::<aidoku::canvas::Path>(&memory, &caller, path_ptr).ok() else {
            return Ok(ResultContext::InvalidPath.into());
        };
        path
    };
    let style = {
        let Some(style) =
            read_next::<aidoku::canvas::StrokeStyle>(&memory, &caller, style_ptr).ok()
        else {
            return Ok(ResultContext::InvalidStyle.into());
        };
        style
    };

    let store = caller.data_mut();
    let Some(canvas) = &mut store.get_mut_canvas(ctx_id) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    let final_path = path_to_raqote_path(path);

    canvas.stroke(
        &final_path,
        &Source::Solid(raqote::SolidSource {
            r: style.color.red as u8,
            g: style.color.green as u8,
            b: style.color.blue as u8,
            a: (style.color.alpha * 255.0) as u8,
        }),
        &raqote::StrokeStyle {
            width: style.width,
            cap: match style.cap {
                aidoku::canvas::LineCap::Butt => LineCap::Butt,
                aidoku::canvas::LineCap::Round => LineCap::Round,
                aidoku::canvas::LineCap::Square => LineCap::Square,
            },
            join: match style.join {
                aidoku::canvas::LineJoin::Miter => LineJoin::Miter,
                aidoku::canvas::LineJoin::Round => LineJoin::Round,
                aidoku::canvas::LineJoin::Bevel => LineJoin::Bevel,
            },
            miter_limit: style.miter_limit,
            dash_array: style.dash_array,
            dash_offset: style.dash_offset,
        },
        &DrawOptions::default(),
    );

    store.set_canvas(ctx_id, canvas);
    Ok(ResultContext::Success.into())
}
#[aidoku_wasm_function]
fn draw_text(
    mut caller: Caller<'_, WasmStore>,
    ctx_id: i32,
    text: Option<String>,
    size: f32,
    x: f32,
    y: f32,
    font_id: i32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> Result<i32> {
    let Some(text) = text else {
        return Ok(ResultContext::InvalidString.into());
    };

    let store = caller.data_mut();
    let Some(canvas) = &mut store.get_mut_canvas(ctx_id) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    let Some(font) = store.get_font(font_id) else {
        return Ok(ResultContext::InvalidFont.into());
    };

    canvas.draw_text(
        &font,
        size,
        &text,
        Point::new(x, y),
        &Source::Solid(raqote::SolidSource {
            r: r as u8,
            g: g as u8,
            b: b as u8,
            a: (a * 255.0) as u8,
        }),
        &DrawOptions::default(),
    );

    store.set_canvas(ctx_id, canvas);
    Ok(ResultContext::Success.into())
}

// ----------------- Font -----------------
#[aidoku_wasm_function]
fn new_font(mut caller: Caller<'_, WasmStore>, name: Option<String>) -> Result<i32> {
    let store = caller.data_mut();
    let Some(name) = name else {
        return Ok(ResultContext::InvalidString.into());
    };

    Ok(store
        .create_font(name, None)
        .unwrap_or(ResultContext::InvalidFont.into()))
}
#[aidoku_wasm_function]
fn system_font(mut caller: Caller<'_, WasmStore>, size: i32) -> Result<i32> {
    let store = caller.data_mut();

    let weight = match aidoku::canvas::FontWeight::from(size as u8) {
        FontWeight::UltraLight => Weight::EXTRA_LIGHT,
        FontWeight::Thin => Weight::THIN,
        FontWeight::Light => Weight::LIGHT,
        FontWeight::Regular => Weight::NORMAL,
        FontWeight::Medium => Weight::MEDIUM,
        FontWeight::Semibold => Weight::SEMIBOLD,
        FontWeight::Bold => Weight::BOLD,
        FontWeight::Heavy => Weight::EXTRA_BOLD,
        FontWeight::Black => Weight::BLACK,
    };

    Ok(store
        .create_font(
            "sans-serif".to_string(),
            Some(Properties::new().weight(weight)),
        )
        .unwrap_or(ResultContext::InvalidFont.into()))
}
#[aidoku_wasm_function]
fn load_font(mut caller: Caller<'_, WasmStore>, url: Option<String>) -> Result<i32> {
    let Some(url) = url else {
        return Ok(ResultContext::InvalidPath.into());
    };

    let bytes = match reqwest::blocking::get(&url) {
        Ok(resp) => match resp.bytes() {
            Ok(b) => b.to_vec(),
            Err(_) => return Ok(ResultContext::FontLoadFailed.into()),
        },
        Err(_) => return Ok(ResultContext::FontLoadFailed.into()),
    };

    let store = caller.data_mut();
    Ok(store.set_font_online(&bytes))
}

// ----------------- Image -----------------
#[aidoku_wasm_function]
fn new_image(mut caller: Caller<'_, WasmStore>, bytes: Option<Vec<u8>>) -> Result<i32> {
    let store = caller.data_mut();

    let Some(bytes) = bytes else {
        return Ok(ResultContext::InvalidData.into());
    };

    Ok(store
        .create_image(&bytes)
        .unwrap_or(ResultContext::InvalidData.into()))
}
#[aidoku_wasm_function]
fn get_image(mut caller: Caller<'_, WasmStore>, ctx_id: i32) -> Result<i32> {
    let store = caller.data_mut();
    let Some(canvas) = &mut store.get_mut_canvas(ctx_id) else {
        return Ok(ResultContext::InvalidContext.into());
    };

    let data = canvas.get_data();
    let image = crate::source::wasm_store::ImageData {
        data: data.to_vec(),
        width: canvas.width(),
        height: canvas.height(),
    };

    Ok(store.set_image_data(image))
}
#[aidoku_wasm_function]
fn get_image_data(mut caller: Caller<'_, WasmStore>, img_id: i32) -> Result<i32> {
    let store = caller.data_mut();
    let Some(image) = store.get_image(img_id) else {
        return Ok(ResultContext::InvalidImagePointer.into());
    };

    let width = image.width as u32;
    let height = image.height as u32;

    // ARGB(u32) → RGBA(u8[4]) に変換する（PNG は alpha 対応）
    let mut rgba_pixels: Vec<u8> = Vec::with_capacity((width * height * 4) as usize);

    for px in &image.data {
        let a = ((px >> 24) & 0xFF) as u8;
        let r = ((px >> 16) & 0xFF) as u8;
        let g = ((px >> 8) & 0xFF) as u8;
        let b = (px & 0xFF) as u8;

        rgba_pixels.extend_from_slice(&[r, g, b, a]);
    }

    let mut png_data: Vec<u8> = Vec::<u8>::new();
    let encoder = PngEncoder::new(&mut png_data);

    encoder
        .write_image(&rgba_pixels, width, height, ColorType::Rgba8.into())
        .expect("PNG encode failed");

    Ok(store.store_std_value(Value::Vec(png_data).into(), None) as i32)
}
#[aidoku_wasm_function]
fn get_image_width(mut caller: Caller<'_, WasmStore>, img_id: i32) -> Result<i32> {
    let store = caller.data_mut();
    let Some(image) = store.get_image(img_id) else {
        return Ok(ResultContext::InvalidImagePointer.into());
    };

    Ok(image.width)
}

#[aidoku_wasm_function]
fn get_image_height(mut caller: Caller<'_, WasmStore>, img_id: i32) -> Result<i32> {
    let store = caller.data_mut();
    let Some(image) = store.get_image(img_id) else {
        return Ok(ResultContext::InvalidImagePointer.into());
    };

    Ok(image.height)
}
