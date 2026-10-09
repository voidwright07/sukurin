//! Text and QR recognition for the Text capture mode. Runs off the main
//! thread; the OS OCR engine is used where one exists.

use anyhow::Result;
use sukurin_core::Bitmap;

pub struct Recognized {
    pub lines: Vec<String>,
    pub codes: Vec<String>,
    pub language: String,
}

pub fn recognize(b: &Bitmap) -> Result<Recognized> {
    let codes = qr_codes(b);
    let (lines, language) = ocr(b)?;
    Ok(Recognized { lines, codes, language })
}

fn qr_codes(b: &Bitmap) -> Vec<String> {
    let (w, h) = (b.width() as usize, b.height() as usize);
    let px = b.bgra();
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| {
        let i = (y * w + x) * 4;
        ((px[i] as u32 * 29 + px[i + 1] as u32 * 150 + px[i + 2] as u32 * 77) >> 8) as u8
    });
    img.detect_grids()
        .into_iter()
        .filter_map(|g| g.decode().ok().map(|(_, text)| text))
        .collect()
}

/// Small UI text recognises far better when enlarged; the OS engines are
/// tuned for camera-sized glyphs.
fn upscale_for_ocr(b: &Bitmap) -> Bitmap {
    let k = if b.height() < 400 { 3 } else if b.height() < 1200 { 2 } else { 1 };
    if k == 1 {
        return b.clone();
    }
    let (w, h) = (b.width() * k, b.height() * k);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let src = b.bgra();
    for y in 0..h {
        let sy = (y / k) as usize;
        for x in 0..w {
            let s = (sy * b.width() as usize + (x / k) as usize) * 4;
            let d = ((y * w + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    Bitmap::from_bgra(w, h, out)
}

#[cfg(windows)]
fn ocr(b: &Bitmap) -> Result<(Vec<String>, String)> {
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;

    let mut b = upscale_for_ocr(b);
    let max = OcrEngine::MaxImageDimension()?;
    if b.width() > max || b.height() > max {
        b = b.downscale_to_fit(max, max);
    }
    let writer = DataWriter::new()?;
    writer.WriteBytes(b.bgra())?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, b.width() as i32, b.height() as i32)?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let language = engine.RecognizerLanguage()?.LanguageTag()?.to_string();
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let lines = result.Lines()?;
    let mut out = Vec::with_capacity(lines.Size()? as usize);
    for i in 0..lines.Size()? {
        out.push(lines.GetAt(i)?.Text()?.to_string());
    }
    Ok((out, language))
}

#[cfg(not(windows))]
fn ocr(_: &Bitmap) -> Result<(Vec<String>, String)> {
    anyhow::bail!("text recognition is not available on this platform yet")
}
