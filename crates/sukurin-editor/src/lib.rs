//! The capture editor window: canvas with zoom and pan, tool strip,
//! contextual properties bar, status bar, inspector dock and command palette.
//! Everything drawn on the canvas comes from `sukurin_annotate`'s scene, the
//! same one its `export` rasterises.

mod color;
mod editor;
mod geom;
mod icons;
mod inspector;
mod keys;
mod paint;
mod palette;
mod tools;
mod ui;
mod viewport;
mod widgets;

pub use icons::EditorAssets;

use anyhow::{Context as _, Result};
use editor::Editor;
use gpui_kit::*;
use sukurin_core::{Bitmap, Shot};
use std::rc::Rc;
use std::time::Instant;

/// What the editor asks of the app around it.
pub trait EditorHost: 'static {
    fn copy(&self, image: &Bitmap, cx: &mut App);
    /// The app picks the path and shows its own feedback.
    fn save(&self, image: &Bitmap, cx: &mut App);
    fn pin(&self, shot: Shot, cx: &mut App);
    /// Shows the screenshots already on disk. Hosts without a library ignore it.
    fn open_library(&self, _cx: &mut App) {}
}

const DEFAULT_W: f32 = 1440.;
const DEFAULT_H: f32 = 900.;

/// Opens an editor window on `shot`, centred on the display it came from.
pub fn open_editor(shot: Shot, host: Rc<dyn EditorHost>, cx: &mut App) -> Result<AnyWindowHandle> {
    let t = Instant::now();
    let display = display_for(&shot, cx);
    let visible = display.as_ref().map(|d| d.visible_bounds()).unwrap_or(Bounds::new(point(px(0.), px(0.)), size(px(DEFAULT_W), px(DEFAULT_H))));
    let w = px(DEFAULT_W).min(visible.size.width - px(48.)).max(px(640.));
    let h = px(DEFAULT_H).min(visible.size.height - px(48.)).max(px(420.));
    let bounds = Bounds::new(
        point(visible.origin.x + (visible.size.width - w) / 2., visible.origin.y + (visible.size.height - h) / 2.),
        size(w, h),
    );
    let title: SharedString = editor::capture_name(&shot).into();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(title),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(13.))),
        }),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        app_owns_titlebar_drag: true,
        display_id: display.as_ref().map(|d| d.id()),
        window_min_size: Some(size(px(720.), px(440.))),
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    };
    let handle = cx
        .open_window(options, |window, cx| {
            let editor = cx.new(|cx| Editor::new(shot, host, window, cx));
            let focus = editor.read(cx).focus.clone();
            window.focus(&focus, cx);
            window.activate_window();
            window.on_next_frame(move |_, _| {
                log::info!("editor first frame {:.1} ms after open", t.elapsed().as_secs_f64() * 1000.);
            });
            editor
        })
        .context("opening the editor window")?;
    log::info!("editor window created {:.1} ms after open", t.elapsed().as_secs_f64() * 1000.);
    Ok(handle.into())
}

/// The display containing the capture, matched in logical coordinates.
fn display_for(shot: &Shot, cx: &App) -> Option<Rc<dyn PlatformDisplay>> {
    let s = shot.scale.max(0.5);
    let c = point(
        px((shot.source_rect.x as f32 + shot.source_rect.w as f32 / 2.) / s),
        px((shot.source_rect.y as f32 + shot.source_rect.h as f32 / 2.) / s),
    );
    cx.displays().into_iter().find(|d| d.bounds().contains(&c)).or_else(|| cx.primary_display())
}
