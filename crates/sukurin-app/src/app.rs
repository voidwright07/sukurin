//! The resident controller: owns the capture backend and the windows that a
//! capture opens, and turns tray/hotkey commands into captures.

use crate::capture::{self, Capturer};
use crate::countdown::{self, CancelCountdown, Countdown};
use crate::overlay::{After, Mode, Overlay, OverlayEvent};
use crate::pin::Pin;
use crate::library_window::{LibraryAction, LibraryWindow};
use crate::recognize;
use crate::scroll::{self, HudAction, ScrollHud};
use crate::settings::Settings;
use crate::share;
use crate::thumbnail::{self, Status, ThumbAction, Thumbnail};
use crate::toast::{self, Content, Toast, ToastAction};
use anyhow::{Context as _, Result};
use gpui_kit::*;
use sukurin_core::stitch::{Push, Stitcher};
use sukurin_editor::EditorHost;
use sukurin_library::Library;
use sukurin_core::{Bitmap, Display, Frame, RectPx, Shot};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Capture(Mode),
    OpenLibrary,
    OpenFolder,
    OpenSettings,
    OpenOnboarding,
    ToggleAutostart,
    Quit,
}

/// The one card that lives in the screen corner after a capture.
#[derive(Clone)]
enum Card {
    Thumbnail(Entity<Thumbnail>),
    Toast(Entity<Toast>),
}

impl Card {
    fn id(&self) -> EntityId {
        match self {
            Card::Thumbnail(e) => e.entity_id(),
            Card::Toast(e) => e.entity_id(),
        }
    }

    fn should_close(&self, cx: &App) -> bool {
        match self {
            Card::Thumbnail(e) => e.read(cx).should_close(),
            Card::Toast(e) => e.read(cx).should_close(),
        }
    }
}

pub struct Sukurin {
    capturer: Capturer,
    last_area: Option<RectPx>,
    overlay: Option<(AnyWindowHandle, Entity<Overlay>)>,
    corner: Option<(AnyWindowHandle, Card)>,
    scrolling: Option<Scrolling>,
    library: Option<Library>,
    library_window: Option<(AnyWindowHandle, Entity<LibraryWindow>)>,
    settings_window: Option<AnyWindowHandle>,
    onboarding_window: Option<AnyWindowHandle>,
}

/// A scrolling capture in progress.
struct Scrolling {
    hud_window: AnyWindowHandle,
    hud: Entity<ScrollHud>,
    region: RectPx,
    display: Display,
    stitcher: Stitcher,
    unchanged: u32,
    end: Option<HudAction>,
    taken_at: std::time::SystemTime,
}

impl Global for Sukurin {}

pub fn install(cx: &mut App) -> Result<()> {
    let capturer = Capturer::new().context("starting screen capture")?;
    cx.set_global(Sukurin { capturer, last_area: None, overlay: None, corner: None, scrolling: None, library: open_library(), library_window: None, settings_window: None, onboarding_window: None });
    Ok(())
}

/// The index of screenshots already on disk. Sukurin keeps working without
/// it, so a broken database only costs the duplicate hints and the library.
fn open_library() -> Option<Library> {
    let dir = dirs::data_local_dir()?.join("Sukurin");
    let _ = std::fs::create_dir_all(save_dir());
    let opened = Library::open(&dir.join("library.sqlite"), &dir.join("thumbs"), sukurin_library::default_roots());
    match opened {
        Ok(lib) => {
            lib.start();
            Some(lib)
        }
        Err(e) => {
            log::error!("screenshot library unavailable: {e:#}");
            None
        }
    }
}

pub fn library(cx: &App) -> Option<Library> {
    cx.global::<Sukurin>().library.clone()
}

/// Looks the capture up among existing screenshots and, if one matches,
/// tells the thumbnail so the user can open it instead of keeping another.
/// Looks for an earlier copy of the capture, then saves it when `auto_save`
/// is on. Saving waits for the lookup so the new file can't match itself.
fn check_duplicate(thumb: &Entity<Thumbnail>, auto_save: bool, cx: &mut App) {
    let Some(lib) = library(cx) else {
        if auto_save {
            on_thumb_action(thumb.clone(), ThumbAction::Save, cx);
        }
        return;
    };
    let bitmap = thumb.read(cx).shot.bitmap.clone();
    let weak = thumb.downgrade();
    cx.spawn(async move |cx| {
        let t = Instant::now();
        let hits = cx.background_executor().spawn(async move { lib.similar(&bitmap, 1) }).await;
        let Some(thumb) = weak.upgrade() else { return };
        cx.update(|cx| {
            if let Some(hit) = hits.into_iter().next() {
                log::info!("duplicate of {} (exact {}) found in {:.1} ms", hit.entry.path.display(), hit.exact, ms(t.elapsed()));
                thumb.update(cx, |t, cx| t.set_duplicate(Some(thumbnail::Duplicate { path: hit.entry.path, exact: hit.exact }), cx));
            }
            if auto_save {
                on_thumb_action(thumb, ThumbAction::Save, cx);
            }
        });
    })
    .detach();
}

fn open_library_window(cx: &mut App) {
    if let Some((handle, view)) = cx.global::<Sukurin>().library_window.clone()
        && handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                view.update(cx, |v, cx| v.focus_search(window, cx));
            })
            .is_ok()
    {
        return;
    }
    let Some(lib) = library(cx) else {
        log::error!("library unavailable");
        return;
    };
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1180.), px(780.)), cx)),
        titlebar: Some(TitlebarOptions { title: Some("Sukurin Library".into()), ..Default::default() }),
        focus: true,
        show: true,
        window_min_size: Some(size(px(520.), px(360.))),
        ..Default::default()
    };
    let opened = open_window(opts, cx, |window, cx| {
        let view = cx.new(|cx| LibraryWindow::new(lib, window, cx));
        view.update(cx, |v, cx| v.focus_search(window, cx));
        view
    });
    match opened {
        Ok((handle, view)) => {
            cx.subscribe(&view, |_, a: &LibraryAction, cx| match a {
                LibraryAction::Reveal(p) => reveal(p),
                LibraryAction::Edit(p) => match load_shot(p) {
                    Ok(shot) => edit(shot, cx),
                    Err(e) => log::error!("could not open {}: {e:#}", p.display()),
                },
                LibraryAction::Copy(p) => match load_shot(p) {
                    Ok(shot) => copy_to_clipboard(&shot),
                    Err(e) => log::error!("could not copy {}: {e:#}", p.display()),
                },
                LibraryAction::Pin(p) => match load_shot(p) {
                    Ok(shot) => open_pin(shot, cx),
                    Err(e) => log::error!("could not pin {}: {e:#}", p.display()),
                },
            })
            .detach();
            cx.global_mut::<Sukurin>().library_window = Some((handle, view));
        }
        Err(e) => log::error!("could not open library: {e:#}"),
    }
}

/// A saved screenshot as a `Shot`, placed at the primary display's origin.
fn load_shot(path: &Path) -> Result<Shot> {
    let img = image::open(path)?.to_rgba8();
    let (w, h) = img.dimensions();
    Ok(Shot {
        bitmap: Bitmap::from_rgba(w, h, img.into_raw()),
        scale: 1.0,
        source_rect: RectPx::new(80, 80, w, h),
        taken_at: std::time::SystemTime::now(),
    })
}

pub fn run(cmd: Command, cx: &mut App) {
    match cmd {
        Command::OpenLibrary => open_library_window(cx),
        Command::OpenSettings => open_settings_window(cx),
        Command::OpenOnboarding => open_onboarding_window(cx),
        Command::Capture(mode) => {
            if let Err(e) = begin_capture(mode, cx) {
                log::error!("capture failed: {e:#}");
            }
        }
        Command::OpenFolder => {
            let dir = save_dir();
            let _ = std::fs::create_dir_all(&dir);
            reveal(&dir);
        }
        Command::ToggleAutostart => {
            let enable = !crate::autostart::is_enabled();
            match crate::autostart::set(enable) {
                Ok(()) => log::info!("start with Windows: {enable}"),
                Err(e) => log::error!("could not change start-up setting: {e:#}"),
            }
        }
        Command::Quit => cx.quit(),
    }
}

fn begin_capture(mode: Mode, cx: &mut App) -> Result<()> {
    if cx.global::<Sukurin>().overlay.is_some() || cx.global::<Sukurin>().scrolling.is_some() {
        return Ok(());
    }
    let t = Instant::now();
    let cursor = capture::cursor_position();
    let (frame, last_area) = cx.update_global(|s: &mut Sukurin, _| -> Result<(Frame, Option<RectPx>)> {
        let d = s.capturer.display_at(cursor).context("no display")?;
        Ok((s.capturer.grab(&d)?, s.last_area))
    })?;
    let grabbed = t.elapsed();
    let windows = capture::snap_windows();
    let (bounds, display_id) = gpui_display(&frame.display, cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: true,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id,
        ..Default::default()
    };
    let (handle, overlay) = open_window(opts, cx, |window, cx| {
        let overlay = cx.new(|cx| Overlay::new(frame, windows, last_area, cursor, mode, cx));
        let focus = overlay.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        window.activate_window();
        window.on_next_frame(move |_, cx| {
            log::info!("hotkey to overlay: grab {:.1} ms, first frame {:.1} ms", ms(grabbed), ms(t.elapsed()));
            cx.global_mut::<Sukurin>().capturer.trim();
        });
        overlay
    })?;
    cx.subscribe(&overlay, |overlay, ev: &OverlayEvent, cx| on_overlay(overlay, ev, cx)).detach();
    cx.global_mut::<Sukurin>().overlay = Some((handle, overlay));
    Ok(())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.
}

/// GPUI's view of the display we grabbed, matched by physical origin.
fn gpui_display(d: &Display, cx: &App) -> (Bounds<Pixels>, Option<DisplayId>) {
    let found = cx.displays().into_iter().find(|g| {
        let o = g.bounds().origin;
        (o.x.as_f32() * d.scale).round() as i32 == d.rect.x && (o.y.as_f32() * d.scale).round() as i32 == d.rect.y
    });
    match found {
        Some(g) => (g.bounds(), Some(g.id())),
        None => (
            Bounds::new(
                point(px(d.rect.x as f32 / d.scale), px(d.rect.y as f32 / d.scale)),
                size(px(d.rect.w as f32 / d.scale), px(d.rect.h as f32 / d.scale)),
            ),
            None,
        ),
    }
}

/// Removal is deferred: this runs from the overlay's own event, and the
/// overlay still finishes its key or mouse handler afterwards.
fn close_overlay(cx: &mut App) {
    if let Some((handle, _)) = cx.global_mut::<Sukurin>().overlay.take() {
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
    }
}

fn on_overlay(overlay: Entity<Overlay>, ev: &OverlayEvent, cx: &mut App) {
    match ev {
        OverlayEvent::Cancel => close_overlay(cx),
        OverlayEvent::Delay => {
            let display = overlay.read(cx).frame().display.clone();
            close_overlay(cx);
            delayed_capture(display, cx);
        }
        OverlayEvent::Capture { rect, after } => {
            let frame = overlay.read(cx).frame().clone();
            close_overlay(cx);
            let Some(bitmap) = frame.bitmap.crop(*rect) else { return };
            let origin = frame.display.rect;
            let shot = Shot {
                bitmap,
                scale: frame.display.scale,
                source_rect: rect.translate(origin.x, origin.y),
                taken_at: frame.taken_at,
            };
            match after {
                After::Recognize => recognize_shot(shot, frame.display, cx),
                After::Scroll => start_scrolling(shot, frame.display, cx),
                After::Thumbnail => {
                    cx.global_mut::<Sukurin>().last_area = Some(shot.source_rect);
                    copy_to_clipboard(&shot);
                    show_thumbnail(shot, &frame.display, cx);
                }
                After::Edit => {
                    cx.global_mut::<Sukurin>().last_area = Some(shot.source_rect);
                    copy_to_clipboard(&shot);
                    edit(shot, cx);
                }
            }
        }
    }
}

/// What the editor can ask of the app: the same clipboard, save folder,
/// library and pins as every other capture path.
struct Host {
    display: Display,
}

impl EditorHost for Host {
    fn copy(&self, image: &Bitmap, _: &mut App) {
        copy_bitmap(image.clone());
    }

    fn save(&self, image: &Bitmap, cx: &mut App) {
        let (image, lib, display) = (image.clone(), library(cx), self.display.clone());
        cx.spawn(async move |cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    let path = save_bitmap(&image, std::time::SystemTime::now())?;
                    if let Some(lib) = lib
                        && let Err(e) = lib.record(&path, &image)
                    {
                        log::warn!("could not index {}: {e:#}", path.display());
                    }
                    Ok::<_, anyhow::Error>(path)
                })
                .await;
            cx.update(|cx| match saved {
                Ok(path) => {
                    log::info!("saved {}", path.display());
                    show_toast(Content::Saved { path }, &display, cx);
                }
                Err(e) => show_toast(Content::Nothing { reason: format!("Save failed: {e:#}") }, &display, cx),
            });
        })
        .detach();
    }

    fn pin(&self, shot: Shot, cx: &mut App) {
        open_pin(shot, cx);
    }

    fn open_library(&self, cx: &mut App) {
        open_library_window(cx);
    }
}

fn edit(shot: Shot, cx: &mut App) {
    let display = cx
        .global::<Sukurin>()
        .capturer
        .display_at(sukurin_core::PointPx::new(shot.source_rect.x, shot.source_rect.y))
        .unwrap_or(Display { rect: shot.source_rect, scale: shot.scale, name: String::new() });
    let t = Instant::now();
    match sukurin_editor::open_editor(shot, std::rc::Rc::new(Host { display }), cx) {
        Ok(_) => log::info!("editor opened in {:.1} ms", ms(t.elapsed())),
        Err(e) => log::error!("could not open editor: {e:#}"),
    }
}

const DELAY_SECONDS: u32 = 3;

fn delayed_capture(display: Display, cx: &mut App) {
    let (bounds, display_id) = gpui_display(&display, cx);
    let s = countdown::SIZE;
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(bounds.center().x - px(s / 2.), bounds.center().y - px(s / 2.)),
            size(px(s), px(s)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: false,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let Ok((handle, view)) = open_window(opts, cx, |_, cx| cx.new(|_| Countdown { remaining: DELAY_SECONDS })) else {
        return;
    };
    let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = cancelled.clone();
    cx.subscribe(&view, move |_, _: &CancelCountdown, _| flag.set(true)).detach();
    cx.spawn(async move |cx| {
        for left in (1..DELAY_SECONDS).rev() {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if cancelled.get() {
                break;
            }
            cx.update(|cx| view.update(cx, |c, cx| {
                c.remaining = left;
                cx.notify();
            }));
        }
        cx.background_executor().timer(Duration::from_secs(1)).await;
        cx.update(|cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
        if cancelled.get() {
            return;
        }
        // Let the compositor drop the countdown before the screen is grabbed.
        cx.background_executor().timer(Duration::from_millis(80)).await;
        cx.update(|cx| run(Command::Capture(Mode::Area), cx));
    })
    .detach();
}

pub fn copy_to_clipboard(shot: &Shot) {
    copy_bitmap(shot.bitmap.clone());
}

fn copy_bitmap(b: Bitmap) {
    std::thread::spawn(move || {
        let data = arboard::ImageData { width: b.width() as usize, height: b.height() as usize, bytes: b.to_rgba().into() };
        if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_image(data)) {
            log::error!("clipboard: {e}");
        }
    });
}

fn copy_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// Opens a card in the bottom-right corner of `display`, replacing any card
/// already there, and dismisses it once it says it is done.
fn open_card<V: Render>(
    display: &Display,
    w: f32,
    h: f32,
    cx: &mut App,
    build: impl FnOnce(&mut App) -> Entity<V>,
    wrap: impl FnOnce(Entity<V>) -> Card,
) -> Option<(Entity<V>, Point<Pixels>)> {
    if let Some((handle, _)) = cx.global_mut::<Sukurin>().corner.take() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
    let (fallback, display_id) = gpui_display(display, cx);
    let visible = display_id.and_then(|id| cx.find_display(id)).map(|d| d.visible_bounds()).unwrap_or(fallback);
    let origin = point(visible.right() - px(w + 16.), visible.bottom() - px(h + 16.));
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size(px(w), px(h))))),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: false,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let (handle, entity) = match open_window(opts, cx, |_, cx| build(cx)) {
        Ok(opened) => opened,
        Err(e) => {
            log::error!("could not open card: {e:#}");
            return None;
        }
    };
    let card = wrap(entity.clone());
    cx.global_mut::<Sukurin>().corner = Some((handle, card.clone()));
    let id = card.id();
    cx.spawn(async move |cx| loop {
        cx.background_executor().timer(Duration::from_millis(250)).await;
        let still_here = cx.update(|cx| {
            let current = cx.global::<Sukurin>().corner.as_ref().filter(|(_, c)| c.id() == id).map(|(_, c)| c.clone());
            match current {
                Some(c) if c.should_close(cx) => {
                    close_card(id, cx);
                    false
                }
                Some(_) => true,
                None => false,
            }
        });
        if !still_here {
            break;
        }
    })
    .detach();
    Some((entity, origin))
}

fn close_card(id: EntityId, cx: &mut App) {
    let current = cx.global::<Sukurin>().corner.as_ref().is_some_and(|(_, c)| c.id() == id);
    if current && let Some((handle, _)) = cx.global_mut::<Sukurin>().corner.take() {
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
    }
}

fn show_thumbnail(shot: Shot, display: &Display, cx: &mut App) {
    let (w, h) = (thumbnail::WIDTH, Thumbnail::window_height(&shot));
    let preview_h = Thumbnail::preview_height(&shot);
    let Some((thumb, origin)) = open_card(display, w, h, cx, |cx| cx.new(|_| Thumbnail::new(shot)), Card::Thumbnail) else {
        return;
    };
    log::info!(
        "thumbnail at {},{} {}x{} px, actions row y {}",
        (origin.x.as_f32() * display.scale).round(),
        (origin.y.as_f32() * display.scale).round(),
        (w * display.scale).round(),
        (h * display.scale).round(),
        ((origin.y.as_f32() + preview_h + 22.) * display.scale).round()
    );
    cx.subscribe(&thumb, |thumb, a: &ThumbAction, cx| on_thumb_action(thumb, a.clone(), cx)).detach();
    check_duplicate(&thumb, Settings::load().auto_save, cx);
}

fn recognize_shot(shot: Shot, display: Display, cx: &mut App) {
    cx.spawn(async move |cx| {
        let t = Instant::now();
        let bitmap = shot.bitmap.clone();
        let result = cx.background_executor().spawn(async move { recognize::recognize(&bitmap) }).await;
        let millis = t.elapsed().as_millis();
        cx.update(|cx| {
            let content = match result {
                Ok(r) if !r.codes.is_empty() => Content::Code { text: r.codes[0].clone() },
                Ok(r) if r.lines.iter().any(|l| !l.trim().is_empty()) => {
                    Content::Text { lines: r.lines, language: r.language, millis }
                }
                Ok(_) => Content::Nothing { reason: format!("{} × {} px", shot.bitmap.width(), shot.bitmap.height()) },
                Err(e) => Content::Nothing { reason: format!("{e:#}") },
            };
            log::info!("recognised in {millis} ms: {}", match &content {
                Content::Text { lines, .. } => format!("{} lines", lines.len()),
                Content::Code { text } => format!("code {text}"),
                Content::Nothing { reason } => format!("nothing ({reason})"),
                Content::Saved { path } => format!("saved {}", path.display()),
            });
            let text = Toast::new(content.clone()).clipboard_text();
            if !text.is_empty() {
                copy_text(text, cx);
            }
            show_toast(content, &display, cx);
        });
    })
    .detach();
}

/// Wheel notches per step. Small enough that consecutive frames overlap by
/// most of the region, which is what makes alignment reliable.
const SCROLL_NOTCHES: i32 = 2;
const SETTLE: Duration = Duration::from_millis(120);
/// Consecutive unchanged frames that mean the page has ended.
const END_AFTER: u32 = 3;
/// A frame is used only once two grabs this far apart agree, so a page
/// caught mid-repaint is never stitched.
const STABLE_GAP: Duration = Duration::from_millis(40);
const STABLE_TRIES: usize = 4;

fn start_scrolling(first: Shot, display: Display, cx: &mut App) {
    let region = first.source_rect;
    let s = display.scale;
    let (_, display_id) = gpui_display(&display, cx);
    let screen = Bounds::new(
        point(px(display.rect.x as f32 / s), px(display.rect.y as f32 / s)),
        size(px(display.rect.w as f32 / s), px(display.rect.h as f32 / s)),
    );
    let (rx, ry, rr) = (region.x as f32 / s, region.y as f32 / s, region.right() as f32 / s);
    let x = if rr + 12. + scroll::WIDTH <= screen.right().as_f32() {
        rr + 12.
    } else if rx - 12. - scroll::WIDTH >= screen.left().as_f32() {
        rx - 12. - scroll::WIDTH
    } else {
        screen.right().as_f32() - scroll::WIDTH - 12.
    };
    let y = ry.clamp(screen.top().as_f32() + 12., screen.bottom().as_f32() - scroll::HEIGHT - 12.);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(x), px(y)), size(px(scroll::WIDTH), px(scroll::HEIGHT))))),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: true,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let opened = open_window(opts, cx, |window, cx| {
        let hud = cx.new(|cx| ScrollHud::new(s, cx));
        let focus = hud.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        hud
    });
    let (hud_window, hud) = match opened {
        Ok(o) => o,
        Err(e) => {
            log::error!("could not open scrolling HUD: {e:#}");
            return;
        }
    };
    cx.subscribe(&hud, |_, a: &HudAction, cx| {
        let Some(session) = cx.global_mut::<Sukurin>().scrolling.as_mut() else { return };
        match a {
            HudAction::TogglePause => {
                let hud = session.hud.clone();
                hud.update(cx, |h, cx| {
                    h.paused = !h.paused;
                    cx.notify();
                });
            }
            end => session.end = Some(*end),
        }
    })
    .detach();

    let mut stitcher = Stitcher::new();
    stitcher.push(&first.bitmap);
    hud.update(cx, |h, _| {
        h.height_px = stitcher.height();
        h.frames = stitcher.frames();
    });
    scroll::move_cursor(region.x + region.w as i32 / 2, region.y + region.h as i32 / 2);
    cx.global_mut::<Sukurin>().scrolling = Some(Scrolling {
        hud_window,
        hud,
        region,
        display,
        stitcher,
        unchanged: 0,
        end: None,
        taken_at: first.taken_at,
    });
    log::info!("scrolling capture started");

    cx.spawn(async move |cx| loop {
        let paused = cx.update(|cx| {
            let s = cx.global::<Sukurin>().scrolling.as_ref()?;
            Some(s.hud.read(cx).paused)
        });
        match paused {
            None => break,
            Some(false) => scroll::wheel_down(SCROLL_NOTCHES),
            Some(true) => {}
        }
        cx.background_executor().timer(SETTLE).await;
        let mut frame = cx.update(grab_scroll_region);
        for _ in 0..STABLE_TRIES {
            let Some(Ok(prev)) = &frame else { break };
            cx.background_executor().timer(STABLE_GAP).await;
            let next = cx.update(grab_scroll_region);
            let settled = matches!(&next, Some(Ok(b)) if b == prev);
            frame = next;
            if settled {
                break;
            }
        }
        if !cx.update(|cx| stitch_frame(frame, cx)) {
            break;
        }
    })
    .detach();
}

/// `None` once the session has ended.
fn grab_scroll_region(cx: &mut App) -> Option<Result<Bitmap>> {
    cx.update_global(|s: &mut Sukurin, _| {
        let session = s.scrolling.as_ref()?;
        if session.end.is_some() {
            return None;
        }
        let (display, region) = (session.display.clone(), session.region);
        Some(s.capturer.grab(&display).and_then(|f| {
            f.bitmap
                .crop(region.translate(-display.rect.x, -display.rect.y))
                .context("scroll region left the display")
        }))
    })
}

/// Stitches one settled frame. Returns false once the session is over.
fn stitch_frame(grabbed: Option<Result<Bitmap>>, cx: &mut App) -> bool {
    if let Some(grabbed) = grabbed {
        let bitmap = match grabbed {
            Ok(b) => b,
            Err(e) => {
                log::error!("scrolling grab failed: {e:#}");
                finish_scrolling(Some(HudAction::Stop), cx);
                return false;
            }
        };
        if let Ok(dir) = std::env::var("SUKURIN_DUMP_SCROLL") {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let _ = image::save_buffer(
                format!("{dir}/frame-{n}.png"),
                &bitmap.to_rgba(),
                bitmap.width(),
                bitmap.height(),
                image::ExtendedColorType::Rgba8,
            );
        }
        let session = cx.global_mut::<Sukurin>().scrolling.as_mut().unwrap();
        let pushed = session.stitcher.push(&bitmap);
        match pushed {
            Push::Unchanged => session.unchanged += 1,
            Push::Added(_) | Push::Lost => session.unchanged = 0,
        }
        let (height, frames, ended) = (session.stitcher.height(), session.stitcher.frames(), session.unchanged >= END_AFTER);
        let hud = session.hud.clone();
        let paused = hud.read(cx).paused;
        let at_limit = hud.update(cx, |h, cx| {
            h.height_px = height;
            h.frames = frames;
            if pushed == Push::Lost {
                h.lost += 1;
            }
            cx.notify();
            height >= h.max_px()
        });
        if at_limit || (ended && !paused) {
            finish_scrolling(Some(HudAction::Stop), cx);
            return false;
        }
        return true;
    }
    let end = cx.global::<Sukurin>().scrolling.as_ref().and_then(|s| s.end);
    finish_scrolling(end, cx);
    false
}

fn finish_scrolling(end: Option<HudAction>, cx: &mut App) {
    let Some(session) = cx.global_mut::<Sukurin>().scrolling.take() else { return };
    cx.global_mut::<Sukurin>().capturer.trim();
    let handle = session.hud_window;
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    });
    if end == Some(HudAction::Cancel) {
        log::info!("scrolling capture cancelled");
        return;
    }
    let Some(mut bitmap) = session.stitcher.finish() else { return };
    let max = (scroll::MAX_POINTS * session.display.scale) as u32;
    if bitmap.height() > max {
        bitmap = bitmap.crop(RectPx::new(0, 0, bitmap.width(), max)).unwrap_or(bitmap);
    }
    log::info!("scrolling capture finished: {}x{} px from {} frames", bitmap.width(), bitmap.height(), session.stitcher.frames());
    let shot = Shot {
        source_rect: RectPx::new(session.region.x, session.region.y, bitmap.width(), bitmap.height()),
        bitmap,
        scale: session.display.scale,
        taken_at: session.taken_at,
    };
    copy_to_clipboard(&shot);
    show_thumbnail(shot, &session.display, cx);
}

fn on_thumb_action(thumb: Entity<Thumbnail>, a: ThumbAction, cx: &mut App) {
    let shot = thumb.read(cx).shot.clone();
    match a {
        ThumbAction::Copy => {
            copy_to_clipboard(&shot);
            thumb.update(cx, |t, cx| t.set_status(Status::Copied, cx));
        }
        ThumbAction::Save => {
            let weak = thumb.downgrade();
            let lib = library(cx);
            cx.spawn(async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let path = save_png(&shot)?;
                        if let Some(lib) = lib
                            && let Err(e) = lib.record(&path, &shot.bitmap)
                        {
                            log::warn!("could not index {}: {e:#}", path.display());
                        }
                        Ok::<_, anyhow::Error>(path)
                    })
                    .await;
                if let Some(thumb) = weak.upgrade() {
                    let status = match result {
                        Ok(path) => {
                            log::info!("saved {}", path.display());
                            Status::Saved(path)
                        }
                        Err(e) => Status::Failed(format!("Save failed: {e}").into()),
                    };
                    cx.update(|cx| thumb.update(cx, |t, cx| t.set_status(status, cx)));
                }
            })
            .detach();
        }
        ThumbAction::Pin => {
            close_card(thumb.entity_id(), cx);
            open_pin(shot, cx);
        }
        ThumbAction::Edit => {
            close_card(thumb.entity_id(), cx);
            edit(shot, cx);
        }
        ThumbAction::SendToAi => {
            let weak = thumb.downgrade();
            cx.spawn(async move |cx| {
                let copied = cx
                    .background_executor()
                    .spawn(async move {
                        let lines = recognize::recognize(&shot.bitmap).map(|r| r.lines).unwrap_or_default();
                        share::copy_image_and_text(&shot.bitmap, &share::ai_context(&shot, &lines))
                    })
                    .await;
                let status = match copied {
                    Ok(()) => {
                        open_url(&share::ai_url(Settings::load().ai_chat));
                        Status::CopiedForAi
                    }
                    Err(e) => Status::Failed(format!("Copy for AI failed: {e:#}").into()),
                };
                if let Some(thumb) = weak.upgrade() {
                    cx.update(|cx| thumb.update(cx, |t, cx| t.set_status(status, cx)));
                }
            })
            .detach();
        }
        ThumbAction::DragOut => {
            // The OS drag loop pumps messages, so GPUI timers and paints run
            // inside it. It must start from a task, never from a callback that
            // holds the app borrowed.
            let weak = thumb.downgrade();
            cx.spawn(async move |cx| {
                let dropped = share::temp_png(&shot).and_then(|p| share::drag_file(&p));
                if let Err(e) = &dropped {
                    log::error!("drag out failed: {e:#}");
                }
                if let (Ok(()), Some(thumb)) = (dropped, weak.upgrade()) {
                    log::info!("dragged out");
                    cx.update(|cx| thumb.update(cx, |t, cx| t.set_status(Status::Dropped, cx)));
                }
            })
            .detach();
        }
        ThumbAction::OpenExisting(path) => reveal(&path),
        ThumbAction::Dismiss => close_card(thumb.entity_id(), cx),
    }
}

fn open_pin(shot: Shot, cx: &mut App) {
    let natural = Pin::natural_size(&shot);
    let origin = point(
        px(shot.source_rect.x as f32 / shot.scale),
        px(shot.source_rect.y as f32 / shot.scale),
    );
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, natural))),
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: true,
        show: true,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let opened = open_window(opts, cx, |window, cx| {
        let pin = cx.new(|cx| Pin::new(shot, cx));
        let focus = pin.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        pin
    });
    match opened {
        Ok(_) => log::info!("pinned"),
        Err(e) => log::error!("could not pin: {e:#}"),
    }
}

pub fn save_dir() -> PathBuf {
    dirs::picture_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default()).join("Sukurin")
}

fn save_png(shot: &Shot) -> Result<PathBuf> {
    save_bitmap(&shot.bitmap, shot.taken_at)
}

fn save_bitmap(bitmap: &Bitmap, taken_at: std::time::SystemTime) -> Result<PathBuf> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let dir = save_dir();
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::DateTime::<chrono::Local>::from(taken_at).format("%Y-%m-%d at %H.%M.%S");
    let mut path = dir.join(format!("Sukurin {stamp}.png"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("Sukurin {stamp} ({n}).png"));
        n += 1;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
    PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::Adaptive).write_image(
        &bitmap.to_rgba(),
        bitmap.width(),
        bitmap.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(path)
}

/// Shows `path` in the platform file manager.
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    let r = if path.is_dir() {
        std::process::Command::new("explorer").arg(path).spawn()
    } else {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
    if let Err(e) = r {
        log::error!("could not open {}: {e}", path.display());
    }
}

/// Opens a link in the default browser. Only called with URLs decoded from
/// a QR code the user captured and then explicitly clicked.
fn open_url(url: &str) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        log::error!("could not open link: {e}");
    }
}

fn show_toast(content: Content, display: &Display, cx: &mut App) {
    let h = Toast::height(&content);
    if let Some((toast, _)) = open_card(display, toast::WIDTH, h, cx, |cx| cx.new(|_| Toast::new(content)), Card::Toast) {
        cx.subscribe(&toast, |toast, a: &ToastAction, cx| match a.clone() {
            ToastAction::Copy(text) => copy_text(text, cx),
            ToastAction::Open(url) => open_url(&url),
            ToastAction::Reveal(path) => reveal(&path),
            ToastAction::Dismiss => close_card(toast.entity_id(), cx),
        })
        .detach();
    }
}

fn open_settings_window(cx: &mut App) {
    if let Some(handle) = cx.global::<Sukurin>().settings_window
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(720.), px(470.)), cx)),
        titlebar: Some(TitlebarOptions { title: Some("Sukurin Settings".into()), ..Default::default() }),
        focus: true,
        show: true,
        ..Default::default()
    };
    let opened = open_window(opts, cx, |window, cx| {
        let view = cx.new(crate::settings_window::SettingsWindow::new);
        let focus = view.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        view
    });
    match opened {
        Ok((handle, _)) => cx.global_mut::<Sukurin>().settings_window = Some(handle),
        Err(e) => log::error!("could not open settings: {e:#}"),
    }
}

fn open_onboarding_window(cx: &mut App) {
    if let Some(handle) = cx.global::<Sukurin>().onboarding_window
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(640.), px(500.)), cx)),
        titlebar: Some(TitlebarOptions { title: Some("Welcome to Sukurin".into()), ..Default::default() }),
        focus: true,
        show: true,
        ..Default::default()
    };
    let opened = open_window(opts, cx, |window, cx| {
        let view = cx.new(crate::onboarding::Onboarding::new);
        let focus = view.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        view
    });
    match opened {
        Ok((handle, _)) => cx.global_mut::<Sukurin>().onboarding_window = Some(handle),
        Err(e) => log::error!("could not open setup: {e:#}"),
    }
}
