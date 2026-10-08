//! Getting a capture out of Shattr and into another app: dragged out as a
//! file, or put on the clipboard as an image plus a text description for an
//! AI chat to read.

use anyhow::{Context as _, Result};
use shattr_core::{Bitmap, Shot};
use std::path::PathBuf;

/// The capture as a PNG in the temp folder, named the way a saved one would
/// be, so the receiving app shows a sensible file name.
pub fn temp_png(shot: &Shot) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("Shattr");
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::DateTime::<chrono::Local>::from(shot.taken_at).format("%Y-%m-%d at %H.%M.%S");
    let path = dir.join(format!("Shattr {stamp}.png"));
    if !path.exists() {
        std::fs::write(&path, encode_png(&shot.bitmap)?)?;
    }
    Ok(path)
}

pub fn encode_png(b: &Bitmap) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive).write_image(
        &b.to_rgba(),
        b.width(),
        b.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

/// Runs a shell drag of `path` as a file. Blocks in the OS drag loop until
/// the user drops or cancels, which is how Windows drag sources work.
#[cfg(windows)]
pub fn drag_file(path: &std::path::Path) -> Result<()> {
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{DROPEFFECT_COPY, IDropSource};
    use windows::Win32::UI::Shell::{BHID_DataObject, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop};
    use windows::core::HSTRING;
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None)?;
        let data: IDataObject = item.BindToHandler(None, &BHID_DataObject)?;
        SHDoDragDrop(None, &data, None::<&IDropSource>, DROPEFFECT_COPY).context("drag")?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn drag_file(_: &std::path::Path) -> Result<()> {
    anyhow::bail!("dragging out is not available on this platform yet")
}

/// What a model should know about a capture besides its pixels.
pub fn ai_context(shot: &Shot, text: &[String]) -> String {
    let (w, h) = (shot.bitmap.width(), shot.bitmap.height());
    let mut s = format!(
        "Screenshot, {} × {} pt ({w} × {h} px at {}x scale).",
        (w as f32 / shot.scale).round(),
        (h as f32 / shot.scale).round(),
        shot.scale
    );
    let lines: Vec<&String> = text.iter().filter(|l| !l.trim().is_empty()).collect();
    if !lines.is_empty() {
        s.push_str("\n\nText in the screenshot:\n");
        for l in lines {
            s.push_str(l);
            s.push('\n');
        }
    }
    s
}

/// Puts the image (as both a bitmap and PNG) and `text` on the clipboard at
/// once. Pasting into a chat then attaches the image; plain-text targets get
/// the description.
#[cfg(windows)]
pub fn copy_image_and_text(b: &Bitmap, text: &str) -> Result<()> {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::core::w;

    const CF_DIB: u32 = 8;
    const CF_UNICODETEXT: u32 = 13;

    fn global(bytes: &[u8]) -> Result<HGLOBAL> {
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let p = GlobalLock(h) as *mut u8;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            let _ = GlobalUnlock(h);
            Ok(h)
        }
    }

    let mut dib = Vec::with_capacity(40 + b.bgra().len());
    let header: [u32; 10] = [40, b.width(), (b.height() as i32) as u32, 1 | (32 << 16), 0, b.bgra().len() as u32, 2835, 2835, 0, 0];
    for v in header {
        dib.extend_from_slice(&v.to_le_bytes());
    }
    let row = b.width() as usize * 4;
    for y in (0..b.height() as usize).rev() {
        dib.extend_from_slice(&b.bgra()[y * row..(y + 1) * row]);
    }
    let png = encode_png(b)?;
    let utf16: Vec<u8> = text.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect();

    unsafe {
        OpenClipboard(None).context("open clipboard")?;
        let result = (|| -> Result<()> {
            EmptyClipboard()?;
            SetClipboardData(CF_DIB, Some(HANDLE(global(&dib)?.0)))?;
            let png_format = RegisterClipboardFormatW(w!("PNG"));
            SetClipboardData(png_format, Some(HANDLE(global(&png)?.0)))?;
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global(&utf16)?.0)))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

#[cfg(not(windows))]
pub fn copy_image_and_text(b: &Bitmap, text: &str) -> Result<()> {
    let mut c = arboard::Clipboard::new()?;
    c.set_image(arboard::ImageData { width: b.width() as usize, height: b.height() as usize, bytes: b.to_rgba().into() })?;
    let _ = text;
    Ok(())
}

/// Where "Send to AI" opens. `SHATTR_AI_URL` overrides the default chat.
pub fn ai_url() -> String {
    std::env::var("SHATTR_AI_URL").unwrap_or_else(|_| "https://claude.ai/new".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_context_states_size_and_only_non_empty_text() {
        let shot = Shot {
            bitmap: Bitmap::from_bgra(300, 150, vec![0; 300 * 150 * 4]),
            scale: 1.5,
            source_rect: shattr_core::RectPx::new(0, 0, 300, 150),
            taken_at: std::time::SystemTime::now(),
        };
        let s = ai_context(&shot, &["Deploy".into(), "  ".into(), "Logs".into()]);
        assert!(s.starts_with("Screenshot, 200 × 100 pt (300 × 150 px at 1.5x scale)."));
        assert!(s.ends_with("Text in the screenshot:\nDeploy\nLogs\n"));
        assert_eq!(ai_context(&shot, &[]).lines().count(), 1);
    }
}
