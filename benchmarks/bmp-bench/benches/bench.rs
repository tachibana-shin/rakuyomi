use criterion::{black_box, criterion_group, criterion_main, Criterion};

static COLOR_BMP: &[u8] = include_bytes!("../../data/color_test.bmp");
static GRAY_BMP: &[u8] = include_bytes!("../../data/gray_test.bmp");

fn decode_rgb_bmp_crate(data: &[u8]) -> Vec<u8> {
    let img = bmp::from_reader(&mut std::io::Cursor::new(data)).unwrap();
    let (w, h) = (img.get_width() as usize, img.get_height() as usize);
    let mut rgb = vec![0u8; w * h * 3];
    for (x, y) in img.coordinates() {
        let pixel = img.get_pixel(x, y);
        let i = (y as usize * w + x as usize) * 3;
        rgb[i] = pixel.r;
        rgb[i + 1] = pixel.g;
        rgb[i + 2] = pixel.b;
    }
    rgb
}

fn decode_rgb_image_crate(data: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(data).unwrap().to_rgb8();
    img.into_raw()
}

fn decode_gray_image_crate(data: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(data).unwrap().to_luma8();
    img.into_raw()
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
    bench_group!(c, "decode_rgb", COLOR_BMP, decode_rgb_bmp_crate, decode_rgb_image_crate);
}

fn bench_decode_gray(c: &mut Criterion) {
    bench_group!(c, "decode_gray", GRAY_BMP, decode_gray_image_crate);
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_decode_color, bench_decode_gray,
}
criterion_main!(benches);
