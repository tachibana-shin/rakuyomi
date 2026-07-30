use criterion::{black_box, criterion_group, criterion_main, Criterion};

static COLOR_AVIF: &[u8] = include_bytes!("../../data/color_test.avif");
static GRAY_AVIF: &[u8] = include_bytes!("../../data/gray_test.avif");

fn decode_rgb_image_crate(data: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(data).unwrap().to_rgb8();
    img.into_raw()
}

fn decode_rgb_libavif(data: &[u8]) -> Vec<u8> {
    let rgb = libavif::decode_rgb(data).unwrap();
    let pixels: &[u8] = &rgb;
    pixels.to_vec()
}

fn decode_gray_image_crate(data: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(data).unwrap().to_luma8();
    img.into_raw()
}

fn decode_gray_libavif(data: &[u8]) -> Vec<u8> {
    let rgb = libavif::decode_rgb(data).unwrap();
    let pixels: &[u8] = &rgb;
    pixels.to_vec()
}

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

fn bench_decode_color(c: &mut Criterion) {
    bench_group!(c, "decode_rgb", COLOR_AVIF,
        decode_rgb_image_crate, decode_rgb_libavif);
}

fn bench_decode_gray(c: &mut Criterion) {
    bench_group!(c, "decode_gray", GRAY_AVIF,
        decode_gray_image_crate, decode_gray_libavif);
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_decode_color, bench_decode_gray,
}
criterion_main!(benches);
