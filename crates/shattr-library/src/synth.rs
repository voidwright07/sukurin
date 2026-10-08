//! Screenshot-like test images: a window with a header bar, a sidebar and a
//! striped table whose cells hold glyph-textured "words".

use image::{DynamicImage, ImageBuffer, RgbaImage, imageops::FilterType};
use shattr_core::Bitmap;

#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// One table row: the seed that generates its words.
pub type Row = u64;

pub fn rows(n: usize) -> Vec<Row> {
    (0..n as u64).map(|i| 1000 + i).collect()
}

pub fn screen(w: u32, h: u32, rows: &[Row]) -> Bitmap {
    let mut img: RgbaImage = ImageBuffer::from_raw(w, h, vec![255; w as usize * h as usize * 4]).unwrap();
    let header = h * 6 / 100;
    let sidebar = w * 18 / 100;
    fill(&mut img, 0, 0, w, header, [38, 58, 112]);
    fill(&mut img, 0, header, sidebar, h - header, [242, 243, 245]);
    fill(&mut img, sidebar, header, 1, h - header, [210, 210, 214]);
    let glyph_h = (h / 90).max(5);
    let glyph_w = glyph_h * 6 / 10;
    text(&mut img, w / 60, header / 3, glyph_w, glyph_h, 7, [235, 235, 245], 7);
    for i in 0..8u32 {
        text(&mut img, w / 60, header + glyph_h * 2 + i * glyph_h * 3, glyph_w, glyph_h, 99 + i as u64, [70, 70, 80], 3);
    }
    let row_h = h * 35 / 1000;
    let top = header + row_h;
    for (i, &seed) in rows.iter().enumerate() {
        let y = top + i as u32 * row_h;
        if y + row_h > h {
            break;
        }
        if i % 2 == 1 {
            fill(&mut img, sidebar + 1, y, w - sidebar - 1, row_h, [246, 247, 249]);
        }
        fill(&mut img, sidebar + 1, y + row_h - 1, w - sidebar - 1, 1, [225, 225, 230]);
        let mut rng = Rng::new(seed);
        let mut x = sidebar + w / 80;
        let ty = y + (row_h - glyph_h) / 2;
        for column in 0..4 {
            let words = 1 + rng.below(3);
            for _ in 0..words {
                let len = 2 + rng.below(8) as u32;
                text(&mut img, x, ty, glyph_w, glyph_h, rng.next(), [45, 45, 52], len);
                x += (len + 1) * (glyph_w + 1);
            }
            x = sidebar + w / 80 + (column + 1) * (w - sidebar) / 4;
        }
    }
    Bitmap::from_rgba(w, h, img.into_raw())
}

/// A run of `len` glyphs, each a few random 1px strokes in a glyph box.
#[allow(clippy::too_many_arguments)]
fn text(img: &mut RgbaImage, x: u32, y: u32, gw: u32, gh: u32, seed: u64, color: [u8; 3], len: u32) {
    let mut rng = Rng::new(seed);
    for g in 0..len {
        let gx = x + g * (gw + 1);
        for _ in 0..2 + rng.below(2) {
            let sx = gx + rng.below(gw as u64) as u32;
            fill(img, sx, y, 1, gh, color);
        }
        let sy = y + rng.below(gh as u64) as u32;
        fill(img, gx, sy, gw, 1, color);
    }
}

fn fill(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, c: [u8; 3]) {
    let (iw, ih) = img.dimensions();
    let (x1, y1) = ((x + w).min(iw) as usize, (y + h).min(ih) as usize);
    let x0 = (x as usize).min(x1);
    let span: Vec<u8> = [c[0], c[1], c[2], 255].repeat(x1 - x0);
    for row in img.chunks_exact_mut(iw as usize * 4).take(y1).skip(y as usize) {
        row[x0 * 4..x1 * 4].copy_from_slice(&span);
    }
}

pub fn uniform(w: u32, h: u32, c: [u8; 3]) -> Bitmap {
    Bitmap::from_rgba(w, h, [c[0], c[1], c[2], 255].repeat(w as usize * h as usize))
}

pub fn to_image(b: &Bitmap) -> RgbaImage {
    ImageBuffer::from_raw(b.width(), b.height(), b.to_rgba()).unwrap()
}

pub fn from_image(img: RgbaImage) -> Bitmap {
    Bitmap::from_rgba(img.width(), img.height(), img.into_raw())
}

pub fn jpeg_bytes(b: &Bitmap, quality: u8) -> Vec<u8> {
    let rgb = DynamicImage::ImageRgba8(to_image(b)).into_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(rgb.as_raw(), b.width(), b.height(), image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

pub fn png_bytes(b: &Bitmap) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    to_image(b).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

pub fn reencode_jpeg(b: &Bitmap, quality: u8) -> Bitmap {
    from_image(image::load_from_memory(&jpeg_bytes(b, quality)).unwrap().into_rgba8())
}

pub fn resize(b: &Bitmap, w: u32, h: u32) -> Bitmap {
    from_image(DynamicImage::ImageRgba8(to_image(b)).resize_exact(w, h, FilterType::Triangle).into_rgba8())
}
