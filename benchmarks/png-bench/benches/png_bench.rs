use criterion::{black_box, criterion_group, criterion_main, Criterion};

static COLOR_PNG: &[u8] = include_bytes!("../../data/color_test.png");
static GRAY_PNG: &[u8] = include_bytes!("../../data/gray_test.png");

// ── zune-png ───────────────────────────────────────────────────────────

fn decode_rgb_zune_png(data: &[u8]) -> Vec<u8> {
    let mut dec = zune_png::PngDecoder::new(zune_core::bytestream::ZCursor::new(data));
    dec.decode_raw().unwrap()
}

fn decode_gray_zune_png(data: &[u8]) -> Vec<u8> {
    decode_rgb_zune_png(data)
}

// ── png ────────────────────────────────────────────────────────────────

fn decode_rgb_png(data: &[u8]) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    buf
}

fn decode_gray_png(data: &[u8]) -> Vec<u8> {
    decode_rgb_png(data)
}

// ── lodepng ────────────────────────────────────────────────────────────

fn decode_rgb_lodepng(data: &[u8]) -> Vec<u8> {
    let bitmap = lodepng::decode24(data).unwrap();
    bytemuck::cast_slice(&bitmap.buffer).to_vec()
}

fn decode_gray_lodepng(data: &[u8]) -> Vec<u8> {
    let image = lodepng::decode_memory(data, lodepng::ColorType::GREY, 8).unwrap();
    match image {
        lodepng::Image::Grey(bmp) => bytemuck::cast_slice(&bmp.buffer).to_vec(),
        _ => panic!("expected grey"),
    }
}

// ── png_pong ───────────────────────────────────────────────────────────

fn decode_rgb_png_pong(data: &[u8]) -> Vec<u8> {
    use png_pong::PngRaster;
    let cursor = std::io::Cursor::new(data);
    let decoder = png_pong::Decoder::new(cursor).unwrap();
    let step = decoder.into_steps().last().unwrap().unwrap();
    match step.raster {
        PngRaster::Rgb8(raster) => raster.as_u8_slice().to_vec(),
        _ => panic!("unexpected raster type"),
    }
}

fn decode_gray_png_pong(data: &[u8]) -> Vec<u8> {
    use png_pong::PngRaster;
    let cursor = std::io::Cursor::new(data);
    let decoder = png_pong::Decoder::new(cursor).unwrap();
    let step = decoder.into_steps().last().unwrap().unwrap();
    match step.raster {
        PngRaster::Gray8(raster) => raster.as_u8_slice().to_vec(),
        _ => panic!("unexpected raster type"),
    }
}

// ── zenpng ─────────────────────────────────────────────────────────────

fn decode_rgb_zenpng(data: &[u8]) -> Vec<u8> {
    use enough::Unstoppable;
    use zenpng::PngDecodeConfig;
    let output = zenpng::decode(data, &PngDecodeConfig::default(), &Unstoppable).unwrap();
    output.pixels.into_vec()
}

fn decode_gray_zenpng(data: &[u8]) -> Vec<u8> {
    decode_rgb_zenpng(data)
}

// ── Criterion benchmarks ───────────────────────────────────────────────

fn bench_decode_rgb(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_rgb");
    g.sample_size(30);
    g.bench_function("zune-png",   |b| b.iter(|| black_box(decode_rgb_zune_png(black_box(COLOR_PNG)))));
    g.bench_function("png",        |b| b.iter(|| black_box(decode_rgb_png(black_box(COLOR_PNG)))));
    g.bench_function("lodepng",    |b| b.iter(|| black_box(decode_rgb_lodepng(black_box(COLOR_PNG)))));
    g.bench_function("png_pong",   |b| b.iter(|| black_box(decode_rgb_png_pong(black_box(COLOR_PNG)))));
    g.bench_function("zenpng",     |b| b.iter(|| black_box(decode_rgb_zenpng(black_box(COLOR_PNG)))));
    g.finish();
}

fn bench_decode_gray(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_gray");
    g.sample_size(30);
    g.bench_function("zune-png",   |b| b.iter(|| black_box(decode_gray_zune_png(black_box(GRAY_PNG)))));
    g.bench_function("png",        |b| b.iter(|| black_box(decode_gray_png(black_box(GRAY_PNG)))));
    g.bench_function("lodepng",    |b| b.iter(|| black_box(decode_gray_lodepng(black_box(GRAY_PNG)))));
    g.bench_function("png_pong",   |b| b.iter(|| black_box(decode_gray_png_pong(black_box(GRAY_PNG)))));
    g.bench_function("zenpng",     |b| b.iter(|| black_box(decode_gray_zenpng(black_box(GRAY_PNG)))));
    g.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets =
        bench_decode_rgb,
        bench_decode_gray,
}
criterion_main!(benches);
