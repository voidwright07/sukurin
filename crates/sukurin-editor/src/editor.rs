//! Editor state and everything input does to it. Rendering lives in `ui`,
//! `inspector` and `palette`.

use crate::color::{Format, Srgb};
use crate::geom::{self, Handle};
use crate::keys::{self, Action};
use crate::paint::{self, ImageCache, PaintPrim};
use crate::tools::Tool;
use crate::viewport::{self, Viewport};
use crate::EditorHost;
use gpui_kit::*;
use shattr_annotate::{
    AnnotationId, ArrowKind, Axis, Backdrop, Document, History, Pt, RectF, RedactMode, Shape, Style,
};
use shattr_core::{Bitmap, Shot};
use shattr_ui::{DARK, LIGHT, Theme};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

pub const TITLE_H: f32 = 38.;
pub const PROPS_H: f32 = 40.;
pub const STATUS_H: f32 = 28.;
pub const STRIP_W: f32 = 44.;
const FIT_MARGIN: f32 = 32.;
/// Resize limits, well inside GPU texture sizes and memory.
const MAX_SIDE: f32 = 16384.;
const MAX_AREA: f32 = 64e6;
/// Grab radius of handles, in view pixels.
const GRAB: f32 = 7.;

/// What a press on the canvas is doing until the button comes back up.
pub enum Gesture {
    Idle,
    Pan { last: Point<Pixels> },
    /// Dragging out a new annotation. `before` is the document to restore on
    /// cancel and to checkpoint on commit.
    Draw { before: Document, id: AnnotationId, anchor: Pt },
    /// Moving the whole annotation or one of its handles.
    Edit { before: Document, id: AnnotationId, grab: Grab, from: Pt, orig: Shape },
    /// A magnifier whose callout follows the cursor until the next click.
    Place { before: Document, id: AnnotationId },
    CropDraw { anchor: Pt },
    CropEdit { grab: Grab, from: Pt, orig: RectF },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grab {
    Move,
    Handle(Handle),
}

pub struct TextEdit {
    pub id: AnnotationId,
    pub before: Document,
    pub fresh: bool,
}

/// Values the properties bar edits; new annotations start from these.
#[derive(Clone, Copy)]
pub struct Defaults {
    pub style: Style,
    pub measure: Style,
    pub spotlight: Style,
    pub arrow: ArrowKind,
    pub blur: RedactMode,
    pub erase_text_only: bool,
    pub block: f32,
    pub spot_oval: bool,
    pub overlay_opacity: f32,
    pub aspect: Option<(u32, u32)>,
    pub measure_px: bool,
    pub gap: f32,
}

impl Default for Defaults {
    fn default() -> Self {
        let style = Style::default();
        Self {
            style,
            measure: Style { color: shattr_core::Rgba::opaque(0xE2, 0x3D, 0x9B), width: 2., ..style },
            spotlight: style,
            arrow: ArrowKind::Curved,
            blur: RedactMode::Pixelate,
            erase_text_only: false,
            block: 8.,
            spot_oval: false,
            overlay_opacity: 0.5,
            aspect: None,
            measure_px: false,
            gap: 24.,
        }
    }
}

pub struct PaletteState {
    pub query: String,
    pub selected: usize,
}

pub struct ResizeDraft {
    pub w: String,
    pub h: String,
    pub editing_w: bool,
    pub nearest: bool,
}

/// Painted scene, rebuilt only when the document changes. While a gesture
/// edits one annotation, `base` holds all the others and `live` that one.
#[derive(Default)]
pub struct SceneCache {
    pub layers: Vec<PaintPrim>,
    layer_key: Vec<(usize, Pt)>,
    pub base: Vec<PaintPrim>,
    base_key: Option<(u64, Option<AnnotationId>)>,
    pub live: Vec<PaintPrim>,
    /// Annotations stacked above the live one.
    pub above: Vec<PaintPrim>,
    /// The backdrop gradient as an image, so it matches export's
    /// corner-to-corner gradient exactly.
    pub backdrop: Option<PaintPrim>,
    backdrop_key: Option<BackdropKey>,
    backdrop_images: ImageCache,
    live_key: Option<(u64, AnnotationId)>,
    layer_images: ImageCache,
    base_images: ImageCache,
    live_images: ImageCache,
    /// Nearest-enlarged copies of the visible layer pixels, painted over the
    /// layers when zoomed in, because GPUI samples images bilinearly.
    pub crisp: Vec<PaintPrim>,
    crisp_key: Option<CrispKey>,
    crisp_images: ImageCache,
}

type BackdropKey = (shattr_core::Rgba, shattr_core::Rgba, u32, u32, u32, u32);

#[derive(Clone, PartialEq, Debug)]
pub struct CrispKey {
    vp: (u32, u32, u32),
    area: (u32, u32),
    layers: Vec<(usize, Pt)>,
}

impl SceneCache {
    pub fn release(&mut self, window: &mut Window) {
        self.layer_images.release(window);
        self.base_images.release(window);
        self.live_images.release(window);
        self.crisp_images.release(window);
        self.backdrop_images.release(window);
    }
}

pub struct Editor {
    pub host: Rc<dyn EditorHost>,
    pub shot: Shot,
    pub title: String,
    pub doc: Document,
    pub history: History,
    pub edited: bool,
    pub tool: Tool,
    pub defaults: Defaults,
    pub selected: Option<AnnotationId>,
    pub gesture: Gesture,
    pub vp: Viewport,
    pub fitted: bool,
    pub area: Bounds<Pixels>,
    pub scale_factor: f32,
    pub cursor: Option<Pt>,
    pub mouse: Point<Pixels>,
    pub space: bool,
    pub inspector: bool,
    pub palette: Option<PaletteState>,
    pub text: Option<TextEdit>,
    pub crop_draft: Option<RectF>,
    pub format: Format,
    pub picks: Vec<Srgb>,
    pub picking: bool,
    pub copied: Option<Format>,
    pub flash: Option<(SharedString, Instant)>,
    pub dims_px: bool,
    pub resize: Option<ResizeDraft>,
    pub hovered_tool: Option<Tool>,
    crisp_pending: Option<CrispKey>,
    rev: u64,
    base_rev: u64,
    pub scene: Rc<RefCell<SceneCache>>,
    pub focus: FocusHandle,
    _appearance: Subscription,
}

/// Follows the OS appearance. `SHATTR_THEME=light|dark` overrides it, for
/// checking both themes without touching system settings.
pub fn theme(window: &Window) -> (Theme, bool) {
    match std::env::var("SHATTR_THEME").as_deref() {
        Ok("light") => return (LIGHT, false),
        Ok("dark") => return (DARK, true),
        _ => {}
    }
    match window.appearance() {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => (DARK, true),
        _ => (LIGHT, false),
    }
}

pub fn shape_tool(shape: &Shape) -> Tool {
    match shape {
        Shape::Arrow { .. } => Tool::Arrow,
        Shape::Line { .. } => Tool::Line,
        Shape::Rect { .. } => Tool::Rect,
        Shape::Oval { .. } => Tool::Oval,
        Shape::Freehand { .. } => Tool::Pen,
        Shape::Highlighter { .. } => Tool::Highlighter,
        Shape::Text { .. } => Tool::Text,
        Shape::Step { .. } => Tool::Step,
        Shape::Spotlight { .. } => Tool::Spotlight,
        Shape::Redact { mode: RedactMode::Pixelate | RedactMode::Blur, .. } => Tool::Blur,
        Shape::Redact { .. } => Tool::Erase,
        Shape::Magnifier { .. } => Tool::Magnifier,
        Shape::Measure { .. } => Tool::Measure,
        Shape::Guide { .. } => Tool::Guides,
        Shape::Overlay { .. } => Tool::Overlay,
    }
}

pub fn capture_name(shot: &Shot) -> String {
    let secs = shot.taken_at.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let local = secs as i64 + local_offset_secs();
    let day = local.rem_euclid(86_400);
    format!("Capture {:02}.{:02}.{:02}", day / 3600, day % 3600 / 60, day % 60)
}

#[cfg(windows)]
fn local_offset_secs() -> i64 {
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        rest: [u8; 168],
    }
    unsafe extern "system" {
        fn GetTimeZoneInformation(tzi: *mut TimeZoneInformation) -> u32;
    }
    let mut tzi = TimeZoneInformation { bias: 0, rest: [0; 168] };
    let id = unsafe { GetTimeZoneInformation(&mut tzi) };
    let dst_bias = i32::from_le_bytes([tzi.rest[164], tzi.rest[165], tzi.rest[166], tzi.rest[167]]);
    let bias = if id == 2 { tzi.bias + dst_bias } else { tzi.bias };
    -(bias as i64) * 60
}

#[cfg(unix)]
fn local_offset_secs() -> i64 {
    #[repr(C)]
    struct Tm {
        fields: [i32; 9],
        gmtoff: std::ffi::c_long,
        zone: *const std::ffi::c_char,
    }
    unsafe extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm { fields: [0; 9], gmtoff: 0, zone: std::ptr::null() };
    unsafe {
        let now = time(std::ptr::null_mut());
        if localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
    }
    tm.gmtoff as i64
}

#[cfg(not(any(windows, unix)))]
fn local_offset_secs() -> i64 {
    0
}

pub fn decode_image(bytes: &[u8]) -> Option<Bitmap> {
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    Some(Bitmap::from_rgba(img.width(), img.height(), img.into_raw()))
}

impl Editor {
    pub fn new(shot: Shot, host: Rc<dyn EditorHost>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let doc = Document::new(shot.bitmap.clone(), shot.scale);
        let appearance = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
        let mut this = Self {
            host,
            title: capture_name(&shot),
            shot,
            doc,
            history: History::default(),
            edited: false,
            tool: Tool::Arrow,
            defaults: Defaults::default(),
            selected: None,
            gesture: Gesture::Idle,
            vp: Viewport { zoom: 1., origin: Pt::default() },
            fitted: true,
            area: Bounds::default(),
            scale_factor: window.scale_factor(),
            cursor: None,
            mouse: Point::default(),
            space: false,
            inspector: false,
            palette: None,
            text: None,
            crop_draft: None,
            format: Format::Hex,
            picks: Vec::new(),
            picking: false,
            copied: None,
            flash: None,
            dims_px: false,
            resize: None,
            hovered_tool: None,
            crisp_pending: None,
            rev: 0,
            base_rev: 0,
            scene: Rc::default(),
            focus: cx.focus_handle(),
            _appearance: appearance,
        };
        this.layout(window);
        this
    }

    /// Recomputes the canvas area from the window size, refitting when the
    /// user hasn't zoomed or panned yet.
    pub fn layout(&mut self, window: &Window) {
        let s = window.viewport_size();
        let area = Bounds::new(
            point(px(STRIP_W), px(TITLE_H + PROPS_H)),
            size((s.width - px(STRIP_W)).max(px(1.)), (s.height - px(TITLE_H + PROPS_H + STATUS_H)).max(px(1.))),
        );
        let sf = window.scale_factor();
        if area != self.area || sf != self.scale_factor {
            self.area = area;
            self.scale_factor = sf;
            if self.fitted {
                self.fit();
            }
        }
    }

    pub fn content_rect(&self) -> RectF {
        let base = self.shown_rect();
        match self.doc.backdrop {
            Some(b) if self.tool != Tool::Crop => backdrop_frame(base, &b),
            _ => base,
        }
    }

    /// The part of the canvas that is shown: the crop, or everything.
    pub fn shown_rect(&self) -> RectF {
        match (self.tool, self.doc.crop) {
            (Tool::Crop, _) | (_, None) => self.canvas_bounds(),
            (_, Some(c)) => c,
        }
    }

    pub fn canvas_bounds(&self) -> RectF {
        self.doc.canvas_bounds()
    }

    pub fn fit(&mut self) {
        let a = self.area.size;
        self.vp = Viewport::fit(self.content_rect(), a.width.as_f32(), a.height.as_f32(), FIT_MARGIN, self.scale_factor);
        self.fitted = true;
    }

    fn zoom_to(&mut self, zoom: f32, anchor: Option<Pt>) {
        let a = self.area.size;
        let anchor = anchor.unwrap_or(Pt::new(a.width.as_f32() / 2., a.height.as_f32() / 2.));
        self.vp = self.vp.zoom_at(zoom, anchor, self.scale_factor);
        self.fitted = false;
    }

    pub fn view_point(&self, p: Point<Pixels>) -> Pt {
        Pt::new((p.x - self.area.origin.x).as_f32(), (p.y - self.area.origin.y).as_f32())
    }

    pub fn canvas_point(&self, p: Point<Pixels>) -> Pt {
        self.vp.to_canvas(self.view_point(p))
    }

    pub fn in_area(&self, p: Point<Pixels>) -> bool {
        self.area.contains(&p)
    }

    /// The tool whose properties the bar shows: the selected annotation's
    /// kind, else the active tool.
    pub fn props_tool(&self) -> Tool {
        match self.selected.and_then(|id| self.doc.get(id)) {
            Some(a) if matches!(self.tool, Tool::Select) || shape_tool(&a.shape).spec().props == self.tool.spec().props => {
                shape_tool(&a.shape)
            }
            _ => self.tool,
        }
    }

    pub fn style_for(&self, tool: Tool) -> Style {
        match tool {
            Tool::Measure | Tool::Guides => self.defaults.measure,
            Tool::Spotlight => self.defaults.spotlight,
            _ => self.defaults.style,
        }
    }

    pub fn current_style(&self) -> Style {
        match self.selected.and_then(|id| self.doc.get(id)) {
            Some(a) => a.style,
            None => self.style_for(self.tool),
        }
    }

    pub fn selected_shape(&self) -> Option<&Shape> {
        self.selected.and_then(|id| self.doc.get(id)).map(|a| &a.shape)
    }

    fn changed(&mut self, base: bool) {
        self.rev += 1;
        if base {
            self.base_rev += 1;
        }
        self.edited = true;
    }

    /// Ends whatever is in progress (a text edit, a drag, a magnifier being
    /// placed) as its own undo step, so the next change starts clean.
    pub fn settle(&mut self) {
        self.commit_text();
        match self.gesture {
            Gesture::Draw { .. } | Gesture::Edit { .. } | Gesture::Place { .. } => self.finish_gesture(),
            Gesture::Pan { .. } | Gesture::CropDraw { .. } | Gesture::CropEdit { .. } => self.gesture = Gesture::Idle,
            Gesture::Idle => {}
        }
    }

    /// One undoable step outside any gesture.
    pub fn mutate(&mut self, f: impl FnOnce(&mut Document)) {
        self.settle();
        self.history.checkpoint(&self.doc);
        f(&mut self.doc);
        self.changed(true);
    }

    /// Applies a style change to the selection and to the defaults for new
    /// annotations of the same kind.
    pub fn set_style(&mut self, f: impl Fn(&mut Style)) {
        let tool = self.props_tool();
        match tool {
            Tool::Measure | Tool::Guides => f(&mut self.defaults.measure),
            Tool::Spotlight => f(&mut self.defaults.spotlight),
            _ => f(&mut self.defaults.style),
        }
        if let Some(id) = self.text.as_ref().map(|t| t.id) {
            // The text edit's commit records this change with the typing.
            if let Some(a) = self.doc.get_mut(id) {
                f(&mut a.style);
            }
            self.changed(false);
        } else if let Some(id) = self.selected {
            self.mutate(|d| {
                if let Some(a) = d.get_mut(id) {
                    f(&mut a.style);
                }
            });
        }
    }

    /// Applies a shape-parameter change (arrow kind, redaction mode, …) to
    /// the selection.
    pub fn set_shape(&mut self, f: impl Fn(&mut Shape)) {
        if let Some(id) = self.selected {
            self.mutate(|d| {
                if let Some(a) = d.get_mut(id) {
                    f(&mut a.shape);
                }
            });
        }
    }

    pub fn set_arrow_kind(&mut self, kind: ArrowKind) {
        self.defaults.arrow = kind;
        self.set_shape(|s| {
            if let Shape::Arrow { kind: k, control, .. } = s {
                *k = kind;
                if kind != ArrowKind::Bendable {
                    *control = None;
                }
            }
        });
    }

    pub fn set_redact_mode(&mut self, mode: RedactMode) {
        match mode {
            RedactMode::Pixelate | RedactMode::Blur => self.defaults.blur = mode,
            RedactMode::Erase => self.defaults.erase_text_only = false,
            RedactMode::EraseText => self.defaults.erase_text_only = true,
        }
        self.set_shape(|s| {
            if let Shape::Redact { mode: m, .. } = s {
                *m = mode;
            }
        });
    }

    pub fn redact_mode(&self) -> RedactMode {
        if let Some(Shape::Redact { mode, .. }) = self.selected_shape() {
            return *mode;
        }
        match self.tool {
            Tool::Erase if self.defaults.erase_text_only => RedactMode::EraseText,
            Tool::Erase => RedactMode::Erase,
            _ => self.defaults.blur,
        }
    }

    pub fn set_block(&mut self, block: f32) {
        self.defaults.block = block;
        self.set_shape(|s| {
            if let Shape::Redact { block: b, .. } = s {
                *b = block;
            }
        });
    }

    pub fn set_spot_oval(&mut self, oval: bool) {
        self.defaults.spot_oval = oval;
        self.set_shape(|s| {
            if let Shape::Spotlight { oval: o, .. } = s {
                *o = oval;
            }
        });
    }

    pub fn set_overlay_opacity(&mut self, opacity: f32) {
        let opacity = opacity.clamp(0.05, 1.);
        self.defaults.overlay_opacity = opacity;
        self.set_shape(|s| {
            if let Shape::Overlay { opacity: o, .. } = s {
                *o = opacity;
            }
        });
    }

    pub fn set_backdrop(&mut self, b: Option<Backdrop>) {
        self.mutate(|d| d.backdrop = b);
        if self.fitted {
            self.fit();
        }
    }

    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.settle();
        if tool != Tool::Crop {
            self.crop_draft = None;
        }
        match tool {
            Tool::Blur if self.tool == Tool::Blur => {
                let next = if self.defaults.blur == RedactMode::Pixelate { RedactMode::Blur } else { RedactMode::Pixelate };
                self.defaults.blur = next;
            }
            Tool::Crop => {
                self.crop_draft = self.doc.crop.or(Some(self.canvas_bounds()));
            }
            Tool::Backdrop if self.doc.backdrop.is_none() => {
                self.set_backdrop(Some(crate::ui::BACKDROPS[1].backdrop(&self.doc)));
            }
            Tool::Combine => self.paste_layer(cx),
            _ => {}
        }
        if tool != Tool::Select
            && let Some(a) = self.selected.and_then(|id| self.doc.get(id))
            && shape_tool(&a.shape).spec().props != tool.spec().props
        {
            self.selected = None;
        }
        let refit = self.fitted && (tool == Tool::Crop) != (self.tool == Tool::Crop);
        self.tool = tool;
        if refit {
            self.fit();
        }
        cx.notify();
    }

    pub fn flash(&mut self, text: impl Into<SharedString>) {
        self.flash = Some((text.into(), Instant::now()));
    }

    pub fn export(&self) -> Bitmap {
        let t = Instant::now();
        let b = shattr_annotate::export(&self.doc);
        log::info!("export {}x{} in {:.1} ms", b.width(), b.height(), t.elapsed().as_secs_f64() * 1000.);
        b
    }

    pub fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let view_only = matches!(
            action,
            Action::Palette
                | Action::Shortcuts
                | Action::Inspector
                | Action::ZoomIn
                | Action::ZoomOut
                | Action::ZoomFit
                | Action::Zoom100
                | Action::CopyColour
                | Action::CycleFormat
                | Action::SwapContrast
                | Action::HandDrawn
                | Action::Upload
                | Action::SendToAi
        );
        if !view_only && !matches!(action, Action::Undo) {
            self.settle();
        }
        match action {
            Action::Tool(t) => self.set_tool(t, cx),
            Action::ArrowKind(k) => {
                if self.tool != Tool::Arrow && !matches!(self.selected_shape(), Some(Shape::Arrow { .. })) {
                    self.set_tool(Tool::Arrow, cx);
                }
                self.set_arrow_kind(k);
            }
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Copy => {
                self.commit_text();
                let b = self.export();
                self.host.copy(&b, cx);
                self.flash("Copied");
            }
            Action::Save => {
                self.commit_text();
                let b = self.export();
                self.host.save(&b, cx);
                self.flash("Saved");
            }
            Action::Pin => {
                self.commit_text();
                let bitmap = self.export();
                let shot = Shot { bitmap, scale: self.shot.scale, source_rect: self.shot.source_rect, taken_at: self.shot.taken_at };
                self.host.pin(shot, cx);
                self.flash("Pinned");
            }
            Action::OpenLibrary => {
                self.palette = None;
                self.host.open_library(cx);
            }
            Action::Upload | Action::SendToAi => {}
            Action::Palette => {
                self.palette = match self.palette {
                    Some(_) => None,
                    None => Some(PaletteState { query: String::new(), selected: 0 }),
                }
            }
            Action::Shortcuts => self.palette = Some(PaletteState { query: String::new(), selected: 0 }),
            Action::Inspector => {
                self.inspector = !self.inspector;
                self.resize = None;
            }
            Action::ZoomIn => self.zoom_to(viewport::step(self.vp.zoom, self.scale_factor, true), None),
            Action::ZoomOut => self.zoom_to(viewport::step(self.vp.zoom, self.scale_factor, false), None),
            Action::ZoomFit => self.fit(),
            Action::Zoom100 => self.zoom_to(1. / self.scale_factor, None),
            Action::Delete => {
                if let Some(id) = self.selected.take() {
                    self.mutate(|d| {
                        d.remove(id);
                    });
                }
            }
            Action::Duplicate => self.duplicate(),
            Action::HandDrawn => self.set_style(|s| s.hand_drawn = !s.hand_drawn),
            Action::EraseTextOnly => {
                if self.tool != Tool::Erase && !matches!(self.selected_shape(), Some(Shape::Redact { .. })) {
                    self.set_tool(Tool::Erase, cx);
                }
                let mode = if self.redact_mode() == RedactMode::EraseText { RedactMode::Erase } else { RedactMode::EraseText };
                self.set_redact_mode(mode);
            }
            Action::TrimToContent => self.trim_to_content(cx),
            Action::ApplyCrop => self.apply_crop(cx),
            Action::Paste => self.paste_overlay(cx),
            Action::CopyColour => self.copy_colour(cx),
            Action::CycleFormat => {
                self.format = self.format.next();
                self.copied = None;
            }
            Action::SwapContrast => self.picks.reverse(),
            Action::ArrangeRow => self.arrange(Arrange::Row),
            Action::ArrangeColumn => self.arrange(Arrange::Column),
            Action::ArrangeGrid => self.arrange(Arrange::Grid),
            Action::Close => window.remove_window(),
        }
        cx.notify();
    }

    pub fn undo(&mut self) {
        let discarding = self.text.as_ref().is_some_and(|t| t.fresh);
        if matches!(self.gesture, Gesture::Draw { .. } | Gesture::Edit { .. } | Gesture::Place { .. }) || discarding {
            if discarding {
                let edit = self.text.take().expect("checked above");
                self.doc = edit.before;
                self.selected = None;
                self.changed(true);
            } else {
                self.cancel_gesture();
            }
            return;
        }
        self.settle();
        if self.history.undo(&mut self.doc) {
            self.after_history();
        }
    }

    pub fn redo(&mut self) {
        self.settle();
        if self.history.redo(&mut self.doc) {
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.changed(true);
        if self.selected.is_some_and(|id| self.doc.get(id).is_none()) {
            self.selected = None;
        }
        if self.fitted {
            self.fit();
        }
    }

    fn duplicate(&mut self) {
        let Some(a) = self.selected.and_then(|id| self.doc.get(id)).cloned() else { return };
        let offset = 16. * self.doc.scale;
        let mut new = None;
        self.mutate(|d| new = Some(d.add(geom::translate(&a.shape, offset, offset), a.style)));
        self.selected = new;
    }

    pub fn colour_at(&self, p: Pt) -> Option<Srgb> {
        self.doc.layers.iter().rev().find_map(|l| {
            let (x, y) = (p.x - l.origin.x, p.y - l.origin.y);
            if x < 0. || y < 0. {
                return None;
            }
            l.bitmap.pixel(x as u32, y as u32).map(|c| Srgb::new(c.r, c.g, c.b))
        })
    }

    pub fn colour_under_cursor(&self) -> Option<Srgb> {
        self.cursor.and_then(|p| self.colour_at(p))
    }

    fn pick(&mut self, c: Srgb) {
        self.picks.retain(|p| *p != c);
        self.picks.insert(0, c);
        self.picks.truncate(2);
    }

    fn copy_colour(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.colour_under_cursor().or(self.picks.first().copied()) else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(self.format.css(c)));
        self.pick(c);
        self.copied = Some(self.format);
        self.flash(format!("Copied {}", self.format.css(c)));
    }

    /// The region "average of selection" reads: the selected annotation's
    /// frame, the crop draft, or nothing.
    pub fn sample_rect(&self) -> Option<RectF> {
        if let Some(r) = self.crop_draft.filter(|_| self.tool == Tool::Crop) {
            return Some(r);
        }
        let id = self.selected?;
        let shape = &self.doc.get(id)?.shape;
        geom::frame(shape).or_else(|| match shape {
            Shape::Arrow { from, to, .. } | Shape::Line { from, to } | Shape::Measure { from, to } => {
                Some(RectF::from_points(*from, *to))
            }
            _ => None,
        })
    }

    pub fn average(&self, r: RectF) -> Option<Srgb> {
        let l = self.doc.layers.first()?;
        let rect = shattr_core::RectPx::new(
            (r.x - l.origin.x).floor() as i32,
            (r.y - l.origin.y).floor() as i32,
            r.w.ceil().max(1.) as u32,
            r.h.ceil().max(1.) as u32,
        );
        l.bitmap.average(rect).map(|c| Srgb::new(c.r, c.g, c.b))
    }

    fn trim_to_content(&mut self, cx: &mut Context<Self>) {
        let Some(l) = self.doc.layers.first() else { return };
        let from = self.crop_draft.or(self.doc.crop).unwrap_or(self.canvas_bounds());
        let rect = shattr_core::RectPx::new(
            (from.x - l.origin.x).round() as i32,
            (from.y - l.origin.y).round() as i32,
            from.w.round() as u32,
            from.h.round() as u32,
        );
        let t = l.bitmap.trim_flat_margins(rect, 6);
        let origin = l.origin;
        if self.tool != Tool::Crop {
            self.set_tool(Tool::Crop, cx);
        }
        self.crop_draft = Some(RectF::new(t.x as f32 + origin.x, t.y as f32 + origin.y, t.w as f32, t.h as f32));
    }

    pub fn apply_crop(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.crop_draft.filter(|_| self.tool == Tool::Crop) else { return };
        let full = self.canvas_bounds();
        let crop = geom::intersect(draft, full).map(|r| {
            RectF::new(r.x.round(), r.y.round(), r.w.round().max(1.), r.h.round().max(1.))
        });
        let crop = crop.filter(|r| *r != full);
        if crop != self.doc.crop {
            self.mutate(|d| d.crop = crop);
        }
        self.crop_draft = None;
        self.tool = Tool::Select;
        self.fit();
        cx.notify();
    }

    fn clipboard_image(cx: &mut Context<Self>) -> Option<Bitmap> {
        let item = cx.read_from_clipboard()?;
        item.entries().iter().find_map(|e| match e {
            ClipboardEntry::Image(img) => decode_image(&img.bytes),
            ClipboardEntry::ExternalPaths(paths) => paths.paths().iter().find_map(|p| std::fs::read(p).ok().and_then(|b| decode_image(&b))),
            _ => None,
        })
    }

    pub fn paste_overlay(&mut self, cx: &mut Context<Self>) {
        match Self::clipboard_image(cx) {
            Some(b) => self.add_overlay(b),
            None => self.flash("No image on the clipboard"),
        }
    }

    pub fn add_overlay(&mut self, image: Bitmap) {
        let shown = self.shown_rect();
        let k = (shown.w * 0.8 / image.width() as f32).min(shown.h * 0.8 / image.height() as f32).min(1.);
        let (w, h) = (image.width() as f32 * k, image.height() as f32 * k);
        let rect = RectF::new(shown.x + (shown.w - w) / 2., shown.y + (shown.h - h) / 2., w, h);
        let opacity = self.defaults.overlay_opacity;
        let style = self.defaults.style;
        let mut id = None;
        self.mutate(|d| id = Some(d.add(Shape::Overlay { rect, image, opacity }, style)));
        self.selected = id;
        self.crop_draft = None;
        let refit = self.fitted && self.tool == Tool::Crop;
        self.tool = Tool::Overlay;
        if refit {
            self.fit();
        }
    }

    pub fn paste_layer(&mut self, cx: &mut Context<Self>) {
        match Self::clipboard_image(cx) {
            Some(b) => self.add_layer(b),
            None => self.flash("Copy another capture, then Combine"),
        }
    }

    pub fn add_layer(&mut self, bitmap: Bitmap) {
        let r = self.canvas_bounds();
        let origin = Pt::new(r.x + r.w + self.defaults.gap * self.doc.scale, r.y);
        self.mutate(|d| {
            d.layers.push(shattr_annotate::Layer { bitmap, origin });
            d.crop = None;
        });
        self.fit();
    }

    pub fn arrange(&mut self, how: Arrange) {
        let gap = self.defaults.gap * self.doc.scale;
        let sizes: Vec<(f32, f32)> =
            self.doc.layers.iter().map(|l| (l.bitmap.width() as f32, l.bitmap.height() as f32)).collect();
        let cols = match how {
            Arrange::Row => sizes.len(),
            Arrange::Column => 1,
            Arrange::Grid => (sizes.len() as f32).sqrt().ceil() as usize,
        }
        .max(1);
        let col_w: Vec<f32> = (0..cols).map(|c| sizes.iter().skip(c).step_by(cols).map(|s| s.0).fold(0., f32::max)).collect();
        let rows = sizes.len().div_ceil(cols);
        let row_h: Vec<f32> = (0..rows).map(|r| sizes.iter().skip(r * cols).take(cols).map(|s| s.1).fold(0., f32::max)).collect();
        let origins: Vec<Pt> = (0..sizes.len())
            .map(|i| {
                let (c, r) = (i % cols, i / cols);
                Pt::new(col_w[..c].iter().sum::<f32>() + gap * c as f32, row_h[..r].iter().sum::<f32>() + gap * r as f32)
            })
            .collect();
        self.mutate(|d| {
            for (l, o) in d.layers.iter_mut().zip(origins) {
                l.origin = o;
            }
            d.crop = None;
        });
        self.fit();
    }

    pub fn commit_text(&mut self) {
        let Some(edit) = self.text.take() else { return };
        let text = match self.doc.get(edit.id).map(|a| &a.shape) {
            Some(Shape::Text { text, .. }) => text.clone(),
            _ => return,
        };
        if text.trim().is_empty() {
            self.doc.remove(edit.id);
            if !edit.fresh {
                self.history.checkpoint(&edit.before);
            }
            self.selected = None;
        } else {
            if edit.before.get(edit.id) != self.doc.get(edit.id) {
                self.history.checkpoint(&edit.before);
            }
            self.selected = Some(edit.id);
        }
        self.changed(true);
    }

    fn begin_text(&mut self, id: AnnotationId, fresh: bool, before: Document) {
        self.text = Some(TextEdit { id, before, fresh });
        self.selected = Some(id);
    }

    fn edit_text(&mut self, f: impl FnOnce(&mut String)) {
        let Some(id) = self.text.as_ref().map(|t| t.id) else { return };
        if let Some(Shape::Text { text, .. }) = self.doc.get_mut(id).map(|a| &mut a.shape) {
            f(text);
        }
        self.changed(false);
    }

    /// The annotation being edited live, painted on its own over the cached rest.
    pub fn live_id(&self) -> Option<AnnotationId> {
        match &self.gesture {
            Gesture::Draw { id, .. } | Gesture::Edit { id, .. } | Gesture::Place { id, .. } => Some(*id),
            _ => self.text.as_ref().map(|t| t.id),
        }
    }

    fn layer_key(&self) -> Vec<(usize, Pt)> {
        self.doc.layers.iter().map(|l| (l.bitmap.bgra().as_ptr() as usize, l.origin)).collect()
    }

    /// What a crisp overlay would be built for now, if one is wanted.
    fn crisp_key(&self) -> Option<CrispKey> {
        if self.vp.zoom * self.scale_factor < 1.5 || matches!(self.gesture, Gesture::Pan { .. }) {
            return None;
        }
        Some(CrispKey {
            vp: (self.vp.zoom.to_bits(), self.vp.origin.x.to_bits(), self.vp.origin.y.to_bits()),
            area: (self.area.size.width.as_f32().to_bits(), self.area.size.height.as_f32().to_bits()),
            layers: self.layer_key(),
        })
    }

    /// Schedules a crisp overlay once the view has been still for a moment,
    /// and drops one that no longer matches the view.
    pub fn refresh_crisp(&mut self, cx: &mut Context<Self>) {
        let want = self.crisp_key();
        {
            let mut cache = self.scene.borrow_mut();
            if cache.crisp_key != want && !cache.crisp.is_empty() {
                cache.crisp.clear();
                cache.crisp_images.begin();
                cache.crisp_images.end();
                cache.crisp_key = None;
            }
            if want.is_none() || cache.crisp_key == want || self.crisp_pending == want {
                return;
            }
        }
        self.crisp_pending = want.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(90)).await;
            let _ = this.update(cx, |this, cx| {
                if this.crisp_key() == want {
                    this.build_crisp();
                    cx.notify();
                }
                if this.crisp_pending == want {
                    this.crisp_pending = None;
                }
            });
        })
        .detach();
    }

    fn build_crisp(&mut self) {
        let a = self.area.size;
        let visible = RectF::from_points(self.vp.to_canvas(Pt::new(0., 0.)), self.vp.to_canvas(Pt::new(a.width.as_f32(), a.height.as_f32())));
        let zoom = self.vp.zoom * self.scale_factor;
        let mut cache = self.scene.borrow_mut();
        let cache = &mut *cache;
        cache.crisp_images.begin();
        cache.crisp.clear();
        for l in &self.doc.layers {
            let lr = RectF::new(l.origin.x, l.origin.y, l.bitmap.width() as f32, l.bitmap.height() as f32);
            let Some(r) = geom::intersect(visible, lr) else { continue };
            let (x0, y0) = ((r.x - l.origin.x).floor().max(0.), (r.y - l.origin.y).floor().max(0.));
            let (x1, y1) = (
                (r.x + r.w - l.origin.x).ceil().min(l.bitmap.width() as f32),
                (r.y + r.h - l.origin.y).ceil().min(l.bitmap.height() as f32),
            );
            let src = shattr_core::RectPx::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32);
            let Some(crop) = l.bitmap.crop(src) else { continue };
            let k = (zoom.round() as u32).clamp(2, 64).min(4096 / crop.width().max(1)).min(4096 / crop.height().max(1)).max(1);
            let up = paint::nearest(&crop, crop.width() * k, crop.height() * k);
            let image = cache.crisp_images.image(&up, 1., None);
            let dest = RectF::new(l.origin.x + x0, l.origin.y + y0, src.w as f32, src.h as f32);
            cache.crisp.push(PaintPrim::Image { dest, image });
        }
        cache.crisp_images.end();
        cache.crisp_key = self.crisp_key();
    }

    /// Brings the painted scene up to date with the document.
    pub fn refresh_scene(&mut self) {
        let mut cache = self.scene.borrow_mut();
        let cache = &mut *cache;
        let layer_key = self.layer_key();
        if layer_key != cache.layer_key {
            cache.layer_images.begin();
            cache.layers = self
                .doc
                .layers
                .iter()
                .map(|l| PaintPrim::Image {
                    dest: RectF::new(l.origin.x, l.origin.y, l.bitmap.width() as f32, l.bitmap.height() as f32),
                    image: cache.layer_images.image(&l.bitmap, 1., None),
                })
                .collect();
            cache.layer_images.end();
            cache.layer_key = layer_key;
        }
        let live = self.live_id();
        let base_key = Some((self.base_rev, live));
        if cache.base_key != base_key {
            // The live annotation keeps its place in the stack: the others
            // are split into what paints below it and what paints above.
            let split = live.and_then(|id| self.doc.annotations.iter().position(|a| a.id == id));
            let (below, above) = match split {
                Some(i) => {
                    let mut below = self.doc.clone();
                    below.annotations.truncate(i);
                    let mut above = self.doc.clone();
                    above.annotations.drain(..=i);
                    (shattr_annotate::annotations_scene(&below), shattr_annotate::annotations_scene(&above))
                }
                None => (shattr_annotate::annotations_scene(&self.doc), Vec::new()),
            };
            cache.base_images.begin();
            cache.base = paint::prepare(&below, &mut cache.base_images);
            cache.above = paint::prepare(&above, &mut cache.base_images);
            cache.base_images.end();
            cache.base_key = base_key;
        }
        let backdrop_key = self.doc.backdrop.filter(|_| self.tool != Tool::Crop).map(|b| {
            let f = backdrop_frame(self.shown_rect(), &b);
            (b.from, b.to, f.x.to_bits(), f.y.to_bits(), f.w.to_bits(), f.h.to_bits())
        });
        if cache.backdrop_key != backdrop_key {
            cache.backdrop_images.begin();
            cache.backdrop = backdrop_key.and_then(|_| {
                let b = self.doc.backdrop?;
                let frame = backdrop_frame(self.shown_rect(), &b);
                let image = cache.backdrop_images.image(&gradient(&b, frame), 1., None);
                Some(PaintPrim::Image { dest: frame, image })
            });
            cache.backdrop_images.end();
            cache.backdrop_key = backdrop_key;
        }
        match live {
            Some(id) if cache.live_key != Some((self.rev, id)) => {
                let scene = shattr_annotate::annotation_scene(&self.doc, id);
                cache.live_images.begin();
                cache.live = paint::prepare(&scene, &mut cache.live_images);
                cache.live_images.end();
                cache.live_key = Some((self.rev, id));
            }
            Some(_) => {}
            None => {
                if !cache.live.is_empty() {
                    cache.live.clear();
                    cache.live_images.begin();
                    cache.live_images.end();
                }
                cache.live_key = None;
            }
        }
    }

    fn handle_at(&self, v: Pt) -> Option<(AnnotationId, Handle)> {
        let id = self.selected?;
        let shape = &self.doc.get(id)?.shape;
        geom::handles(shape).into_iter().find_map(|(h, p)| {
            let hp = self.vp.to_view(p);
            ((hp.x - v.x).abs() <= GRAB && (hp.y - v.y).abs() <= GRAB).then_some((id, h))
        })
    }

    fn crop_grab(&self, v: Pt) -> Option<Grab> {
        let r = self.crop_draft?;
        for g in geom::BOX {
            let p = self.vp.to_view(Pt::new(r.x + r.w * g.0 as f32 / 2., r.y + r.h * g.1 as f32 / 2.));
            if (p.x - v.x).abs() <= GRAB + 2. && (p.y - v.y).abs() <= GRAB + 2. {
                return Some(Grab::Handle(Handle::Box(g.0, g.1)));
            }
        }
        r.contains(self.vp.to_canvas(v)).then_some(Grab::Move)
    }

    pub fn cursor_style(&self) -> CursorStyle {
        let v = self.view_point(self.mouse);
        match &self.gesture {
            Gesture::Pan { .. } => return CursorStyle::ClosedHand,
            Gesture::Edit { grab: Grab::Move, .. } | Gesture::CropEdit { grab: Grab::Move, .. } => {
                return CursorStyle::ClosedHand;
            }
            _ => {}
        }
        if self.space {
            return CursorStyle::OpenHand;
        }
        if self.tool == Tool::Crop {
            return match self.crop_grab(v) {
                Some(Grab::Handle(Handle::Box(gx, gy))) => box_cursor(gx, gy),
                Some(_) => CursorStyle::OpenHand,
                None => CursorStyle::Crosshair,
            };
        }
        if let Some((_, h)) = self.handle_at(v) {
            return match h {
                Handle::Box(gx, gy) => box_cursor(gx, gy),
                _ => CursorStyle::PointingHand,
            };
        }
        match self.tool {
            Tool::Select => CursorStyle::Arrow,
            Tool::Text => CursorStyle::IBeam,
            Tool::Overlay | Tool::Backdrop | Tool::Combine => CursorStyle::Arrow,
            _ => CursorStyle::Crosshair,
        }
    }

    pub fn mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.mouse = ev.position;
        if !self.in_area(ev.position) {
            return;
        }
        if self.palette.is_some() {
            self.palette = None;
            cx.notify();
            return;
        }
        let v = self.view_point(ev.position);
        let p = self.vp.to_canvas(v);
        if ev.button == MouseButton::Middle || (ev.button == MouseButton::Left && self.space) {
            if !matches!(self.gesture, Gesture::Idle) {
                return;
            }
            self.gesture = Gesture::Pan { last: ev.position };
            cx.notify();
            return;
        }
        if ev.button != MouseButton::Left {
            return;
        }
        if self.picking {
            if let Some(c) = self.colour_at(p) {
                self.pick(c);
            }
            self.picking = false;
            cx.notify();
            return;
        }
        if let Gesture::Place { .. } = self.gesture {
            self.finish_gesture();
            cx.notify();
            return;
        }
        if let Some(edit) = &self.text {
            let on_same = self.doc.hit_test(p, GRAB / self.vp.zoom) == Some(edit.id);
            if on_same {
                return;
            }
            self.commit_text();
        }
        if self.tool == Tool::Crop {
            self.gesture = match self.crop_grab(v) {
                Some(grab) => Gesture::CropEdit { grab, from: p, orig: self.crop_draft.unwrap_or_default() },
                None => Gesture::CropDraw { anchor: p },
            };
            cx.notify();
            return;
        }
        if let Some((id, h)) = self.handle_at(v) {
            let Some(orig) = self.doc.get(id).map(|a| a.shape.clone()) else { return };
            self.gesture = Gesture::Edit { before: self.doc.clone(), id, grab: Grab::Handle(h), from: p, orig };
            cx.notify();
            return;
        }
        let hit = self.doc.hit_test(p, GRAB / self.vp.zoom);
        if ev.click_count >= 2
            && let Some(id) = hit
        {
            match &self.doc.get(id).map(|a| &a.shape) {
                Some(Shape::Text { .. }) => {
                    self.begin_text(id, false, self.doc.clone());
                    cx.notify();
                    return;
                }
                Some(Shape::Guide { .. }) if matches!(self.tool, Tool::Guides | Tool::Select) => {
                    self.selected = None;
                    self.mutate(|d| {
                        d.remove(id);
                    });
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }
        let grab_existing = match self.tool {
            Tool::Select => true,
            Tool::Text | Tool::Step | Tool::Guides => hit.is_some_and(|id| {
                self.doc.get(id).is_some_and(|a| shape_tool(&a.shape) == self.tool)
            }),
            Tool::Overlay | Tool::Backdrop | Tool::Combine => true,
            _ => hit.is_some() && hit == self.selected,
        };
        if grab_existing {
            match hit {
                Some(id) => {
                    self.selected = Some(id);
                    let orig = self.doc.get(id).map(|a| a.shape.clone()).expect("hit annotation exists");
                    self.gesture = Gesture::Edit { before: self.doc.clone(), id, grab: Grab::Move, from: p, orig };
                }
                None => self.selected = None,
            }
            cx.notify();
            return;
        }
        self.start_draw(p);
        cx.notify();
    }

    fn start_draw(&mut self, p: Pt) {
        let d = &self.defaults;
        let r0 = RectF::new(p.x, p.y, 0., 0.);
        let shape = match self.tool {
            Tool::Arrow => Shape::Arrow { from: p, to: p, control: None, kind: d.arrow },
            Tool::Line => Shape::Line { from: p, to: p },
            Tool::Rect => Shape::Rect { rect: r0 },
            Tool::Oval => Shape::Oval { rect: r0 },
            Tool::Pen => Shape::Freehand { points: vec![p] },
            Tool::Highlighter => Shape::Highlighter { points: vec![p] },
            Tool::Text => Shape::Text { origin: p, text: String::new() },
            Tool::Step => Shape::Step { center: p, number: self.doc.next_step() },
            Tool::Spotlight => Shape::Spotlight { rect: r0, oval: d.spot_oval },
            Tool::Blur => Shape::Redact { rect: r0, mode: d.blur, block: d.block },
            Tool::Erase => Shape::Redact {
                rect: r0,
                mode: if d.erase_text_only { RedactMode::EraseText } else { RedactMode::Erase },
                block: d.block,
            },
            Tool::Magnifier => Shape::Magnifier { source: r0, callout: r0 },
            Tool::Measure => Shape::Measure { from: p, to: p },
            Tool::Guides => {
                let b = self.canvas_bounds();
                let (dx, dy) = ((p.x - b.x).min(b.x + b.w - p.x), (p.y - b.y).min(b.y + b.h - p.y));
                if dx < dy { Shape::Guide { axis: Axis::Vertical, at: p.x } } else { Shape::Guide { axis: Axis::Horizontal, at: p.y } }
            }
            Tool::Select | Tool::Crop | Tool::Overlay | Tool::Backdrop | Tool::Combine => return,
        };
        let style = self.style_for(self.tool);
        let before = self.doc.clone();
        let id = self.doc.add(shape, style);
        self.selected = Some(id);
        self.changed(false);
        if self.tool == Tool::Text {
            self.begin_text(id, true, before);
            return;
        }
        self.gesture = Gesture::Draw { before, id, anchor: p };
    }

    pub fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.mouse = ev.position;
        if ev.position.x > px(STRIP_W) {
            self.hovered_tool = None;
        }
        let inside = self.in_area(ev.position);
        let p = self.canvas_point(ev.position);
        self.cursor = inside.then_some(p);
        let mods = ev.modifiers;
        let (constrain, centre) = (mods.shift, mods.alt);
        match &mut self.gesture {
            Gesture::Idle => {}
            Gesture::Pan { last } => {
                let d = ev.position - *last;
                *last = ev.position;
                self.vp = self.vp.pan(d.x.as_f32(), d.y.as_f32());
                self.fitted = false;
            }
            Gesture::Draw { id, anchor, .. } => {
                let (id, anchor) = (*id, *anchor);
                if let Some(a) = self.doc.get_mut(id) {
                    a.shape = drawn_shape(&a.shape, anchor, p, constrain, centre);
                }
                self.changed(false);
            }
            Gesture::Edit { id, grab, from, orig, .. } => {
                let (id, grab, from) = (*id, *grab, *from);
                let shape = match grab {
                    Grab::Move => {
                        let (mut dx, mut dy) = (p.x - from.x, p.y - from.y);
                        if constrain {
                            if dx.abs() > dy.abs() { dy = 0. } else { dx = 0. }
                        }
                        geom::translate(orig, dx, dy)
                    }
                    Grab::Handle(h) => geom::drag_handle(orig, h, p, constrain),
                };
                if let Some(a) = self.doc.get_mut(id) {
                    a.shape = shape;
                }
                self.changed(false);
            }
            Gesture::Place { id, .. } => {
                let id = *id;
                if let Some(Shape::Magnifier { callout, .. }) = self.doc.get_mut(id).map(|a| &mut a.shape) {
                    callout.x = p.x - callout.w / 2.;
                    callout.y = p.y - callout.h / 2.;
                }
                self.changed(false);
            }
            Gesture::CropDraw { anchor } => {
                let r = geom::drag_rect(*anchor, p, constrain, centre);
                self.crop_draft = Some(fit_aspect(r, *anchor, p, self.defaults.aspect));
            }
            Gesture::CropEdit { grab, from, orig } => {
                let r = match grab {
                    Grab::Move => RectF::new(orig.x + p.x - from.x, orig.y + p.y - from.y, orig.w, orig.h),
                    Grab::Handle(Handle::Box(gx, gy)) => geom::resize_box(*orig, (*gx, *gy), p, constrain || self.defaults.aspect.is_some()),
                    Grab::Handle(_) => *orig,
                };
                self.crop_draft = Some(r);
            }
        }
        cx.notify();
    }

    pub fn mouse_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        self.mouse = ev.position;
        match &self.gesture {
            Gesture::Pan { .. } => self.gesture = Gesture::Idle,
            Gesture::Draw { id, .. } if self.tool == Tool::Magnifier => {
                let id = *id;
                let ok = matches!(self.doc.get(id).map(|a| &a.shape), Some(Shape::Magnifier { source, .. }) if source.w >= 3. && source.h >= 3.);
                if ok {
                    let Gesture::Draw { before, .. } = std::mem::replace(&mut self.gesture, Gesture::Idle) else { unreachable!() };
                    self.gesture = Gesture::Place { before, id };
                } else {
                    self.cancel_gesture();
                }
            }
            Gesture::Draw { .. } | Gesture::Edit { .. } => self.finish_gesture(),
            Gesture::CropDraw { .. } | Gesture::CropEdit { .. } => {
                self.gesture = Gesture::Idle;
                if self.crop_draft.is_some_and(|r| r.w < 2. || r.h < 2.) {
                    self.crop_draft = None;
                }
            }
            Gesture::Idle | Gesture::Place { .. } => {}
        }
        cx.notify();
    }

    /// Commits the gesture as one undo step, or drops a degenerate drawing.
    fn finish_gesture(&mut self) {
        match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::Draw { before, id, .. } => {
                let keep = self.doc.get(id).is_some_and(|a| !degenerate(&a.shape, 3. / self.vp.zoom.max(0.01)));
                if keep {
                    self.history.checkpoint(&before);
                    self.changed(true);
                } else {
                    self.doc = before;
                    self.selected = None;
                    self.changed(true);
                }
            }
            Gesture::Edit { before, id, orig, .. } => {
                if self.doc.get(id).map(|a| &a.shape) != Some(&orig) {
                    self.history.checkpoint(&before);
                }
                self.changed(true);
            }
            Gesture::Place { before, .. } => {
                self.history.checkpoint(&before);
                self.changed(true);
            }
            g => self.gesture = g,
        }
    }

    fn cancel_gesture(&mut self) {
        match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::Draw { before, .. } | Gesture::Edit { before, .. } | Gesture::Place { before, .. } => {
                self.doc = before;
                if self.selected.is_some_and(|id| self.doc.get(id).is_none()) {
                    self.selected = None;
                }
                self.changed(true);
            }
            Gesture::CropDraw { .. } | Gesture::CropEdit { .. } => {}
            _ => {}
        }
    }

    pub fn scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let over_inspector = self.inspector && ev.position.x > self.area.right() - px(crate::inspector::WIDTH + 8.);
        if !self.in_area(ev.position) || over_inspector {
            return;
        }
        let v = self.view_point(ev.position);
        let d = ev.delta.pixel_delta(px(40.));
        if ev.modifiers.secondary() || ev.modifiers.platform || ev.modifiers.control {
            let dy = d.y.as_f32();
            let factor = (1.0015f32).powf(dy.clamp(-400., 400.));
            self.zoom_to(self.vp.zoom * factor, Some(v));
        } else {
            let (dx, dy) = if ev.modifiers.shift && d.x.as_f32() == 0. { (d.y, d.x) } else { (d.x, d.y) };
            self.vp = self.vp.pan(dx.as_f32(), dy.as_f32());
            self.fitted = false;
        }
        cx.notify();
    }

    pub fn pinch(&mut self, ev: &PinchEvent, cx: &mut Context<Self>) {
        let v = self.view_point(ev.position);
        self.zoom_to(self.vp.zoom * (1. + ev.delta), Some(v));
        cx.notify();
    }

    pub fn key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if self.palette.is_some() {
            self.palette_key(ev, window, cx);
            return;
        }
        if self.resize.is_some() {
            self.resize_key(ev, cx);
            return;
        }
        if self.text.is_some() && self.text_key(k) {
            cx.notify();
            return;
        }
        if k.key == "space" && !k.modifiers.modified() {
            self.space = true;
            cx.notify();
            return;
        }
        if k.key == "escape" {
            self.escape(window, cx);
            return;
        }
        if let Some(step) = arrow_step(k) {
            self.nudge(step, ev.is_held);
            cx.notify();
            return;
        }
        if !k.modifiers.modified() && (k.key == "[" || k.key == "]") {
            if let Some(Shape::Overlay { opacity, .. }) = self.selected_shape() {
                let next = opacity + if k.key == "]" { 0.1 } else { -0.1 };
                self.set_overlay_opacity(next);
            }
            cx.notify();
            return;
        }
        if ev.is_held && !matches!(keys::lookup(k), Some(Action::Undo | Action::Redo | Action::ZoomIn | Action::ZoomOut)) {
            return;
        }
        if let Some(action) = keys::lookup(k) {
            self.run(action, window, cx);
        }
    }

    pub fn key_up(&mut self, ev: &KeyUpEvent, cx: &mut Context<Self>) {
        if ev.keystroke.key == "space" {
            self.space = false;
            cx.notify();
        }
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking {
            self.picking = false;
        } else if !matches!(self.gesture, Gesture::Idle) {
            self.cancel_gesture();
        } else if self.tool == Tool::Crop {
            self.crop_draft = None;
            self.tool = Tool::Select;
            if self.fitted {
                self.fit();
            }
        } else if self.selected.is_some() {
            self.selected = None;
        } else {
            window.remove_window();
            return;
        }
        cx.notify();
    }

    /// Held arrows extend the first press's undo step instead of adding one
    /// per auto-repeat.
    fn nudge(&mut self, (dx, dy): (f32, f32), held: bool) {
        if self.tool == Tool::Crop {
            if let Some(r) = &mut self.crop_draft {
                r.x += dx;
                r.y += dy;
            }
            return;
        }
        let Some(id) = self.selected else { return };
        let Some(shape) = self.doc.get(id).map(|a| a.shape.clone()) else { return };
        let moved = geom::translate(&shape, dx, dy);
        if held {
            if let Some(a) = self.doc.get_mut(id) {
                a.shape = moved;
            }
            self.changed(true);
        } else {
            self.mutate(|d| {
                if let Some(a) = d.get_mut(id) {
                    a.shape = moved;
                }
            });
        }
    }

    /// Typing into the text annotation being edited. Returns whether the
    /// keystroke was consumed.
    fn text_key(&mut self, k: &Keystroke) -> bool {
        if !is_typing(k) {
            return false;
        }
        match k.key.as_str() {
            "escape" => {
                self.commit_text();
                true
            }
            "backspace" => {
                self.edit_text(|t| {
                    t.pop();
                });
                true
            }
            "enter" => {
                self.edit_text(|t| t.push('\n'));
                true
            }
            "tab" | "left" | "right" | "up" | "down" | "delete" => true,
            _ => match k.key_char.as_deref() {
                Some(c) if !c.chars().any(char::is_control) => {
                    let c = c.to_string();
                    self.edit_text(|t| t.push_str(&c));
                    true
                }
                _ => false,
            },
        }
    }

    pub fn tick_flash(&mut self) -> bool {
        match &self.flash {
            Some((_, at)) if at.elapsed().as_millis() > 1600 => {
                self.flash = None;
                true
            }
            _ => false,
        }
    }

    pub fn drop_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let image = paths.paths().iter().find_map(|p| std::fs::read(p).ok().and_then(|b| decode_image(&b)));
        match image {
            Some(b) if self.tool == Tool::Combine => self.add_layer(b),
            Some(b) => self.add_overlay(b),
            None => self.flash("Not an image"),
        }
        cx.notify();
    }

    pub fn resize_to(&mut self, w: u32, nearest: bool) {
        let Some(base) = self.doc.layers.first().map(|l| l.bitmap.clone()) else { return };
        if w == 0 || w == base.width() {
            return;
        }
        let largest = |f: fn(&Bitmap) -> f32| self.doc.layers.iter().map(|l| f(&l.bitmap)).fold(1., f32::max);
        let side = largest(|b| b.width().max(b.height()) as f32);
        let area = largest(|b| b.width() as f32 * b.height() as f32);
        let k = (w as f32 / base.width() as f32).min(MAX_SIDE / side).min((MAX_AREA / area).sqrt());
        let h = ((base.height() as f32 * k).round() as u32).max(1);
        let layers: Vec<(Bitmap, Pt)> = self
            .doc
            .layers
            .iter()
            .map(|l| {
                let lw = ((l.bitmap.width() as f32 * k).round() as u32).max(1);
                let lh = if l.bitmap.shares_buffer_with(&base) {
                    h
                } else {
                    ((l.bitmap.height() as f32 * k).round() as u32).max(1)
                };
                (resample(&l.bitmap, lw, lh, nearest), Pt::new(l.origin.x * k, l.origin.y * k))
            })
            .collect();
        self.mutate(|d| {
            for (l, (b, o)) in d.layers.iter_mut().zip(layers) {
                l.bitmap = b;
                l.origin = o;
            }
            for a in &mut d.annotations {
                a.shape = geom::scale(&a.shape, k);
                a.style.width *= k;
                a.style.font_size *= k;
                a.style.corner_radius *= k;
            }
            d.crop = d.crop.map(|c| RectF::new(c.x * k, c.y * k, c.w * k, c.h * k));
        });
        self.fit();
    }
}

#[derive(Clone, Copy)]
pub enum Arrange {
    Row,
    Column,
    Grid,
}

fn box_cursor(gx: u8, gy: u8) -> CursorStyle {
    match (gx, gy) {
        (1, _) => CursorStyle::ResizeUpDown,
        (_, 1) => CursorStyle::ResizeLeftRight,
        (0, 0) | (2, 2) => CursorStyle::ResizeUpLeftDownRight,
        _ => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// Whether a keystroke is text input rather than a shortcut. AltGr arrives
/// as Ctrl+Alt with a character on Windows.
pub fn is_typing(k: &Keystroke) -> bool {
    let m = k.modifiers;
    let altgr = m.control && m.alt && k.key_char.is_some();
    !(m.platform || (m.secondary() && !altgr))
}

fn arrow_step(k: &Keystroke) -> Option<(f32, f32)> {
    let m = k.modifiers;
    if m.secondary() || m.alt || m.platform {
        return None;
    }
    let s = if m.shift { 10. } else { 1. };
    match k.key.as_str() {
        "left" => Some((-s, 0.)),
        "right" => Some((s, 0.)),
        "up" => Some((0., -s)),
        "down" => Some((0., s)),
        _ => None,
    }
}

fn fit_aspect(r: RectF, anchor: Pt, p: Pt, aspect: Option<(u32, u32)>) -> RectF {
    let Some(_) = aspect else { return r };
    let a = geom::with_aspect(r, aspect);
    let x = if p.x < anchor.x { anchor.x - a.w } else { anchor.x };
    let y = if p.y < anchor.y { anchor.y - a.h } else { anchor.y };
    RectF::new(x, y, a.w, a.h)
}

/// The shape a drawing gesture describes so far.
pub fn drawn_shape(current: &Shape, anchor: Pt, p: Pt, constrain: bool, centre: bool) -> Shape {
    let rect = || geom::drag_rect(anchor, p, constrain, centre);
    match current {
        Shape::Arrow { kind, .. } => {
            let (from, to) = geom::drag_segment(anchor, p, constrain, centre);
            Shape::Arrow { from, to, control: None, kind: *kind }
        }
        Shape::Line { .. } => {
            let (from, to) = geom::drag_segment(anchor, p, constrain, centre);
            Shape::Line { from, to }
        }
        Shape::Measure { .. } => {
            let (from, to) = geom::drag_segment(anchor, p, constrain, centre);
            Shape::Measure { from, to }
        }
        Shape::Rect { .. } => Shape::Rect { rect: rect() },
        Shape::Oval { .. } => Shape::Oval { rect: rect() },
        Shape::Spotlight { oval, .. } => Shape::Spotlight { rect: rect(), oval: *oval },
        Shape::Redact { mode, block, .. } => Shape::Redact { rect: geom::drag_rect(anchor, p, false, centre), mode: *mode, block: *block },
        Shape::Freehand { points } | Shape::Highlighter { points } => {
            let mut points = points.clone();
            let highlighter = matches!(current, Shape::Highlighter { .. });
            if highlighter && !constrain {
                // Highlighting follows a line of text: hold the anchor's row.
                let end = Pt::new(p.x, anchor.y);
                points = vec![anchor, end];
            } else if constrain && points.len() >= 2 {
                *points.last_mut().unwrap() = p;
            } else if points.last().is_none_or(|l| (l.x - p.x).hypot(l.y - p.y) >= 1.0) {
                points.push(p);
            }
            if highlighter { Shape::Highlighter { points } } else { Shape::Freehand { points } }
        }
        Shape::Magnifier { .. } => {
            let source = geom::drag_rect(anchor, p, constrain, centre);
            let callout = RectF::new(source.x + source.w + 24., source.y - source.h * 2. - 24., source.w * 2., source.h * 2.);
            let side = callout.w.max(callout.h);
            Shape::Magnifier { source, callout: RectF::new(callout.x, callout.y.max(0.), side, side) }
        }
        Shape::Step { number, .. } => Shape::Step { center: p, number: *number },
        Shape::Guide { axis, .. } => match axis {
            Axis::Vertical => Shape::Guide { axis: *axis, at: p.x },
            Axis::Horizontal => Shape::Guide { axis: *axis, at: p.y },
        },
        s => s.clone(),
    }
}

/// Too small to keep: a click without a drag.
pub fn degenerate(shape: &Shape, min: f32) -> bool {
    match shape {
        Shape::Arrow { from, to, .. } | Shape::Line { from, to } | Shape::Measure { from, to } => {
            (to.x - from.x).hypot(to.y - from.y) < min
        }
        Shape::Rect { rect } | Shape::Oval { rect } | Shape::Spotlight { rect, .. } | Shape::Redact { rect, .. } => {
            rect.w < min || rect.h < min
        }
        Shape::Freehand { points } | Shape::Highlighter { points } => {
            geom::points_bounds(points).is_none_or(|r| r.w.max(r.h) < min)
        }
        Shape::Magnifier { source, .. } => source.w < min || source.h < min,
        _ => false,
    }
}

/// The framed output rectangle the backdrop draws around `content`.
pub fn backdrop_frame(content: RectF, b: &Backdrop) -> RectF {
    let mut r = RectF::new(content.x - b.padding, content.y - b.padding, content.w + 2. * b.padding, content.h + 2. * b.padding);
    if let Some((aw, ah)) = b.aspect {
        let a = aw as f32 / ah as f32;
        if r.w / r.h < a {
            let w = r.h * a;
            r.x -= (w - r.w) / 2.;
            r.w = w;
        } else {
            let h = r.w / a;
            r.y -= (h - r.h) / 2.;
            r.h = h;
        }
    }
    r
}

/// The backdrop's gradient over `frame`, running corner to corner like the
/// export, small enough to upload instantly; bilinear sampling keeps it smooth.
pub fn gradient(b: &Backdrop, frame: RectF) -> Bitmap {
    let (fw, fh) = (frame.w.max(1.), frame.h.max(1.));
    let (w, h) = if fw >= fh { (256u32, ((256. * fh / fw).round() as u32).max(1)) } else { (((256. * fw / fh).round() as u32).max(1), 256u32) };
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for j in 0..h {
        for i in 0..w {
            let x = (i as f32 + 0.5) / w as f32 * fw;
            let y = (j as f32 + 0.5) / h as f32 * fh;
            let t = ((x * fw + y * fh) / (fw * fw + fh * fh)).clamp(0., 1.);
            let mix = |a: u8, c: u8| (a as f32 * (1. - t) + c as f32 * t).round() as u8;
            out.extend_from_slice(&[mix(b.from.b, b.to.b), mix(b.from.g, b.to.g), mix(b.from.r, b.to.r), 255]);
        }
    }
    Bitmap::from_bgra(w, h, out)
}

/// Bilinear (or nearest) resample to an exact size.
pub fn resample(b: &Bitmap, w: u32, h: u32, nearest: bool) -> Bitmap {
    if nearest {
        return paint::nearest(b, w, h);
    }
    if w <= b.width() && h <= b.height() && (w as f32 / b.width() as f32 - h as f32 / b.height() as f32).abs() < 0.01 {
        let d = b.downscale_to_fit(w, h);
        if d.width() == w && d.height() == h {
            return d;
        }
    }
    let (sw, sh) = (b.width() as usize, b.height() as usize);
    let src = b.bgra();
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let fy = ((y as f32 + 0.5) * sh as f32 / h as f32 - 0.5).clamp(0., (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..w as usize {
            let fx = ((x as f32 + 0.5) * sw as f32 / w as f32 - 0.5).clamp(0., (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..4 {
                let p = |xx: usize, yy: usize| src[(yy * sw + xx) * 4 + c] as f32;
                let top = p(x0, y0) * (1. - tx) + p(x1, y0) * tx;
                let bottom = p(x0, y1) * (1. - tx) + p(x1, y1) * tx;
                out.push((top * (1. - ty) + bottom * ty).round() as u8);
            }
        }
    }
    Bitmap::from_bgra(w, h, out)
}

#[cfg(test)]
mod tests {
    use super::{backdrop_frame, degenerate, drawn_shape, gradient, is_typing, resample};
    use gpui_kit::{Keystroke, Modifiers};
    use shattr_annotate::{ArrowKind, Backdrop, Pt, RectF, Shape};
    use shattr_core::Bitmap;

    #[test]
    fn a_click_without_drag_is_degenerate() {
        assert!(degenerate(&Shape::Rect { rect: RectF::new(5., 5., 1., 40.) }, 3.));
        assert!(!degenerate(&Shape::Rect { rect: RectF::new(5., 5., 10., 40.) }, 3.));
        assert!(degenerate(&Shape::Arrow { from: Pt::new(0., 0.), to: Pt::new(1., 1.), control: None, kind: ArrowKind::Straight }, 3.));
        assert!(!degenerate(&Shape::Step { center: Pt::new(0., 0.), number: 1 }, 3.));
    }

    #[test]
    fn backdrop_frame_pads_then_meets_the_aspect() {
        let b = Backdrop {
            from: shattr_core::Rgba::opaque(0, 0, 0),
            to: shattr_core::Rgba::opaque(0, 0, 0),
            padding: 10.,
            shadow: 0.,
            radius: 0.,
            aspect: Some((1, 1)),
        };
        let r = backdrop_frame(RectF::new(0., 0., 100., 50.), &b);
        assert_eq!(r, RectF::new(-10., -35., 120., 120.));
    }

    #[test]
    fn highlighter_holds_the_row_unless_shift_is_held() {
        let s = Shape::Highlighter { points: vec![Pt::new(0., 10.)] };
        let d = drawn_shape(&s, Pt::new(0., 10.), Pt::new(50., 14.), false, false);
        assert_eq!(d, Shape::Highlighter { points: vec![Pt::new(0., 10.), Pt::new(50., 10.)] });
    }

    #[test]
    fn resample_hits_the_exact_size_and_keeps_flat_colour() {
        let b = Bitmap::from_bgra(4, 2, [10u8, 20, 30, 255].repeat(8));
        let r = resample(&b, 3, 5, false);
        assert_eq!((r.width(), r.height()), (3, 5));
        assert_eq!(r.pixel(2, 4).unwrap(), shattr_core::Rgba { r: 30, g: 20, b: 10, a: 255 });
    }

    #[test]
    fn gradient_runs_corner_to_corner() {
        let b = Backdrop {
            from: shattr_core::Rgba::opaque(0, 0, 0),
            to: shattr_core::Rgba::opaque(200, 100, 50),
            padding: 0.,
            shadow: 0.,
            radius: 0.,
            aspect: None,
        };
        let g = gradient(&b, RectF::new(10., 10., 400., 100.));
        assert_eq!((g.width(), g.height()), (256, 64));
        let tl = g.pixel(0, 0).unwrap();
        let br = g.pixel(255, 63).unwrap();
        assert!(tl.r < 3 && br.r > 197, "{tl:?} {br:?}");
        let mid = g.pixel(128, 32).unwrap();
        assert!((mid.r as i32 - 100).abs() <= 2, "{mid:?}");
    }

    #[test]
    fn altgr_types_but_ctrl_shortcuts_do_not() {
        let ks = |control: bool, alt: bool, c: Option<&str>| Keystroke {
            modifiers: Modifiers { control, alt, ..Default::default() },
            key: "q".into(),
            key_char: c.map(str::to_string),
        };
        assert!(is_typing(&ks(false, false, Some("q"))));
        assert!(is_typing(&ks(true, true, Some("@"))));
        if !cfg!(target_os = "macos") {
            assert!(!is_typing(&ks(true, false, None)));
        }
    }

    #[test]
    fn a_dragged_step_follows_the_cursor_and_keeps_its_number() {
        let s = Shape::Step { center: Pt::new(1., 1.), number: 4 };
        assert_eq!(drawn_shape(&s, Pt::new(1., 1.), Pt::new(9., 5.), false, false), Shape::Step { center: Pt::new(9., 5.), number: 4 });
    }
}
