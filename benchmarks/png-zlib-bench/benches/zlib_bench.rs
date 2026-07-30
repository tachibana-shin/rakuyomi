use criterion::{black_box, criterion_group, criterion_main, Criterion};

static COLOR_PNG: &[u8] = include_bytes!("../../data/color_test.png");
static GRAY_PNG: &[u8] = include_bytes!("../../data/gray_test.png");

// ── png (zlib-rs backend) ─────────────────────────────────────────────

fn decode_rgb_png_zlib_rs(data: &[u8]) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    buf
}

fn decode_gray_png_zlib_rs(data: &[u8]) -> Vec<u8> {
    decode_rgb_png_zlib_rs(data)
}

// ── spng (libspng) ────────────────────────────────────────────────────

fn decode_rgb_spng(data: &[u8]) -> Vec<u8> {
    let (_info, pixels) = spng::decode(data, spng::Format::Rgb8).unwrap();
    pixels
}

fn decode_gray_spng(data: &[u8]) -> Vec<u8> {
    let (_info, pixels) = spng::decode(data, spng::Format::G8).unwrap();
    pixels
}

// ── Raw inflate benchmarks (DEFLATE throughput) ───────────────────────

fn build_compressed_buf() -> (Vec<u8>, Vec<u8>) {
    use std::io::Write;
    // Generate a realistic buffer: RGB noise + gradient
    let w = 1600;
    let h = 2400;
    let mut raw = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            raw[i] = ((x as f32 / w as f32) * 255.0) as u8;
            raw[i + 1] = ((y as f32 / h as f32) * 255.0) as u8;
            raw[i + 2] =
                (128.0 + 64.0 * ((x as f32 / w as f32) * std::f32::consts::PI * 4.0).sin()
                    + 64.0 * ((y as f32 / h as f32) * std::f32::consts::PI * 4.0).cos()) as u8;
        }
    }
    let mut compressed = Vec::new();
    {
        let mut enc = flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::default());
        enc.write_all(&raw).unwrap();
        enc.finish().unwrap();
    }
    (compressed, raw)
}

fn inflate_flate2_zlib_rs(compressed: &[u8], expected_size: usize) -> Vec<u8> {
    use std::io::Read;
    let mut dec = flate2::read::ZlibDecoder::new(compressed);
    let mut buf = vec![0u8; expected_size];
    dec.read_exact(&mut buf).unwrap();
    buf
}

fn inflate_zune_inflate(compressed: &[u8], _expected_size: usize) -> Vec<u8> {
    use zune_inflate::DeflateDecoder;
    let mut dec = DeflateDecoder::new(compressed);
    dec.decode_zlib().unwrap()
}

// ── Criterion benchmarks ───────────────────────────────────────────────

fn bench_decode_rgb(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_rgb");
    g.sample_size(30);
    g.bench_function("png (zlib-rs)", |b| b.iter(|| black_box(decode_rgb_png_zlib_rs(black_box(COLOR_PNG)))));
    g.bench_function("spng",           |b| b.iter(|| black_box(decode_rgb_spng(black_box(COLOR_PNG)))));
    g.finish();
}

fn bench_decode_gray(c: &mut Criterion) {
    let mut g = c.benchmark_group("decode_gray");
    g.sample_size(30);
    g.bench_function("png (zlib-rs)", |b| b.iter(|| black_box(decode_gray_png_zlib_rs(black_box(GRAY_PNG)))));
    g.bench_function("spng",           |b| b.iter(|| black_box(decode_gray_spng(black_box(GRAY_PNG)))));
    g.finish();
}

fn bench_raw_inflate(c: &mut Criterion) {
    let (compressed, raw) = build_compressed_buf();
    let expected = raw.len();
    let mut g = c.benchmark_group("raw_inflate");
    g.sample_size(30);
    g.bench_function("flate2 (zlib-rs)", |b| b.iter(|| black_box(inflate_flate2_zlib_rs(&compressed, expected))));
    g.bench_function("zune-inflate",     |b| b.iter(|| black_box(inflate_zune_inflate(&compressed, expected))));
    g.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30);
    targets =
        bench_decode_rgb,
        bench_decode_gray,
        bench_raw_inflate,
}
criterion_main!(benches);
