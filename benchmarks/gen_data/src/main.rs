fn make_rgb(w: usize, h: usize) -> Vec<u8> {
    let mut rgb = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            rgb[i] = ((x as f32 / w as f32) * 255.0) as u8;
            rgb[i + 1] = ((y as f32 / h as f32) * 255.0) as u8;
            rgb[i + 2] = (128.0 + 64.0 * ((x as f32 / w as f32) * std::f32::consts::PI * 4.0).sin()
                + 64.0 * ((y as f32 / h as f32) * std::f32::consts::PI * 4.0).cos()) as u8;
        }
    }
    rgb
}

fn make_gray(w: usize, h: usize) -> Vec<u8> {
    let mut gray = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            gray[y * w + x] = ((x as f32 / w as f32) * 128.0 + (y as f32 / h as f32) * 128.0) as u8;
        }
    }
    gray
}

fn encode_bmp(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let row_size = ((w * 24 + 31) / 32) * 4;
    let pixel_data_size = row_size * h;
    let file_size = 14 + 40 + pixel_data_size;
    let mut bmp = Vec::with_capacity(file_size);

    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
    bmp.extend_from_slice(&[0u8; 4]);
    bmp.extend_from_slice(&(54u32).to_le_bytes());
    bmp.extend_from_slice(&(40u32).to_le_bytes());
    bmp.extend_from_slice(&(w as u32).to_le_bytes());
    bmp.extend_from_slice(&(h as u32).to_le_bytes());
    bmp.extend_from_slice(&(1u16).to_le_bytes());
    bmp.extend_from_slice(&(24u16).to_le_bytes());
    bmp.extend_from_slice(&(0u32).to_le_bytes());
    bmp.extend_from_slice(&(pixel_data_size as u32).to_le_bytes());
    bmp.extend_from_slice(&[0u8; 16]);

    for y in (0..h).rev() {
        let row_start = y * w * 3;
        for x in 0..w {
            let i = row_start + x * 3;
            bmp.push(rgb[i + 2]);
            bmp.push(rgb[i + 1]);
            bmp.push(rgb[i]);
        }
        for _ in 0..(row_size - w * 3) {
            bmp.push(0);
        }
    }
    bmp
}

fn encode_bmp_gray(gray: &[u8], w: usize, h: usize) -> Vec<u8> {
    let row_size = ((w * 24 + 31) / 32) * 4;
    let pixel_data_size = row_size * h;
    let file_size = 14 + 40 + pixel_data_size;
    let mut bmp = Vec::with_capacity(file_size);

    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
    bmp.extend_from_slice(&[0u8; 4]);
    bmp.extend_from_slice(&(54u32).to_le_bytes());
    bmp.extend_from_slice(&(40u32).to_le_bytes());
    bmp.extend_from_slice(&(w as u32).to_le_bytes());
    bmp.extend_from_slice(&(h as u32).to_le_bytes());
    bmp.extend_from_slice(&(1u16).to_le_bytes());
    bmp.extend_from_slice(&(24u16).to_le_bytes());
    bmp.extend_from_slice(&(0u32).to_le_bytes());
    bmp.extend_from_slice(&(pixel_data_size as u32).to_le_bytes());
    bmp.extend_from_slice(&[0u8; 16]);

    for y in (0..h).rev() {
        let row_start = y * w;
        for x in 0..w {
            let g = gray[row_start + x];
            bmp.push(g);
            bmp.push(g);
            bmp.push(g);
        }
        for _ in 0..(row_size - w * 3) {
            bmp.push(0);
        }
    }
    bmp
}

fn main() {
    let w = 1600usize;
    let h = 2400usize;

    let out_dir = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.push("data");
        std::fs::create_dir_all(&p).unwrap();
        p
    };

    let rgb = make_rgb(w, h);

    // Color JPEG
    {
        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        comp.set_size(w, h);
        comp.set_quality(80.0);
        comp.set_fastest_defaults();
        let out = Vec::with_capacity(w * h);
        let mut comp = comp.start_compress(out).unwrap();
        comp.write_scanlines(&rgb).unwrap();
        let data = comp.finish().unwrap();
        std::fs::write(out_dir.join("color_test.jpg"), &data).unwrap();
        eprintln!("Color JPEG: {} bytes ({}x{})", data.len(), w, h);
    }

    // Gray JPEG
    {
        let gray = make_gray(w, h);
        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_GRAYSCALE);
        comp.set_size(w, h);
        comp.set_quality(80.0);
        comp.set_fastest_defaults();
        let out = Vec::with_capacity(w * h);
        let mut comp = comp.start_compress(out).unwrap();
        comp.write_scanlines(&gray).unwrap();
        let data = comp.finish().unwrap();
        std::fs::write(out_dir.join("gray_test.jpg"), &data).unwrap();
        eprintln!("Gray JPEG: {} bytes ({}x{})", data.len(), w, h);
    }

    // Color PNG (RGB)
    {
        let png_data = lodepng::encode24(&rgb, w, h).unwrap();
        std::fs::write(out_dir.join("color_test.png"), &png_data).unwrap();
        eprintln!("Color PNG: {} bytes ({}x{})", png_data.len(), w, h);
    }

    // Gray PNG
    {
        let gray = make_gray(w, h);
        let png_data = lodepng::encode_memory(&gray, w, h, lodepng::ColorType::GREY, 8).unwrap();
        std::fs::write(out_dir.join("gray_test.png"), &png_data).unwrap();
        eprintln!("Gray PNG: {} bytes ({}x{})", png_data.len(), w, h);
    }

    // Color WebP (lossy)
    {
        let webp_data = webpx::Encoder::new_rgb(&rgb, w as u32, h as u32)
            .quality(80.0)
            .encode(webpx::Unstoppable)
            .unwrap();
        std::fs::write(out_dir.join("color_test.webp"), &webp_data).unwrap();
        eprintln!("Color WebP (lossy): {} bytes ({}x{})", webp_data.len(), w, h);
    }

    // Gray WebP (lossy)
    {
        let gray = make_gray(w, h);
        let gray_rgb: Vec<u8> = gray.iter().flat_map(|&g| [g, g, g]).collect();
        let webp_data = webpx::Encoder::new_rgb(&gray_rgb, w as u32, h as u32)
            .quality(80.0)
            .encode(webpx::Unstoppable)
            .unwrap();
        std::fs::write(out_dir.join("gray_test.webp"), &webp_data).unwrap();
        eprintln!("Gray WebP (lossy): {} bytes ({}x{})", webp_data.len(), w, h);
    }

    // Color WebP (lossless)
    {
        let webp_data = webpx::Encoder::new_rgb(&rgb, w as u32, h as u32)
            .lossless(true)
            .encode(webpx::Unstoppable)
            .unwrap();
        std::fs::write(out_dir.join("color_lossless.webp"), &webp_data).unwrap();
        eprintln!("Color WebP (lossless): {} bytes ({}x{})", webp_data.len(), w, h);
    }

    // Gray WebP (lossless)
    {
        let gray = make_gray(w, h);
        let gray_rgb: Vec<u8> = gray.iter().flat_map(|&g| [g, g, g]).collect();
        let webp_data = webpx::Encoder::new_rgb(&gray_rgb, w as u32, h as u32)
            .lossless(true)
            .encode(webpx::Unstoppable)
            .unwrap();
        std::fs::write(out_dir.join("gray_lossless.webp"), &webp_data).unwrap();
        eprintln!("Gray WebP (lossless): {} bytes ({}x{})", webp_data.len(), w, h);
    }

    // Color BMP
    {
        let bmp_data = encode_bmp(&rgb, w, h);
        std::fs::write(out_dir.join("color_test.bmp"), &bmp_data).unwrap();
        eprintln!("Color BMP: {} bytes ({}x{})", bmp_data.len(), w, h);
    }

    // Gray BMP
    {
        let gray = make_gray(w, h);
        let bmp_data = encode_bmp_gray(&gray, w, h);
        std::fs::write(out_dir.join("gray_test.bmp"), &bmp_data).unwrap();
        eprintln!("Gray BMP: {} bytes ({}x{})", bmp_data.len(), w, h);
    }

    // Color AVIF
    {
        let pixels: Vec<rgb::RGB8> = rgb.chunks_exact(3).map(|c| rgb::RGB8::new(c[0], c[1], c[2])).collect();
        let img = ravif::Img::new(pixels.as_slice(), w, h);
        let encoded = ravif::Encoder::new()
            .with_quality(80.0)
            .with_speed(10)
            .encode_rgb(img)
            .unwrap();
        std::fs::write(out_dir.join("color_test.avif"), &encoded.avif_file).unwrap();
        eprintln!("Color AVIF: {} bytes ({}x{})", encoded.avif_file.len(), w, h);
    }

    // Gray AVIF
    {
        let gray = make_gray(w, h);
        let pixels: Vec<rgb::RGB8> = gray.iter().map(|&g| rgb::RGB8::new(g, g, g)).collect();
        let img = ravif::Img::new(pixels.as_slice(), w, h);
        let encoded = ravif::Encoder::new()
            .with_quality(80.0)
            .with_speed(10)
            .encode_rgb(img)
            .unwrap();
        std::fs::write(out_dir.join("gray_test.avif"), &encoded.avif_file).unwrap();
        eprintln!("Gray AVIF: {} bytes ({}x{})", encoded.avif_file.len(), w, h);
    }

    // Color JPEG XL (via cjxl CLI)
    {
        use std::io::Write;
        let ppm = format!("P6\n{} {}\n255\n", w, h);
        let mut child = std::process::Command::new("cjxl")
            .args(["-", out_dir.join("color_test.jxl").to_str().unwrap(), "--quiet"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(ppm.as_bytes()).unwrap();
        stdin.write_all(&rgb).unwrap();
        let _ = stdin;
        let status = child.wait().unwrap();
        assert!(status.success());
        let data = std::fs::read(out_dir.join("color_test.jxl")).unwrap();
        eprintln!("Color JXL: {} bytes ({}x{})", data.len(), w, h);
    }

    // Gray JPEG XL (via cjxl CLI)
    {
        use std::io::Write;
        let gray = make_gray(w, h);
        let pgm = format!("P5\n{} {}\n255\n", w, h);
        let mut child = std::process::Command::new("cjxl")
            .args(["-", out_dir.join("gray_test.jxl").to_str().unwrap(), "--quiet"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(pgm.as_bytes()).unwrap();
        stdin.write_all(&gray).unwrap();
        let _ = stdin;
        let status = child.wait().unwrap();
        assert!(status.success());
        let data = std::fs::read(out_dir.join("gray_test.jxl")).unwrap();
        eprintln!("Gray JXL: {} bytes ({}x{})", data.len(), w, h);
    }
}
