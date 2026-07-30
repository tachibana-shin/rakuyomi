use criterion::{black_box, criterion_group, criterion_main, Criterion};

static COLOR_LOSSY: &[u8] = include_bytes!("../../data/color_test.webp");
static GRAY_LOSSY: &[u8] = include_bytes!("../../data/gray_test.webp");
static COLOR_LOSSLESS: &[u8] = include_bytes!("../../data/color_lossless.webp");
static GRAY_LOSSLESS: &[u8] = include_bytes!("../../data/gray_lossless.webp");

// ── zenwebp ──────────────────────────────────────────────────────────

fn decode_rgb_zenwebp(data: &[u8]) -> Vec<u8> {
    zenwebp::oneshot::decode_rgb(data).unwrap().0
}

fn decode_gray_zenwebp(data: &[u8]) -> Vec<u8> {
    let (rgb, _, _) = zenwebp::oneshot::decode_rgb(data).unwrap();
    rgb
}

// ── image-webp ───────────────────────────────────────────────────────

fn decode_rgb_image_webp(data: &[u8]) -> Vec<u8> {
    use image_webp::WebPDecoder;
    let mut decoder = WebPDecoder::new(std::io::Cursor::new(data)).unwrap();
    let (w, h) = decoder.dimensions();
    let buf_size = decoder.output_buffer_size().unwrap();
    let mut buf = vec![0u8; buf_size];
    decoder.read_image(&mut buf).unwrap();
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for rgba in buf.chunks_exact(4) {
        rgb.push(rgba[0]);
        rgb.push(rgba[1]);
        rgb.push(rgba[2]);
    }
    rgb
}

fn decode_gray_image_webp(data: &[u8]) -> Vec<u8> {
    decode_rgb_image_webp(data)
}

// ── webp-rust ────────────────────────────────────────────────────────

fn decode_rgb_webp_rust(data: &[u8]) -> Vec<u8> {
    let img = webp_rust::decode(data).unwrap();
    let mut rgb = Vec::with_capacity(img.width * img.height * 3);
    for rgba in img.rgba.chunks_exact(4) {
        rgb.push(rgba[0]);
        rgb.push(rgba[1]);
        rgb.push(rgba[2]);
    }
    rgb
}

fn decode_gray_webp_rust(data: &[u8]) -> Vec<u8> {
    decode_rgb_webp_rust(data)
}

// ── webpx ────────────────────────────────────────────────────────────

fn decode_rgb_webpx(data: &[u8]) -> Vec<u8> {
    webpx::decode_rgb(data).unwrap().0
}

fn decode_gray_webpx(data: &[u8]) -> Vec<u8> {
    let (rgb, _, _) = webpx::decode_rgb(data).unwrap();
    rgb
}

// ── Criterion benchmarks ─────────────────────────────────────────────

macro_rules! bench_group {
    ($c:ident, $name:expr, $data:expr, $($decoder:ident),+) => {
        let mut g = $c.benchmark_group($name);
        g.sample_size(30);
        $(
            g.bench_function(stringify!($decoder), |b| b.iter(|| black_box($decoder(black_box($data)))));
        )+
        g.finish();
    };
}

fn bench_decode_color_lossy(c: &mut Criterion) {
    bench_group!(c, "decode_rgb_lossy", COLOR_LOSSY,
        decode_rgb_zenwebp, decode_rgb_image_webp, decode_rgb_webp_rust, decode_rgb_webpx);
}

fn bench_decode_gray_lossy(c: &mut Criterion) {
    bench_group!(c, "decode_gray_lossy", GRAY_LOSSY,
        decode_gray_zenwebp, decode_gray_image_webp, decode_gray_webp_rust, decode_gray_webpx);
}

fn bench_decode_color_lossless(c: &mut Criterion) {
    bench_group!(c, "decode_rgb_lossless", COLOR_LOSSLESS,
        decode_rgb_zenwebp, decode_rgb_image_webp, decode_rgb_webp_rust, decode_rgb_webpx);
}

fn bench_decode_gray_lossless(c: &mut Criterion) {
    bench_group!(c, "decode_gray_lossless", GRAY_LOSSLESS,
        decode_gray_zenwebp, decode_gray_image_webp, decode_gray_webp_rust, decode_gray_webpx);
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_decode_color_lossy, bench_decode_gray_lossy,
        bench_decode_color_lossless, bench_decode_gray_lossless,
}
criterion_main!(benches);
