//! The post-capture corner card. It never takes focus, so whatever the user
//! was typing into keeps the keyboard.

use shattr_ui::icons::{Icon, icon};
use shattr_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use shattr_ui::widgets::{icon_button, render_image};
use gpui_kit::*;
use shattr_core::Shot;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const WIDTH: f32 = 300.;
const MIN_PREVIEW: f32 = 110.;
const MAX_PREVIEW: f32 = 220.;
pub const LINGER: Duration = Duration::from_secs(6);

#[derive(Clone, Debug)]
pub enum ThumbAction {
    Copy,
    Save,
    Pin,
    Edit,
    SendToAi,
    DragOut,
    Dismiss,
    OpenExisting(PathBuf),
}

/// What the card says under the actions.
#[derive(Clone)]
pub enum Status {
    Copied,
    CopiedForAi,
    Dropped,
    Saved(PathBuf),
    Failed(SharedString),
}

pub struct Duplicate {
    pub path: PathBuf,
    pub exact: bool,
}

pub struct Thumbnail {
    pub shot: Shot,
    preview: Arc<RenderImage>,
    status: Status,
    duplicate: Option<Duplicate>,
    hovered: bool,
    shown_at: Instant,
    press: Option<Point<Pixels>>,
}

impl EventEmitter<ThumbAction> for Thumbnail {}

impl Thumbnail {
    pub fn new(shot: Shot) -> Self {
        let small = shot.bitmap.downscale_to_fit((WIDTH * 2.) as u32, (MAX_PREVIEW * 2.) as u32);
        Self {
            preview: render_image(&small),
            shot,
            status: Status::Copied,
            duplicate: None,
            hovered: false,
            shown_at: Instant::now(),
            press: None,
        }
    }

    pub fn preview_height(shot: &Shot) -> f32 {
        (WIDTH * shot.bitmap.height() as f32 / shot.bitmap.width() as f32).clamp(MIN_PREVIEW, MAX_PREVIEW)
    }

    pub fn window_height(shot: &Shot) -> f32 {
        Self::preview_height(shot) + 2. + 40. + 26.
    }

    pub fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.status = status;
        self.shown_at = Instant::now();
        cx.notify();
    }

    pub fn set_duplicate(&mut self, d: Option<Duplicate>, cx: &mut Context<Self>) {
        self.duplicate = d;
        self.shown_at = Instant::now();
        cx.notify();
    }

    pub fn should_close(&self) -> bool {
        !self.hovered && self.shown_at.elapsed() >= LINGER
    }

    fn remaining(&self) -> f32 {
        if self.hovered {
            return 1.0;
        }
        1.0 - (self.shown_at.elapsed().as_secs_f32() / LINGER.as_secs_f32()).min(1.0)
    }

    fn status_line(&self, cx: &mut Context<Self>) -> AnyElement {
        let row = div().flex().items_center().gap(px(6.)).px(px(12.)).h(px(26.)).text_size(px(12.)).text_color(T.text3);
        if let Some(d) = &self.duplicate {
            let name = d.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let path = d.path.clone();
            let lead = if d.exact { "Already saved as" } else { "Looks like" };
            return row
                .child(div().text_color(T.warn).child(lead))
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(T.text2).child(name))
                .child(
                    div()
                        .id("open-existing")
                        .px(px(6.))
                        .h(px(20.))
                        .rounded(px(4.))
                        .bg(T.control)
                        .text_color(T.text)
                        .flex()
                        .items_center()
                        .hover(|d| d.bg(T.hover))
                        .child("Open")
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(ThumbAction::OpenExisting(path.clone())))),
                )
                .into_any_element();
        }
        let (i, text): (Icon, SharedString) = match &self.status {
            Status::Copied => (Icon::Check, "Copied to clipboard".into()),
            Status::CopiedForAi => (Icon::Ai, "Image and its text copied · paste into the chat".into()),
            Status::Dropped => (Icon::Check, "Dropped as a PNG file".into()),
            Status::Saved(p) => (Icon::Check, format!("Saved {}", p.file_name().unwrap_or_default().to_string_lossy()).into()),
            Status::Failed(e) => (Icon::Close, e.clone()),
        };
        row.child(icon(i).size(px(14.)).text_color(T.text3))
            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(text))
            .into_any_element()
    }
}

impl Render for Thumbnail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.hovered && !self.should_close() {
            window.request_animation_frame();
        }
        let s = self.shot.scale;
        let dims = format!(
            "{} × {}",
            (self.shot.bitmap.width() as f32 / s).round(),
            (self.shot.bitmap.height() as f32 / s).round()
        );
        let action = |id: &'static str, i: Icon, label: &'static str, a: ThumbAction, cx: &mut Context<Self>| {
            icon_button(id, i, label, &T).on_click(cx.listener(move |_, _, _, cx| cx.emit(a.clone())))
        };
        div()
            .id("thumbnail")
            .size_full()
            .flex()
            .flex_col()
            .bg(T.chrome)
            .rounded(px(10.))
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xFFFFFF14))
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(T.text)
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                if !*hovered {
                    this.shown_at = Instant::now();
                }
                cx.notify();
            }))
            .child(
                div()
                    .id("preview")
                    .relative()
                    .w_full()
                    .h(px(Self::preview_height(&self.shot)))
                    .bg(T.canvas)
                    .cursor(CursorStyle::OpenHand)
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _, _| this.press = Some(ev.position)))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.press = None))
                    .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                        if let Some(from) = this.press
                            && ev.pressed_button == Some(MouseButton::Left)
                            && (ev.position - from).magnitude() > 6.
                        {
                            this.press = None;
                            cx.emit(ThumbAction::DragOut);
                        }
                    }))
                    .child(img(ImageSource::Render(self.preview.clone())).size_full().object_fit(ObjectFit::Contain))
                    .child(
                        div()
                            .absolute()
                            .left(px(8.))
                            .top(px(8.))
                            .h(px(20.))
                            .px(px(6.))
                            .rounded(px(4.))
                            .bg(T.hud)
                            .flex()
                            .items_center()
                            .font_family(MONO_FONT)
                            .text_size(px(11.))
                            .child(dims),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(6.))
                            .top(px(6.))
                            .rounded(px(6.))
                            .bg(T.hud)
                            .child(action("dismiss", Icon::Close, "Dismiss", ThumbAction::Dismiss, cx)),
                    ),
            )
            .child(
                div().w_full().h(px(2.)).bg(T.hover).child(div().h_full().w(relative(self.remaining())).bg(T.text3)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .p(px(6.))
                    .child(action("copy", Icon::Copy, "Copy  Ctrl+C", ThumbAction::Copy, cx))
                    .child(action("save", Icon::Save, "Save  Ctrl+S", ThumbAction::Save, cx))
                    .child(action("pin", Icon::Pin, "Pin  Ctrl+P", ThumbAction::Pin, cx))
                    .child(action("ai", Icon::Ai, "Send to AI  Ctrl+Enter", ThumbAction::SendToAi, cx))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("edit")
                            .h(px(28.))
                            .px(px(12.))
                            .rounded(px(6.))
                            .bg(T.control)
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .font_weight(FontWeight::MEDIUM)
                            .hover(|d| d.bg(T.hover))
                            .child("Edit")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ThumbAction::Edit))),
                    ),
            )
            .child(self.status_line(cx))
    }
}
