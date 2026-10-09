//! Screen grabbing and window discovery. Everything here speaks physical
//! pixels on the virtual desktop.

#[cfg(windows)]
mod dxgi;
#[cfg(windows)]
mod hdr;
#[cfg(windows)]
mod win_windows;

use anyhow::{Result, anyhow};
#[cfg(not(windows))]
use sukurin_core::Bitmap;
use sukurin_core::{Display, Frame, PointPx, RectPx};
use std::time::SystemTime;

/// A top-level window the overlay can snap to.
#[derive(Clone, Debug)]
pub struct SnapWindow {
    pub rect: RectPx,
    pub title: String,
}

pub struct Capturer {
    #[cfg(windows)]
    outputs: Vec<dxgi::Output>,
    #[cfg(not(windows))]
    monitors: Vec<(Display, xcap::Monitor)>,
}

impl Capturer {
    pub fn new() -> Result<Self> {
        #[cfg(windows)]
        {
            let mut outputs = dxgi::outputs()?;
            for o in &mut outputs {
                if let Err(e) = o.warm() {
                    log::warn!("could not pre-open capture for {}: {e:#}", o.display.name);
                }
            }
            Ok(Self { outputs })
        }
        #[cfg(not(windows))]
        {
            let monitors = xcap::Monitor::all()?
                .into_iter()
                .map(|m| {
                    let d = Display {
                        rect: RectPx::new(
                            m.x().unwrap_or(0),
                            m.y().unwrap_or(0),
                            m.width().unwrap_or(0),
                            m.height().unwrap_or(0),
                        ),
                        scale: m.scale_factor().unwrap_or(1.0),
                        name: m.name().unwrap_or_default(),
                    };
                    (d, m)
                })
                .collect();
            Ok(Self { monitors })
        }
    }

    pub fn displays(&self) -> Vec<Display> {
        #[cfg(windows)]
        return self.outputs.iter().map(|o| o.display.clone()).collect();
        #[cfg(not(windows))]
        return self.monitors.iter().map(|(d, _)| d.clone()).collect();
    }

    pub fn display_at(&self, p: PointPx) -> Option<Display> {
        let all = self.displays();
        all.iter().find(|d| d.rect.contains(p)).or(all.first()).cloned()
    }

    /// Frees the memory grabs read the screen through. The next grab pays
    /// about 1 ms to set it up again, so call this once a capture is over,
    /// not between the frames of a scrolling capture.
    pub fn trim(&mut self) {
        #[cfg(windows)]
        for o in &mut self.outputs {
            o.trim();
        }
    }

    pub fn grab(&mut self, display: &Display) -> Result<Frame> {
        #[cfg(windows)]
        let bitmap = {
            let output = self
                .outputs
                .iter_mut()
                .find(|o| o.display.rect == display.rect)
                .ok_or_else(|| anyhow!("display {} went away", display.name))?;
            match output.grab() {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("dxgi capture failed, using GDI: {e:#}");
                    let r = display.rect;
                    let img = xcap::Monitor::from_point(r.x + 1, r.y + 1)?.capture_image()?;
                    let (w, h) = img.dimensions();
                    sukurin_core::Bitmap::from_rgba(w, h, img.into_raw())
                }
            }
        };
        #[cfg(not(windows))]
        let bitmap = {
            let (_, m) = self
                .monitors
                .iter()
                .find(|(d, _)| d.rect == display.rect)
                .ok_or_else(|| anyhow!("display {} went away", display.name))?;
            let img = m.capture_image()?;
            let (w, h) = img.dimensions();
            Bitmap::from_rgba(w, h, img.into_raw())
        };
        Ok(Frame { display: display.clone(), bitmap, taken_at: SystemTime::now() })
    }
}

pub fn cursor_position() -> PointPx {
    #[cfg(windows)]
    unsafe {
        let mut p = windows::Win32::Foundation::POINT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
        PointPx::new(p.x, p.y)
    }
    #[cfg(not(windows))]
    PointPx::default()
}

/// Visible top-level windows, front to back.
pub fn snap_windows() -> Vec<SnapWindow> {
    #[cfg(windows)]
    return win_windows::visible_windows();
    #[cfg(not(windows))]
    return xcap::Window::all()
        .map(|ws| {
            ws.into_iter()
                .filter(|w| !w.is_minimized().unwrap_or(true))
                .map(|w| SnapWindow {
                    rect: RectPx::new(
                        w.x().unwrap_or(0),
                        w.y().unwrap_or(0),
                        w.width().unwrap_or(0),
                        w.height().unwrap_or(0),
                    ),
                    title: w.title().unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
}
