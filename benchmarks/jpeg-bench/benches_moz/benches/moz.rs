use criterion::{black_box, criterion_group, criterion_main, Criterion};

const W: usize = 1600;
const H: usize = 2400;

static COLOR_JPEG: &[u8] = include_bytes!("../../../data/color_test.jpg");

fn decode_rgb_mozjpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let decomp = mozjpeg::Decompress::new_mem(data).unwrap();
    let w = decomp.width() as usize;
    let h = decomp.height() as usize;
    let mut started = decomp.rgb().unwrap();
    let pixels = started.read_scanlines().unwrap();
    (pixels, w, h)
}

fn decode_gray_mozjpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let decomp = mozjpeg::Decompress::new_mem(data).unwrap();
    let w = decomp.width() as usize;
    let h = decomp.height() as usize;
    let mut started = decomp.grayscale().unwrap();
    let pixels = started.read_scanlines().unwrap();
    (pixels, w, h)
}

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

fn encode_rgb_mozjpeg(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    comp.set_size(w, h);
    comp.set_quality(80.0);
    comp.set_fastest_defaults();
    let out = Vec::with_capacity(w * h);
    let mut comp = comp.start_compress(out).unwrap();
    comp.write_scanlines(pixels).unwrap();
    comp.finish().unwrap()
}

fn encode_gray_mozjpeg(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_GRAYSCALE);
    comp.set_size(w, h);
    comp.set_quality(80.0);
    comp.set_fastest_defaults();
    let out = Vec::with_capacity(w * h);
    let mut comp = comp.start_compress(out).unwrap();
    comp.write_scanlines(pixels).unwrap();
    comp.finish().unwrap()
}

fn decode_1_8_mozjpeg(data: &[u8]) -> (Vec<u8>, usize, usize) {
    let mut decomp = mozjpeg::Decompress::new_mem(data).unwrap();
    decomp.scale(8);
    let mut started = decomp.rgb().unwrap();
    let w = started.width() as usize;
    let h = started.height() as usize;
    let pixels = started.read_scanlines().unwrap();
    (pixels, w, h)
}

fn bench_decode_rgb(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_rgb");
    g.sample_size(30);
    g.bench_function("mozjpeg", |b| b.iter(|| black_box(decode_rgb_mozjpeg(black_box(COLOR_JPEG)))));
    g.finish();
}

fn bench_decode_gray(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_gray");
    g.sample_size(30);
    g.bench_function("mozjpeg", |b| b.iter(|| black_box(decode_gray_mozjpeg(black_box(COLOR_JPEG)))));
    g.finish();
}

fn bench_decode_subsampled(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_subsampled_1_8");
    g.sample_size(30);
    g.bench_function("mozjpeg", |b| b.iter(|| black_box(decode_1_8_mozjpeg(black_box(COLOR_JPEG)))));
    g.finish();
}

fn bench_encode_rgb(c: &mut Criterion) {
    let pixels = make_rgb_pixels(W, H);
    let mut g = c.benchmark_group("encode_rgb");
    g.sample_size(30);
    g.bench_function("mozjpeg", |b| b.iter(|| black_box(encode_rgb_mozjpeg(&pixels, W, H))));
    g.finish();
}

fn bench_encode_gray(c: &mut Criterion) {
    let pixels = make_gray_pixels(W, H);
    let mut g = c.benchmark_group("encode_gray");
    g.sample_size(30);
    g.bench_function("mozjpeg", |b| b.iter(|| black_box(encode_gray_mozjpeg(&pixels, W, H))));
    g.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_decode_rgb, bench_decode_gray, bench_decode_subsampled,
        bench_encode_rgb, bench_encode_gray,
}
criterion_main!(benches);
