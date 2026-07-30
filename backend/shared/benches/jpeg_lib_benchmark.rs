use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn make_rgb_pixels(w: usize, h: usize) -> Vec<u8> {
    let mut rgb = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            rgb[i] = ((x as f32 / w as f32) * 255.0) as u8;
            rgb[i + 1] = ((y as f32 / h as f32) * 255.0) as u8;
            rgb[i + 2] = (128.0
                + 64.0 * ((x as f32 / w as f32) * std::f32::consts::PI * 4.0).sin()
                + 64.0 * ((y as f32 / h as f32) * std::f32::consts::PI * 4.0).cos())
                as u8;
        }
    }
    rgb
}

fn make_gray_pixels(w: usize, h: usize) -> Vec<u8> {
    let mut gray = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            gray[y * w + x] = ((x as f32 / w as f32) * 128.0 + (y as f32 / h as f32) * 128.0) as u8;
        }
    }
    gray
}

fn encode_color_jpeg(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let img = turbojpeg::Image {
        pixels: rgb,
        width: w,
        pitch: w * 3,
        height: h,
        format: turbojpeg::PixelFormat::RGB,
    };
    turbojpeg::compress(img, 80, turbojpeg::Subsamp::Sub2x2)
        .unwrap()
        .to_vec()
}

fn encode_gray_jpeg(gray: &[u8], w: usize, h: usize) -> Vec<u8> {
    let img = turbojpeg::Image {
        pixels: gray,
        width: w,
        pitch: w,
        height: h,
        format: turbojpeg::PixelFormat::GRAY,
    };
    turbojpeg::compress(img, 80, turbojpeg::Subsamp::Gray)
        .unwrap()
        .to_vec()
}

const W: usize = 1600;
const H: usize = 2400;

struct TestData {
    color_jpeg: Vec<u8>,
    gray_jpeg: Vec<u8>,
    rgb_pixels: Vec<u8>,
    gray_pixels: Vec<u8>,
}

fn setup() -> TestData {
    let rgb = make_rgb_pixels(W, H);
    let gray = make_gray_pixels(W, H);
    let color_jpeg = encode_color_jpeg(&rgb, W, H);
    let gray_jpeg = encode_gray_jpeg(&gray, W, H);
    TestData {
        color_jpeg,
        gray_jpeg,
        rgb_pixels: rgb,
        gray_pixels: gray,
    }
}

mod decode_rgb {
    use super::*;

    pub fn zune(data: &[u8]) -> (Vec<u8>, usize, usize) {
        use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
        use zune_jpeg::JpegDecoder;
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
        let raw = decoder.decode().unwrap();
        let info = decoder.info().unwrap();
        (raw, info.width as usize, info.height as usize)
    }

    pub fn turbojpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
        let img = turbojpeg::decompress(data, turbojpeg::PixelFormat::RGB).unwrap();
        (img.pixels, img.width as usize, img.height as usize)
    }

    pub fn libjpegturbo(data: &[u8]) -> (Vec<u8>, usize, usize) {
        use libjpeg_turbo_rs::{decompress_to, PixelFormat};
        let img = decompress_to(data, PixelFormat::Rgb).unwrap();
        (img.data, img.width, img.height)
    }
}

mod decode_gray {
    use super::*;

    pub fn zune(data: &[u8]) -> (Vec<u8>, usize, usize) {
        use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
        use zune_jpeg::JpegDecoder;
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Grayscale);
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
        let raw = decoder.decode().unwrap();
        let info = decoder.info().unwrap();
        (raw, info.width as usize, info.height as usize)
    }

    pub fn turbojpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
        let img = turbojpeg::decompress(data, turbojpeg::PixelFormat::GRAY).unwrap();
        (img.pixels, img.width as usize, img.height as usize)
    }

    pub fn libjpegturbo(data: &[u8]) -> (Vec<u8>, usize, usize) {
        use libjpeg_turbo_rs::{decompress_to, PixelFormat};
        let img = decompress_to(data, PixelFormat::Grayscale).unwrap();
        (img.data, img.width, img.height)
    }
}

mod decode_subsampled {
    use super::*;

    pub fn turbojpeg_1_8(data: &[u8]) -> (Vec<u8>, usize, usize) {
        let mut decomp = turbojpeg::Decompressor::new().unwrap();
        decomp
            .set_scaling_factor(turbojpeg::ScalingFactor::new(1, 8))
            .unwrap();
        let hdr = decomp.read_header(data).unwrap();
        let scaled = hdr.scaled(turbojpeg::ScalingFactor::new(1, 8));
        let buf_len = scaled.width as usize * scaled.height as usize * 3;
        let mut pixels = vec![0u8; buf_len];
        let img = turbojpeg::Image {
            pixels: pixels.as_mut_slice(),
            width: scaled.width,
            pitch: scaled.width * 3,
            height: scaled.height,
            format: turbojpeg::PixelFormat::RGB,
        };
        decomp.decompress(data, img).unwrap();
        (pixels, scaled.width as usize, scaled.height as usize)
    }

    pub fn libjpegturbo_1_8(data: &[u8]) -> (Vec<u8>, usize, usize) {
        use libjpeg_turbo_rs::{Decoder, PixelFormat, ScalingFactor};
        let decoder = Decoder::new(data)
            .unwrap()
            .with_output_format(PixelFormat::Rgb)
            .with_scale(ScalingFactor::new(1, 8));
        let img = decoder.decode_image().unwrap();
        (img.data, img.width, img.height)
    }
}

mod encode_rgb {
    use super::*;

    pub fn turbojpeg(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
        let img = turbojpeg::Image {
            pixels,
            width: w,
            pitch: w * 3,
            height: h,
            format: turbojpeg::PixelFormat::RGB,
        };
        turbojpeg::compress(img, 80, turbojpeg::Subsamp::Sub2x2)
            .unwrap()
            .to_vec()
    }

    pub fn libjpegturbo(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
        use libjpeg_turbo_rs::{compress, PixelFormat, Subsampling};
        compress(pixels, w, h, PixelFormat::Rgb, 80, Subsampling::S420).unwrap()
    }
}

mod encode_gray {
    use super::*;

    pub fn turbojpeg(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
        let img = turbojpeg::Image {
            pixels,
            width: w,
            pitch: w,
            height: h,
            format: turbojpeg::PixelFormat::GRAY,
        };
        turbojpeg::compress(img, 80, turbojpeg::Subsamp::Gray)
            .unwrap()
            .to_vec()
    }

    pub fn libjpegturbo(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
        use libjpeg_turbo_rs::{compress, PixelFormat, Subsampling};
        compress(pixels, w, h, PixelFormat::Grayscale, 80, Subsampling::S420).unwrap()
    }
}

pub fn bench_decode_rgb(c: &mut Criterion) {
    let td = setup();
    let mut group = c.benchmark_group("decode_rgb");
    group.sample_size(30);
    group.bench_function("zune-jpeg", |b| {
        b.iter(|| black_box(decode_rgb::zune(black_box(&td.color_jpeg))))
    });
    group.bench_function("turbojpeg", |b| {
        b.iter(|| black_box(decode_rgb::turbojpeg(black_box(&td.color_jpeg))))
    });
    group.bench_function("libjpeg-turbo-rs", |b| {
        b.iter(|| black_box(decode_rgb::libjpegturbo(black_box(&td.color_jpeg))))
    });
    group.finish();
}

pub fn bench_decode_gray(c: &mut Criterion) {
    let td = setup();
    let mut group = c.benchmark_group("decode_gray");
    group.sample_size(30);
    group.bench_function("zune-jpeg", |b| {
        b.iter(|| black_box(decode_gray::zune(black_box(&td.gray_jpeg))))
    });
    group.bench_function("turbojpeg", |b| {
        b.iter(|| black_box(decode_gray::turbojpeg(black_box(&td.gray_jpeg))))
    });
    group.bench_function("libjpeg-turbo-rs", |b| {
        b.iter(|| black_box(decode_gray::libjpegturbo(black_box(&td.gray_jpeg))))
    });
    group.finish();
}

pub fn bench_decode_subsampled(c: &mut Criterion) {
    let td = setup();
    let mut group = c.benchmark_group("decode_subsampled_1_8");
    group.sample_size(30);
    group.bench_function("turbojpeg", |b| {
        b.iter(|| black_box(decode_subsampled::turbojpeg_1_8(black_box(&td.color_jpeg))))
    });
    group.bench_function("libjpeg-turbo-rs", |b| {
        b.iter(|| {
            black_box(decode_subsampled::libjpegturbo_1_8(black_box(
                &td.color_jpeg,
            )))
        })
    });
    group.finish();
}

pub fn bench_encode_rgb(c: &mut Criterion) {
    let td = setup();
    let mut group = c.benchmark_group("encode_rgb");
    group.sample_size(30);
    group.bench_function("turbojpeg", |b| {
        b.iter(|| black_box(encode_rgb::turbojpeg(&td.rgb_pixels, W, H)))
    });
    group.bench_function("libjpeg-turbo-rs", |b| {
        b.iter(|| black_box(encode_rgb::libjpegturbo(&td.rgb_pixels, W, H)))
    });
    group.finish();
}

pub fn bench_encode_gray(c: &mut Criterion) {
    let td = setup();
    let mut group = c.benchmark_group("encode_gray");
    group.sample_size(30);
    group.bench_function("turbojpeg", |b| {
        b.iter(|| black_box(encode_gray::turbojpeg(&td.gray_pixels, W, H)))
    });
    group.bench_function("libjpeg-turbo-rs", |b| {
        b.iter(|| black_box(encode_gray::libjpegturbo(&td.gray_pixels, W, H)))
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets =
        bench_decode_rgb,
        bench_decode_gray,
        bench_decode_subsampled,
        bench_encode_rgb,
        bench_encode_gray,
}
criterion_main!(benches);
