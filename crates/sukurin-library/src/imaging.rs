use std::path::Path;

use anyhow::Result;
use image::ImageReader;
use image::codecs::jpeg::JpegEncoder;
use sukurin_core::Bitmap;

pub(crate) fn decode(path: &Path) -> Result<Bitmap> {
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()?.into_rgba8();
    let (w, h) = img.dimensions();
    Ok(Bitmap::from_rgba(w, h, img.into_raw()))
}

const THUMB_MAX_W: u32 = 320;
const THUMB_MAX_H: u32 = 960;
/// Source pixels sampled per output pixel along each axis. Averaging 4x4
/// samples of a large block is indistinguishable at thumbnail size and keeps
/// a 2560x1600 thumbnail near a millisecond.
const THUMB_SAMPLES: u32 = 4;

/// A JPEG at most 320 px wide (and 960 px tall, for scrolling captures),
/// box-filtered from the bitmap.
pub(crate) fn thumbnail_jpeg(b: &Bitmap) -> Result<Vec<u8>> {
    let (w, h) = (b.width(), b.height());
    anyhow::ensure!(w > 0 && h > 0, "empty bitmap");
    let scale = (THUMB_MAX_W as f32 / w as f32).min(THUMB_MAX_H as f32 / h as f32).min(1.0);
    let tw = ((w as f32 * scale).round() as u32).max(1);
    let th = ((h as f32 * scale).round() as u32).max(1);
    let px = b.bgra();
    let mut rgb = Vec::with_capacity((tw * th * 3) as usize);
    for oy in 0..th {
        let (y0, y1) = (oy * h / th, ((oy + 1) * h / th).max(oy * h / th + 1));
        let ystep = ((y1 - y0) / THUMB_SAMPLES).max(1);
        for ox in 0..tw {
            let (x0, x1) = (ox * w / tw, ((ox + 1) * w / tw).max(ox * w / tw + 1));
            let xstep = ((x1 - x0) / THUMB_SAMPLES).max(1);
            let (mut r, mut g, mut bl, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in (y0..y1).step_by(ystep as usize) {
                for x in (x0..x1).step_by(xstep as usize) {
                    let i = (y as usize * w as usize + x as usize) * 4;
                    bl += px[i] as u32;
                    g += px[i + 1] as u32;
                    r += px[i + 2] as u32;
                    n += 1;
                }
            }
            rgb.extend_from_slice(&[(r / n) as u8, (g / n) as u8, (bl / n) as u8]);
        }
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 80).encode(&rgb, tw, th, image::ExtendedColorType::Rgb8)?;
    Ok(out)
}
