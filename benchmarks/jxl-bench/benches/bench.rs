use criterion::{black_box, criterion_group, criterion_main, Criterion};

static COLOR_JXL: &[u8] = include_bytes!("../../data/color_test.jxl");
static GRAY_JXL: &[u8] = include_bytes!("../../data/gray_test.jxl");

fn decode_rgb_jxl_oxide(data: &[u8]) -> Vec<u8> {
    let img = jxl_oxide::JxlImage::read_with_defaults(std::io::Cursor::new(data)).unwrap();
    let render = img.render_frame(0).unwrap();
    let mut stream = render.stream_no_alpha();
    let w = stream.width() as usize;
    let h = stream.height() as usize;
    let ch = stream.channels() as usize;
    let mut pixels = vec![0u8; w * h * ch];
    stream.write_to_buffer(&mut pixels);
    pixels
}

fn decode_rgb_jpegxl_rs(data: &[u8]) -> Vec<u8> {
    let decoder = jpegxl_rs::decoder_builder().build().unwrap();
    let (_, pixels) = decoder.decode_with::<u8>(data).unwrap();
    pixels
}

fn decode_gray_jxl_oxide(data: &[u8]) -> Vec<u8> {
    let img = jxl_oxide::JxlImage::read_with_defaults(std::io::Cursor::new(data)).unwrap();
    let render = img.render_frame(0).unwrap();
    let mut stream = render.stream_no_alpha();
    let w = stream.width() as usize;
    let h = stream.height() as usize;
    let ch = stream.channels() as usize;
    let mut pixels = vec![0u8; w * h * ch];
    stream.write_to_buffer(&mut pixels);
    pixels
}

fn decode_gray_jpegxl_rs(data: &[u8]) -> Vec<u8> {
    let decoder = jpegxl_rs::decoder_builder().build().unwrap();
    let (_, pixels) = decoder.decode_with::<u8>(data).unwrap();
    pixels
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
    bench_group!(c, "decode_rgb", COLOR_JXL,
        decode_rgb_jxl_oxide, decode_rgb_jpegxl_rs);
}

fn bench_decode_gray(c: &mut Criterion) {
    bench_group!(c, "decode_gray", GRAY_JXL,
        decode_gray_jxl_oxide, decode_gray_jpegxl_rs);
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets = bench_decode_color, bench_decode_gray,
}
criterion_main!(benches);
