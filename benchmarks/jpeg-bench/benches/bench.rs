use criterion::{black_box, criterion_group, criterion_main, Criterion};

const W: usize = 1600;
const H: usize = 2400;

static COLOR_JPEG: &[u8] = include_bytes!("../../data/color_test.jpg");
static GRAY_JPEG: &[u8] = include_bytes!("../../data/gray_test.jpg");

fn make_rgb_pixels(w: usize, h: usize) -> Vec<u8> {
    let mut rgb = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            rgb[i] = ((x as f32 / w as f32) * 255.0) as u8;
            rgb[i + 1] = ((y as f32 / h as f32) * 255.0) as u8;
            rgb[i + 2] =
                (128.0 + 64.0 * ((x as f32 / w as f32) * std::f32::consts::PI * 4.0).sin()
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
            gray[y * w + x] =
                ((x as f32 / w as f32) * 128.0 + (y as f32 / h as f32) * 128.0) as u8;
        }
    }
    gray
}

struct TestData {
    color_jpeg: &'static [u8],
    gray_jpeg: &'static [u8],
    rgb_pixels: Vec<u8>,
    gray_pixels: Vec<u8>,
}

fn setup() -> TestData {
    TestData {
        color_jpeg: COLOR_JPEG,
        gray_jpeg: GRAY_JPEG,
        rgb_pixels: make_rgb_pixels(W, H),
        gray_pixels: make_gray_pixels(W, H),
    }
}

// ── Decode wrappers (RGB) ──────────────────────────────────────────

fn decode_rgb_zune(data: &[u8]) -> (Vec<u8>, usize, usize) {
    use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    use zune_jpeg::JpegDecoder;
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut dec = JpegDecoder::new_with_options(ZCursor::new(data), opts);
    let raw = dec.decode().unwrap();
    let info = dec.info().unwrap();
    (raw, info.width as usize, info.height as usize)
}

fn decode_rgb_turbojpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let img = turbojpeg::decompress(data, turbojpeg::PixelFormat::RGB).unwrap();
    (img.pixels, img.width as usize, img.height as usize)
}

fn decode_rgb_libjpegturbo(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let img = libjpeg_turbo_rs::decompress_to(data, libjpeg_turbo_rs::PixelFormat::Rgb).unwrap();
    (img.data, img.width, img.height)
}

// ── Decode wrappers (Grayscale) ────────────────────────────────────

fn decode_gray_zune(data: &[u8]) -> (Vec<u8>, usize, usize) {
    use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    use zune_jpeg::JpegDecoder;
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma);
    let mut dec = JpegDecoder::new_with_options(ZCursor::new(data), opts);
    let raw = dec.decode().unwrap();
    let info = dec.info().unwrap();
    (raw, info.width as usize, info.height as usize)
}

fn decode_gray_turbojpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let img = turbojpeg::decompress(data, turbojpeg::PixelFormat::GRAY).unwrap();
    (img.pixels, img.width as usize, img.height as usize)
}

fn decode_gray_libjpegturbo(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let img =
        libjpeg_turbo_rs::decompress_to(data, libjpeg_turbo_rs::PixelFormat::Grayscale).unwrap();
    (img.data, img.width, img.height)
}

// ── 1/8 subsampled ─────────────────────────────────────────────────

fn decode_1_8_turbojpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let mut decomp = turbojpeg::Decompressor::new().unwrap();
    decomp.set_scaling_factor(turbojpeg::ScalingFactor::new(1, 8)).unwrap();
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

fn decode_1_8_libjpegturbo(data: &[u8]) -> (Vec<u8>, usize, usize) {
    use libjpeg_turbo_rs::{Decoder, PixelFormat, ScalingFactor};
    let dec = Decoder::new(data)
        .unwrap()
        .with_output_format(PixelFormat::Rgb)
        .with_scale(ScalingFactor::new(1, 8));
    let img = dec.decode_image().unwrap();
    (img.data, img.width, img.height)
}

// ── Encode wrappers ────────────────────────────────────────────────

fn encode_rgb_turbojpeg(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
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

fn encode_rgb_libjpegturbo(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
    libjpeg_turbo_rs::compress(
        pixels,
        w,
        h,
        libjpeg_turbo_rs::PixelFormat::Rgb,
        80,
        libjpeg_turbo_rs::Subsampling::S420,
    )
    .unwrap()
}

fn encode_gray_turbojpeg(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
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

fn encode_gray_libjpegturbo(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
    libjpeg_turbo_rs::compress(
        pixels,
        w,
        h,
        libjpeg_turbo_rs::PixelFormat::Grayscale,
        80,
        libjpeg_turbo_rs::Subsampling::S420,
    )
    .unwrap()
}

// ── Criterion benchmarks ───────────────────────────────────────────

fn bench_decode_rgb(c: &mut Criterion) {
    let td = setup();
    let mut g = c.benchmark_group("decode_rgb");
    g.sample_size(30);
    g.bench_function("zune-jpeg",      |b| b.iter(|| black_box(decode_rgb_zune(black_box(td.color_jpeg)))));
    g.bench_function("turbojpeg",      |b| b.iter(|| black_box(decode_rgb_turbojpeg(black_box(td.color_jpeg)))));
    g.bench_function("libjpeg-turbo-rs", |b| b.iter(|| black_box(decode_rgb_libjpegturbo(black_box(td.color_jpeg)))));
    g.finish();
}

fn bench_decode_gray(c: &mut Criterion) {
    let td = setup();
    let mut g = c.benchmark_group("decode_gray");
    g.sample_size(30);
    g.bench_function("zune-jpeg",      |b| b.iter(|| black_box(decode_gray_zune(black_box(td.gray_jpeg)))));
    g.bench_function("turbojpeg",      |b| b.iter(|| black_box(decode_gray_turbojpeg(black_box(td.gray_jpeg)))));
    g.bench_function("libjpeg-turbo-rs", |b| b.iter(|| black_box(decode_gray_libjpegturbo(black_box(td.gray_jpeg)))));
    g.finish();
}

fn bench_decode_subsampled(c: &mut Criterion) {
    let td = setup();
    let mut g = c.benchmark_group("decode_subsampled_1_8");
    g.sample_size(30);
    g.bench_function("turbojpeg",      |b| b.iter(|| black_box(decode_1_8_turbojpeg(black_box(td.color_jpeg)))));
    g.bench_function("libjpeg-turbo-rs", |b| b.iter(|| black_box(decode_1_8_libjpegturbo(black_box(td.color_jpeg)))));
    g.finish();
}

fn bench_encode_rgb(c: &mut Criterion) {
    let td = setup();
    let mut g = c.benchmark_group("encode_rgb");
    g.sample_size(30);
    g.bench_function("turbojpeg",      |b| b.iter(|| black_box(encode_rgb_turbojpeg(&td.rgb_pixels, W, H))));
    g.bench_function("libjpeg-turbo-rs", |b| b.iter(|| black_box(encode_rgb_libjpegturbo(&td.rgb_pixels, W, H))));
    g.finish();
}

fn bench_encode_gray(c: &mut Criterion) {
    let td = setup();
    let mut g = c.benchmark_group("encode_gray");
    g.sample_size(30);
    g.bench_function("turbojpeg",      |b| b.iter(|| black_box(encode_gray_turbojpeg(&td.gray_pixels, W, H))));
    g.bench_function("libjpeg-turbo-rs", |b| b.iter(|| black_box(encode_gray_libjpegturbo(&td.gray_pixels, W, H))));
    g.finish();
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
