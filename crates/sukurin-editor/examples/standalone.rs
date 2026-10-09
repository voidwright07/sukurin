//! `cargo run -p sukurin-editor --example standalone -- <png> [scale] [warm]`
//!
//! Opens the editor on a PNG with a host that logs every action and writes
//! Copy and Save exports next to the build output. `warm` opens a second
//! editor a moment later, timing an open once the GPU is already up, as in
//! the resident app.

use gpui_kit::*;
use sukurin_core::{Bitmap, RectPx, Shot};
use sukurin_editor::{EditorAssets, EditorHost, open_editor};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

struct LogHost {
    out: PathBuf,
}

impl LogHost {
    fn write(&self, kind: &str, image: &Bitmap) {
        let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = self.out.join(format!("export-{kind}-{stamp}.png"));
        let rgba = image::RgbaImage::from_raw(image.width(), image.height(), image.to_rgba()).expect("bitmap size is consistent");
        match rgba.save(&path) {
            Ok(()) => log::info!("{kind}: {}x{} -> {}", image.width(), image.height(), path.display()),
            Err(e) => log::error!("{kind}: {e}"),
        }
    }
}

impl EditorHost for LogHost {
    fn copy(&self, image: &Bitmap, _: &mut App) {
        self.write("copy", image);
    }

    fn save(&self, image: &Bitmap, _: &mut App) {
        self.write("save", image);
    }

    fn pin(&self, shot: Shot, _: &mut App) {
        log::info!("pin: {}x{} @{}x", shot.bitmap.width(), shot.bitmap.height(), shot.scale);
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or_else(|| anyhow::anyhow!("usage: standalone <png> [scale]"))?;
    let scale: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.5);
    let warm = args.next().as_deref() == Some("warm");
    let img = image::open(&path)?.into_rgba8();
    let bitmap = Bitmap::from_rgba(img.width(), img.height(), img.into_raw());
    let shot = Shot {
        source_rect: RectPx::new(0, 0, bitmap.width(), bitmap.height()),
        bitmap,
        scale,
        taken_at: SystemTime::now(),
    };
    let out = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/agent-editor"));
    std::fs::create_dir_all(&out)?;
    let host = Rc::new(LogHost { out });
    gpui_kit::application().with_assets(EditorAssets).run(move |cx| {
        gpui_kit::init(cx);
        if let Err(e) = open_editor(shot.clone(), host.clone(), cx) {
            log::error!("{e:#}");
            cx.quit();
        }
        if warm {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                cx.update(|cx| open_editor(shot, host, cx)).ok();
            })
            .detach();
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
    Ok(())
}
