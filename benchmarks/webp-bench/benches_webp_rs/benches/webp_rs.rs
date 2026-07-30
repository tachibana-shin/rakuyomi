use criterion::{black_box, criterion_group, criterion_main, Criterion};

static COLOR_LOSSY: &[u8] = include_bytes!("../../../data/color_test.webp");
static GRAY_LOSSY: &[u8] = include_bytes!("../../../data/gray_test.webp");
static COLOR_LOSSLESS: &[u8] = include_bytes!("../../../data/color_lossless.webp");
static GRAY_LOSSLESS: &[u8] = include_bytes!("../../../data/gray_lossless.webp");

fn decode_rgb_webp_rs(data: &[u8]) -> Vec<u8> {
    let img = webp::decode(data).unwrap();
    img.to_rgb8().into_raw()
}

macro_rules! bench_group {
    ($c:ident, $name:expr, $data:expr) => {
        let mut g = $c.benchmark_group($name);
        g.sample_size(30);
        g.bench_function("webp-rs", |b| b.iter(|| black_box(decode_rgb_webp_rs(black_box($data)))));
        g.finish();
    };
}

fn bench_decode_color_lossy(c: &mut Criterion) {
    bench_group!(c, "decode_rgb_lossy", COLOR_LOSSY);
}

fn bench_decode_gray_lossy(c: &mut Criterion) {
    bench_group!(c, "decode_gray_lossy", GRAY_LOSSY);
}

fn bench_decode_color_lossless(c: &mut Criterion) {
    bench_group!(c, "decode_rgb_lossless", COLOR_LOSSLESS);
}

fn bench_decode_gray_lossless(c: &mut Criterion) {
    bench_group!(c, "decode_gray_lossless", GRAY_LOSSLESS);
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_decode_color_lossy, bench_decode_gray_lossy,
        bench_decode_color_lossless, bench_decode_gray_lossless,
}
criterion_main!(benches);
