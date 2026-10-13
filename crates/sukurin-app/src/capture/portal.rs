//! Screenshots through the desktop portal, the one screen grab a Wayland
//! session lets an ordinary app make. GNOME answers a silent request only
//! with a stored permission ("Allow Apps to Take Screenshots?"); an
//! interactive one runs GNOME's own picker and needs no permission.

use anyhow::{Context as _, Result, anyhow};
use ashpd::desktop::ResponseError;
use ashpd::desktop::screenshot::Screenshot;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use sukurin_core::Bitmap;

/// One portal request. The future resolves when the portal answers, so a
/// permission dialog or the picker is waited out inside it: run it off the
/// main thread. `None` means the user cancelled.
pub async fn screenshot(interactive: bool) -> Result<Option<Bitmap>> {
    let asked = SystemTime::now();
    let request = Screenshot::request()
        .interactive(interactive)
        .modal(false)
        .send()
        .await
        .with_context(|| format!("portal screenshot (interactive {interactive})"))?;
    let shot = match request.response() {
        Ok(shot) => shot,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => return Ok(None),
        Err(e) => return Err(anyhow!("portal screenshot (interactive {interactive}): {e}")),
    };
    let path = file_path(shot.uri().as_str())?;
    let img = image::open(&path).with_context(|| format!("reading {}", path.display()))?.to_rgba8();
    // A silent request leaves GNOME's working file where the user keeps
    // pictures; it is ours only if it appeared for this request. What the
    // picker returns may be a screenshot GNOME saved for the user, so it stays.
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t + Duration::from_secs(2) >= asked);
    if interactive {
        log::debug!("picker screenshot left in place: {}", path.display());
    } else if fresh {
        match std::fs::remove_file(&path) {
            Ok(()) => log::debug!("removed portal screenshot {}", path.display()),
            Err(e) => log::warn!("could not remove portal screenshot {}: {e}", path.display()),
        }
    } else {
        log::info!("portal returned an older file, kept: {}", path.display());
    }
    let (w, h) = img.dimensions();
    Ok(Some(Bitmap::from_rgba(w, h, img.into_raw())))
}

fn file_path(uri: &str) -> Result<PathBuf> {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .ok_or_else(|| anyhow!("portal returned a screenshot that is not a local file: {uri}"))
}
