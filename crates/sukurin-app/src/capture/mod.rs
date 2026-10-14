//! Screen grabbing and window discovery. Everything here speaks physical
//! pixels on the virtual desktop.

#[cfg(windows)]
mod dxgi;
#[cfg(windows)]
mod hdr;
#[cfg(windows)]
mod win_windows;
#[cfg(target_os = "linux")]
mod display_config;
#[cfg(target_os = "linux")]
pub mod portal;

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
    /// On Wayland the layout GNOME reports, in portal screenshot pixels; the
    /// RandR rectangles behind `monitors` are Xwayland's integer-scaled view.
    #[cfg(target_os = "linux")]
    wayland: Vec<Display>,
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
            #[cfg(target_os = "linux")]
            let rects = randr_rects()?;
            let monitors = xcap::Monitor::all()?
                .into_iter()
                .map(|m| {
                    #[cfg(target_os = "linux")]
                    let (rect, scale) = (
                        m.id().ok().and_then(|id| rects.get(&id).copied()).unwrap_or_default(),
                        if wayland() { 1.0 } else { m.scale_factor().unwrap_or(1.0) },
                    );
                    #[cfg(not(target_os = "linux"))]
                    let (rect, scale) = (
                        RectPx::new(m.x().unwrap_or(0), m.y().unwrap_or(0), m.width().unwrap_or(0), m.height().unwrap_or(0)),
                        m.scale_factor().unwrap_or(1.0),
                    );
                    let name = m.name().unwrap_or_default();
                    log::debug!("display {name}: {rect:?} at scale {scale}");
                    (Display { rect, scale, name }, m)
                })
                .collect();
            #[cfg(target_os = "linux")]
            let mut this = Self { monitors, wayland: Vec::new() };
            #[cfg(not(target_os = "linux"))]
            let this = Self { monitors };
            #[cfg(target_os = "linux")]
            this.refresh_layout();
            Ok(this)
        }
    }

    /// Re-reads GNOME's monitor layout, which can change between captures.
    /// Falls back to the RandR rectangles when GNOME does not answer.
    #[cfg(target_os = "linux")]
    pub fn refresh_layout(&mut self) {
        if !wayland() {
            return;
        }
        match smol::block_on(display_config::displays()) {
            Ok(displays) => {
                for d in &displays {
                    log::debug!("GNOME display {}: {:?} at scale {}", d.name, d.rect, d.scale);
                }
                self.wayland = displays;
            }
            Err(e) => log::warn!("monitor layout from GNOME unavailable, using RandR: {e:#}"),
        }
    }

    pub fn displays(&self) -> Vec<Display> {
        #[cfg(windows)]
        return self.outputs.iter().map(|o| o.display.clone()).collect();
        #[cfg(target_os = "linux")]
        if !self.wayland.is_empty() {
            return self.wayland.clone();
        }
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
            // On Wayland the caller has already taken one silent portal grab
            // to open the overlay, so the permission is stored and this
            // does not wait on a dialog.
            #[cfg(target_os = "linux")]
            if wayland() {
                let desktop = smol::block_on(portal::screenshot(false))?.ok_or_else(|| anyhow!("screenshot cancelled"))?;
                return Ok(Frame { display: display.clone(), bitmap: display_part(desktop, display)?, taken_at: SystemTime::now() });
            }
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

/// The display's part of a whole-desktop portal screenshot.
#[cfg(target_os = "linux")]
pub fn display_part(desktop: Bitmap, display: &Display) -> Result<Bitmap> {
    desktop.crop(display.rect).ok_or_else(|| {
        anyhow!("display {} at {:?} is outside the {}x{} portal screenshot", display.name, display.rect, desktop.width(), desktop.height())
    })
}

/// Monitor rectangles in physical pixels, keyed by RandR output id. xcap's
/// own position and size accessors divide by a scale factor they read through
/// libwayshot, which aborts on compositors without the wlroots protocols
/// (GNOME, Weston). xcap crops captures with this same raw RandR data, so the
/// geometry and the pixels agree.
#[cfg(target_os = "linux")]
fn randr_rects() -> Result<std::collections::HashMap<u32, RectPx>> {
    use xcb::{Xid, randr};
    let (conn, screen) = xcb::Connection::connect_with_extensions(None, &[xcb::Extension::RandR], &[])?;
    let root = conn.get_setup().roots().nth(screen as usize).ok_or_else(|| anyhow!("no X screen"))?.root();
    let reply = conn.wait_for_reply(conn.send_request(&randr::GetMonitors { window: root, get_active: true }))?;
    let mut rects = std::collections::HashMap::new();
    for m in reply.monitors() {
        let rect = RectPx::new(m.x() as i32, m.y() as i32, m.width() as u32, m.height() as u32);
        for output in m.outputs() {
            rects.insert(output.resource_id(), rect);
        }
    }
    Ok(rects)
}

/// xcap's own test for a Wayland session, which decides whether its scale
/// factor lookup goes through libwayshot. Global shortcuts are X11 key grabs,
/// so they do not fire in such a session either.
#[cfg(target_os = "linux")]
pub fn wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
        || std::env::var("WAYLAND_DISPLAY").is_ok_and(|d| d.to_lowercase().contains("wayland"))
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
