//! The capture overlay: a frozen copy of one display, shown full screen,
//! where the user picks what to keep. Geometry is kept in frame-relative
//! physical pixels and converted to logical points only when drawn.

use crate::capture::SnapWindow;
use sukurin_ui::icons::{Icon, icon};
use sukurin_ui::widgets::{kbd, pill};
use sukurin_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use sukurin_core::{Frame, PointPx, RectPx};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Area,
    Window,
    Screen,
    Scrolling,
    Repeat,
    Delay,
    Text,
}

struct ModeSpec {
    mode: Mode,
    label: &'static str,
    key: &'static str,
    icon: Icon,
    hint: &'static str,
    ready: bool,
}

const MODES: [ModeSpec; 7] = [
    ModeSpec { mode: Mode::Area, label: "Area", key: "1", icon: Icon::Area, hint: "Drag a region or click a window · hold Ctrl to edit", ready: true },
    ModeSpec { mode: Mode::Window, label: "Window", key: "2", icon: Icon::Window, hint: "Click a window", ready: true },
    ModeSpec { mode: Mode::Screen, label: "Screen", key: "3", icon: Icon::Screen, hint: "Click or press ↵", ready: true },
    ModeSpec { mode: Mode::Scrolling, label: "Scrolling", key: "4", icon: Icon::Scrolling, hint: "Drag the scrolling region", ready: true },
    ModeSpec { mode: Mode::Repeat, label: "Last area", key: "5", icon: Icon::Repeat, hint: "Adjust or press ↵", ready: true },
    ModeSpec { mode: Mode::Delay, label: "3 s", key: "6", icon: Icon::Timer, hint: "Capturing in 3 s", ready: true },
    ModeSpec { mode: Mode::Text, label: "Text", key: "7", icon: Icon::Text, hint: "Drag over text · copies what it reads", ready: true },
];

fn spec(mode: Mode) -> &'static ModeSpec {
    MODES.iter().find(|s| s.mode == mode).unwrap()
}

/// What happens to a region once it is captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Thumbnail,
    Edit,
    Recognize,
    Scroll,
}

pub enum OverlayEvent {
    Capture { rect: RectPx, after: After },
    Delay,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
}

impl Handle {
    const ALL: [Handle; 8] = [Handle::NW, Handle::N, Handle::NE, Handle::E, Handle::SE, Handle::S, Handle::SW, Handle::W];

    /// Unit offsets of the handle on the rect: 0 = left/top, 1 = middle, 2 = right/bottom.
    fn grid(self) -> (i32, i32) {
        match self {
            Handle::NW => (0, 0),
            Handle::N => (1, 0),
            Handle::NE => (2, 0),
            Handle::E => (2, 1),
            Handle::SE => (2, 2),
            Handle::S => (1, 2),
            Handle::SW => (0, 2),
            Handle::W => (0, 1),
        }
    }

    fn position(self, r: RectPx) -> PointPx {
        let (gx, gy) = self.grid();
        PointPx::new(r.x + gx * r.w as i32 / 2, r.y + gy * r.h as i32 / 2)
    }

    fn resize(self, start: RectPx, to: PointPx) -> RectPx {
        let (gx, gy) = self.grid();
        let (mut x0, mut y0, mut x1, mut y1) = (start.x, start.y, start.right() - 1, start.bottom() - 1);
        match gx {
            0 => x0 = to.x,
            2 => x1 = to.x,
            _ => {}
        }
        match gy {
            0 => y0 = to.y,
            2 => y1 = to.y,
            _ => {}
        }
        RectPx::from_corners(PointPx::new(x0, y0), PointPx::new(x1, y1))
    }

    fn cursor(self) -> CursorStyle {
        match self {
            Handle::N | Handle::S => CursorStyle::ResizeUpDown,
            Handle::E | Handle::W => CursorStyle::ResizeLeftRight,
            Handle::NW | Handle::SE => CursorStyle::ResizeUpLeftDownRight,
            Handle::NE | Handle::SW => CursorStyle::ResizeUpRightDownLeft,
        }
    }
}

#[derive(Clone, Copy)]
enum Drag {
    Move { from: PointPx, start: RectPx },
    Resize { handle: Handle, start: RectPx },
}

#[derive(Clone, Copy)]
enum Gesture {
    Idle,
    Drawing { anchor: PointPx },
    Selected { rect: RectPx, drag: Option<Drag> },
}

pub struct Overlay {
    frame: Frame,
    image: Arc<RenderImage>,
    windows: Vec<SnapWindow>,
    mode: Mode,
    gesture: Gesture,
    cursor: PointPx,
    last_area: Option<RectPx>,
    focus: FocusHandle,
    copied: Option<String>,
    /// The pixel magnifier is off by default and toggled with Z, so a plain
    /// capture shows only the crosshair and size, like the OS tools.
    loupe: bool,
}

impl EventEmitter<OverlayEvent> for Overlay {}

const LOUPE_CELLS: i32 = 11;
const LOUPE_CELL: f32 = 12.;
const HANDLE_GRAB: i32 = 10;

impl Overlay {
    /// `windows` and `last_area` are in virtual-desktop pixels.
    pub fn new(
        frame: Frame,
        windows: Vec<SnapWindow>,
        last_area: Option<RectPx>,
        cursor: PointPx,
        mode: Mode,
        cx: &mut Context<Self>,
    ) -> Self {
        let origin = frame.display.rect;
        let bounds = frame.bitmap.bounds();
        let to_frame = |r: RectPx| r.translate(-origin.x, -origin.y).intersect(&bounds);
        let windows = windows
            .into_iter()
            .filter_map(|w| Some(SnapWindow { rect: to_frame(w.rect)?, title: w.title }))
            .collect();
        let last_area = last_area.and_then(to_frame);
        let image = sukurin_ui::render_image(&frame.bitmap);
        let mut this = Self {
            frame,
            image,
            windows,
            mode: Mode::Area,
            gesture: Gesture::Idle,
            cursor: PointPx::new(cursor.x - origin.x, cursor.y - origin.y),
            last_area,
            focus: cx.focus_handle(),
            copied: None,
            loupe: false,
        };
        this.set_mode(mode, cx);
        this
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    fn scale(&self) -> f32 {
        self.frame.display.scale
    }

    fn lp(&self, v: i32) -> Pixels {
        px(v as f32 / self.scale())
    }

    fn to_px(&self, p: Point<Pixels>) -> PointPx {
        let b = self.frame.bitmap.bounds();
        PointPx::new(
            ((p.x.as_f32() * self.scale()).floor() as i32).clamp(0, b.w as i32 - 1),
            ((p.y.as_f32() * self.scale()).floor() as i32).clamp(0, b.h as i32 - 1),
        )
    }

    fn hovered_window(&self) -> Option<&SnapWindow> {
        self.windows.iter().find(|w| w.rect.contains(self.cursor))
    }

    fn click_target(&self) -> RectPx {
        self.hovered_window().map(|w| w.rect).unwrap_or(self.frame.bitmap.bounds())
    }

    /// The region that will be captured if the user commits now.
    fn active_rect(&self) -> Option<RectPx> {
        match self.gesture {
            Gesture::Drawing { anchor } => Some(RectPx::from_corners(anchor, self.cursor)),
            Gesture::Selected { rect, .. } => Some(rect),
            Gesture::Idle => match self.mode {
                Mode::Screen => Some(self.frame.bitmap.bounds()),
                Mode::Area | Mode::Window => self.hovered_window().map(|w| w.rect),
                _ => None,
            },
        }
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if !spec(mode).ready {
            return;
        }
        self.mode = mode;
        self.gesture = Gesture::Idle;
        match mode {
            Mode::Repeat => {
                if let Some(rect) = self.last_area {
                    self.gesture = Gesture::Selected { rect, drag: None };
                }
            }
            Mode::Delay => cx.emit(OverlayEvent::Delay),
            _ => {}
        }
        cx.notify();
    }

    fn commit(&mut self, rect: RectPx, then_edit: bool, cx: &mut Context<Self>) {
        if let Some(rect) = rect.intersect(&self.frame.bitmap.bounds()) {
            let after = match (self.mode, then_edit) {
                (Mode::Text, _) => After::Recognize,
                (Mode::Scrolling, _) => After::Scroll,
                (_, true) => After::Edit,
                _ => After::Thumbnail,
            };
            cx.emit(OverlayEvent::Capture { rect, after });
        }
    }

    fn handle_at(&self, rect: RectPx, p: PointPx) -> Option<Handle> {
        let grab = (HANDLE_GRAB as f32 * self.scale()) as i32;
        Handle::ALL.into_iter().find(|h| {
            let c = h.position(rect);
            (c.x - p.x).abs() <= grab && (c.y - p.y).abs() <= grab
        })
    }

    fn mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let p = self.to_px(ev.position);
        self.cursor = p;
        match self.mode {
            Mode::Window => {
                if let Some(w) = self.hovered_window() {
                    let r = w.rect;
                    self.commit(r, false, cx);
                }
            }
            Mode::Screen => self.commit(self.frame.bitmap.bounds(), false, cx),
            _ => {
                self.gesture = match self.gesture {
                    Gesture::Selected { rect, .. } => {
                        if let Some(handle) = self.handle_at(rect, p) {
                            Gesture::Selected { rect, drag: Some(Drag::Resize { handle, start: rect }) }
                        } else if rect.contains(p) {
                            if ev.click_count >= 2 {
                                self.commit(rect, false, cx);
                                return;
                            }
                            Gesture::Selected { rect, drag: Some(Drag::Move { from: p, start: rect }) }
                        } else {
                            Gesture::Drawing { anchor: p }
                        }
                    }
                    _ => Gesture::Drawing { anchor: p },
                };
            }
        }
        cx.notify();
    }

    fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let p = self.to_px(ev.position);
        self.cursor = p;
        if let Gesture::Selected { drag: Some(drag), .. } = self.gesture {
            let bounds = self.frame.bitmap.bounds();
            let rect = match drag {
                Drag::Move { from, start } => {
                    let dx = (p.x - from.x).clamp(-start.x, bounds.right() - start.right());
                    let dy = (p.y - from.y).clamp(-start.y, bounds.bottom() - start.bottom());
                    start.translate(dx, dy)
                }
                Drag::Resize { handle, start } => handle.resize(start, p),
            };
            self.gesture = Gesture::Selected { rect, drag: Some(drag) };
        }
        cx.notify();
    }

    fn mouse_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        let p = self.to_px(ev.position);
        self.cursor = p;
        match self.gesture {
            // Releasing a drag captures straight away; holding Ctrl or ⌘
            // while releasing opens the capture in the editor instead.
            Gesture::Drawing { anchor } => {
                let rect = RectPx::from_corners(anchor, p);
                let click = (rect.w as f32) < 4. * self.scale() && (rect.h as f32) < 4. * self.scale();
                let then_edit = ev.modifiers.control || ev.modifiers.platform;
                self.gesture = Gesture::Idle;
                self.commit(if click { self.click_target() } else { rect }, then_edit, cx);
            }
            Gesture::Selected { rect, .. } => self.gesture = Gesture::Selected { rect, drag: None },
            Gesture::Idle => {}
        }
        cx.notify();
    }

    fn key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let step = if k.modifiers.shift { 10 } else { 1 };
        match k.key.as_str() {
            "escape" => cx.emit(OverlayEvent::Cancel),
            "enter" => {
                let then_edit = k.modifiers.control || k.modifiers.platform;
                match self.gesture {
                    Gesture::Selected { rect, .. } => self.commit(rect, then_edit, cx),
                    _ => self.commit(self.click_target(), then_edit, cx),
                }
            }
            "tab" => {
                if let Gesture::Selected { rect, .. } = self.gesture {
                    let tight = self.frame.bitmap.trim_flat_margins(rect, 6);
                    self.gesture = Gesture::Selected { rect: tight, drag: None };
                }
            }
            "left" | "right" | "up" | "down" => {
                if let Gesture::Selected { rect, drag: None } = self.gesture {
                    let (dx, dy) = match k.key.as_str() {
                        "left" => (-step, 0),
                        "right" => (step, 0),
                        "up" => (0, -step),
                        _ => (0, step),
                    };
                    let moved = rect.translate(dx, dy);
                    if moved.intersect(&self.frame.bitmap.bounds()) == Some(moved) {
                        self.gesture = Gesture::Selected { rect: moved, drag: None };
                    }
                }
            }
            "z" => self.loupe = !self.loupe,
            "c" => {
                if let Some(c) = self.frame.bitmap.pixel(self.cursor.x as u32, self.cursor.y as u32) {
                    cx.write_to_clipboard(ClipboardItem::new_string(c.hex()));
                    self.copied = Some(c.hex());
                }
            }
            key => {
                if let Some(s) = MODES.iter().find(|s| s.key == key) {
                    self.set_mode(s.mode, cx);
                }
            }
        }
        let _ = window;
        cx.notify();
    }

    fn cursor_style(&self) -> CursorStyle {
        match (self.mode, self.gesture) {
            (Mode::Window | Mode::Screen, _) => CursorStyle::PointingHand,
            (_, Gesture::Selected { drag: Some(Drag::Move { .. }), .. }) => CursorStyle::ClosedHand,
            (_, Gesture::Selected { drag: Some(Drag::Resize { handle, .. }), .. }) => handle.cursor(),
            (_, Gesture::Selected { rect, drag: None }) => match self.handle_at(rect, self.cursor) {
                Some(h) => h.cursor(),
                None if rect.contains(self.cursor) => CursorStyle::OpenHand,
                None => CursorStyle::Crosshair,
            },
            _ => CursorStyle::Crosshair,
        }
    }

    fn rect_div(&self, r: RectPx) -> Div {
        div().absolute().left(self.lp(r.x)).top(self.lp(r.y)).w(self.lp(r.w as i32)).h(self.lp(r.h as i32))
    }

    fn dim(&self, active: Option<RectPx>) -> Vec<AnyElement> {
        let b = self.frame.bitmap.bounds();
        let Some(r) = active else {
            return vec![self.rect_div(b).bg(T.scrim).into_any_element()];
        };
        [
            RectPx::new(0, 0, b.w, r.y as u32),
            RectPx::new(0, r.bottom(), b.w, (b.h as i32 - r.bottom()).max(0) as u32),
            RectPx::new(0, r.y, r.x as u32, r.h),
            RectPx::new(r.right(), r.y, (b.w as i32 - r.right()).max(0) as u32, r.h),
        ]
        .into_iter()
        .filter(|d| !d.is_empty())
        .map(|d| self.rect_div(d).bg(T.scrim).into_any_element())
        .collect()
    }

    fn size_label(&self, r: RectPx) -> Div {
        let s = self.scale();
        let pt = format!("{} × {}", (r.w as f32 / s).round(), (r.h as f32 / s).round());
        let phys = format!("{} × {} px", r.w, r.h);
        let mut row = div().flex().items_center().gap(px(8.)).child(
            div().font_family(MONO_FONT).text_size(px(12.)).text_color(T.text).child(pt),
        );
        if (s - 1.).abs() > f32::EPSILON {
            row = row.child(div().font_family(MONO_FONT).text_size(px(11.)).text_color(T.text3).child(phys));
        }
        row
    }

    fn selection(&self) -> Vec<AnyElement> {
        let mut out = Vec::new();
        match self.gesture {
            Gesture::Idle => {
                if let (Some(w), Mode::Area | Mode::Window) = (self.hovered_window(), self.mode) {
                    let border = if self.mode == Mode::Window { px(2.) } else { px(1.) };
                    out.push(self.rect_div(w.rect).border(border).border_color(T.accent).into_any_element());
                    let title: String = w.title.chars().take(48).collect();
                    out.push(
                        pill()
                            .left(self.lp(w.rect.x))
                            .top(self.lp(w.rect.y) + px(6.))
                            .ml(px(6.))
                            .bg(T.accent)
                            .text_color(T.accent_ink)
                            .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child(title))
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(px(12.))
                                    .child(format!("{} × {}", w.rect.w, w.rect.h)),
                            )
                            .into_any_element(),
                    );
                }
            }
            Gesture::Drawing { anchor } => {
                let r = RectPx::from_corners(anchor, self.cursor);
                out.push(self.rect_div(r).border_1().border_color(T.accent).into_any_element());
                out.push(self.size_pill(r));
            }
            Gesture::Selected { rect, drag } => {
                out.push(self.rect_div(rect).border_1().border_color(T.accent).into_any_element());
                let tight = self.frame.bitmap.trim_flat_margins(rect, 6);
                if tight != rect && drag.is_none() {
                    out.push(self.rect_div(tight).border_1().border_dashed().border_color(T.accent).into_any_element());
                    out.push(
                        pill()
                            .left(self.lp(tight.x))
                            .top(self.lp(tight.y) - px(30.))
                            .bg(T.accent)
                            .text_color(T.accent_ink)
                            .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child("Auto-adjust"))
                            .child(
                                div().font_family(MONO_FONT).text_size(px(12.)).child(format!("{} × {}", tight.w, tight.h)),
                            )
                            .child(kbd("⇥").bg(rgba(0x00000024)).text_color(T.accent_ink))
                            .into_any_element(),
                    );
                }
                for h in Handle::ALL {
                    let c = h.position(rect);
                    out.push(
                        div()
                            .absolute()
                            .left(self.lp(c.x) - px(4.))
                            .top(self.lp(c.y) - px(4.))
                            .size(px(8.))
                            .bg(rgb(0xFFFFFF))
                            .border(px(1.5))
                            .border_color(T.accent)
                            .into_any_element(),
                    );
                }
                out.push(self.size_pill(rect));
            }
        }
        out
    }

    fn size_pill(&self, r: RectPx) -> AnyElement {
        let below = self.lp(r.bottom()) + px(8.);
        let top = if below.as_f32() + 32. > self.lp(self.frame.bitmap.height() as i32).as_f32() {
            self.lp(r.bottom()) - px(32.)
        } else {
            below
        };
        pill().left(self.lp(r.x)).top(top).child(self.size_label(r)).into_any_element()
    }

    fn crosshair(&self) -> Vec<AnyElement> {
        let b = self.frame.bitmap.bounds();
        let line = rgba(0xFFFFFF52);
        vec![
            div().absolute().left_0().top(self.lp(self.cursor.y)).w(self.lp(b.w as i32)).h(px(1.)).bg(line).into_any_element(),
            div().absolute().top_0().left(self.lp(self.cursor.x)).h(self.lp(b.h as i32)).w(px(1.)).bg(line).into_any_element(),
        ]
    }

    fn loupe(&self) -> AnyElement {
        let half = LOUPE_CELLS / 2;
        let size = LOUPE_CELL * LOUPE_CELLS as f32;
        let mut grid = div().flex().flex_wrap().w(px(size)).h(px(size)).relative();
        for dy in -half..=half {
            for dx in -half..=half {
                let (x, y) = (self.cursor.x + dx, self.cursor.y + dy);
                let color = (x >= 0 && y >= 0)
                    .then(|| self.frame.bitmap.pixel(x as u32, y as u32))
                    .flatten()
                    .map(|c| rgb(c.u32_rgb()))
                    .unwrap_or(rgb(0x000000));
                grid = grid.child(
                    div()
                        .w(px(LOUPE_CELL))
                        .h(px(LOUPE_CELL))
                        .bg(color)
                        .border_r(px(0.5))
                        .border_b(px(0.5))
                        .border_color(rgba(0x00000014)),
                );
            }
        }
        grid = grid.child(
            div()
                .absolute()
                .left(px(half as f32 * LOUPE_CELL))
                .top(px(half as f32 * LOUPE_CELL))
                .size(px(LOUPE_CELL))
                .border_1()
                .border_color(rgb(0x000000))
                .child(div().size_full().border_1().border_color(rgb(0xFFFFFF))),
        );
        let under = self.frame.bitmap.pixel(self.cursor.x as u32, self.cursor.y as u32);
        let hex = under.map(|c| c.hex()).unwrap_or_default();
        let copied = self.copied.as_deref() == Some(hex.as_str());
        let readout = div()
            .bg(T.hud)
            .px(px(8.))
            .py(px(6.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .font_family(MONO_FONT)
                    .text_size(px(12.))
                    .text_color(T.text)
                    .child(format!("{}, {}", self.cursor.x, self.cursor.y))
                    .child(div().text_color(T.text3).text_size(px(11.)).child("px")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(10.)).rounded(px(2.)).bg(under.map(|c| rgb(c.u32_rgb())).unwrap_or(rgb(0))))
                    .child(div().flex_1().font_family(MONO_FONT).text_size(px(12.)).text_color(T.text).child(hex))
                    .child(if copied { kbd("copied") } else { kbd("C") }),
            );

        let s = self.scale();
        let (cx_l, cy_l) = (self.cursor.x as f32 / s, self.cursor.y as f32 / s);
        let (w_l, h_l) = (self.frame.bitmap.width() as f32 / s, self.frame.bitmap.height() as f32 / s);
        let total_h = size + 44.;
        let left = if cx_l + 24. + size > w_l { cx_l - 24. - size } else { cx_l + 24. };
        let top = if cy_l + 24. + total_h > h_l { cy_l - 24. - total_h } else { cy_l + 24. };
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(size))
            .rounded(px(8.))
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xFFFFFF24))
            .shadow_lg()
            .child(grid)
            .child(readout)
            .into_any_element()
    }

    fn mode_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut bar = div()
            .id("mode-bar")
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(4.))
            .rounded(px(10.))
            .bg(T.chrome)
            .border_1()
            .border_color(rgba(0xFFFFFF10))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (i, s) in MODES.iter().enumerate() {
            let on = s.mode == self.mode;
            let mode = s.mode;
            let (fg, sub) = match (on, s.ready) {
                (true, _) => (T.accent_ink, T.accent_ink),
                (false, true) => (T.text2, T.text3),
                (false, false) => (T.line2, T.line2),
            };
            let mut b = div()
                .id(("mode", i))
                .h(px(32.))
                .px(px(9.))
                .rounded(px(7.))
                .flex()
                .items_center()
                .gap(px(7.))
                .text_color(fg)
                .font_weight(FontWeight::MEDIUM)
                .child(icon(s.icon).text_color(fg))
                .child(s.label)
                .child(div().font_family(MONO_FONT).text_size(px(11.)).text_color(sub).child(s.key));
            if on {
                b = b.bg(T.accent);
            } else if s.ready {
                b = b.hover(|d| d.bg(T.hover).text_color(T.text)).on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)));
            }
            bar = bar.child(b);
        }
        bar.child(div().w(px(1.)).h(px(18.)).mx(px(6.)).bg(T.line2))
            .child(
                div()
                    .id("cancel")
                    .px(px(6.))
                    .child(kbd("esc"))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(OverlayEvent::Cancel))),
            )
    }

    fn hint_bar(&self) -> impl IntoElement {
        let item = |k: &'static str, label: &'static str| {
            div().flex().items_center().gap(px(5.)).child(kbd(k)).child(label)
        };
        let mut bar = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .h(px(30.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(T.hud)
            .text_size(px(12.))
            .text_color(T.text2)
            .child(spec(self.mode).hint);
        if matches!(self.gesture, Gesture::Selected { .. }) {
            bar = bar.child(item("⇥", "auto-adjust")).child(item("←→", "nudge")).child(item("↵", "capture"));
        } else {
            bar = bar.child(item("1–7", "mode")).child(item("Z", if self.loupe { "hide magnifier" } else { "magnifier" }));
        }
        bar.child(item("esc", "cancel"))
    }
}

impl Focusable for Overlay {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Overlay {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active_rect();
        let aiming = matches!(self.mode, Mode::Area | Mode::Repeat | Mode::Text)
            && matches!(
                self.gesture,
                Gesture::Idle | Gesture::Drawing { .. } | Gesture::Selected { drag: Some(Drag::Resize { .. }), .. }
            );
        let show_loupe = self.loupe && aiming;
        let show_crosshair = aiming && !matches!(self.gesture, Gesture::Selected { .. });

        div()
            .id("overlay")
            .size_full()
            .relative()
            .overflow_hidden()
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(T.text)
            .track_focus(&self.focus)
            .cursor(self.cursor_style())
            .on_key_down(cx.listener(|this, ev, window, cx| this.key_down(ev, window, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev, _, cx| this.mouse_down(ev, cx)))
            .on_mouse_move(cx.listener(|this, ev, _, cx| this.mouse_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev, _, cx| this.mouse_up(ev, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| cx.emit(OverlayEvent::Cancel)))
            .child(img(ImageSource::Render(self.image.clone())).absolute().top_0().left_0().size_full())
            .children(self.dim(active))
            .children(self.selection())
            .when(show_crosshair, |d| d.children(self.crosshair()))
            .when(show_loupe, |d| d.child(self.loupe()))
            .child(div().absolute().top(px(16.)).left_0().right_0().flex().justify_center().child(self.mode_bar(cx)))
            .child(div().absolute().bottom(px(20.)).left_0().right_0().flex().justify_center().child(self.hint_bar()))
    }
}
