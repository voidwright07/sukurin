//! Scrolling capture: the HUD shown beside the region while frames are being
//! stitched, and the synthetic wheel input that drives the page.

use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use sukurin_ui::icons::{Icon, icon};
use sukurin_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use sukurin_ui::widgets::kbd;
use std::time::Instant;

pub const WIDTH: f32 = 280.;
pub const HEIGHT: f32 = 236.;
/// Longest stitched result, in logical points.
pub const MAX_POINTS: f32 = 16_000.;
const WARN_AT: f32 = 0.85;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudAction {
    Stop,
    TogglePause,
    Cancel,
}

pub struct ScrollHud {
    scale: f32,
    pub height_px: u32,
    pub frames: u32,
    pub paused: bool,
    pub lost: u32,
    started: Instant,
    focus: FocusHandle,
}

impl EventEmitter<HudAction> for ScrollHud {}

impl ScrollHud {
    pub fn new(scale: f32, cx: &mut Context<Self>) -> Self {
        Self { scale, height_px: 0, frames: 0, paused: false, lost: 0, started: Instant::now(), focus: cx.focus_handle() }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn max_px(&self) -> u32 {
        (MAX_POINTS * self.scale) as u32
    }

    fn fraction(&self) -> f32 {
        (self.height_px as f32 / self.max_px() as f32).min(1.0)
    }

    fn button(id: &'static str, label: &'static str, key: &'static str, primary: bool) -> Stateful<Div> {
        let (bg, fg) = if primary { (T.text, T.chrome) } else { (T.control, T.text) };
        div()
            .id(id)
            .h(px(30.))
            .px(px(10.))
            .rounded(px(6.))
            .bg(bg)
            .text_color(fg)
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .font_weight(FontWeight::MEDIUM)
            .child(label)
            .child(kbd(key).when(primary, |k| k.bg(rgba(0x0000001F)).text_color(fg)))
    }
}

impl Focusable for ScrollHud {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ScrollHud {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.paused {
            window.request_animation_frame();
        }
        let pt = (self.height_px as f32 / self.scale).round() as u32;
        let elapsed = self.started.elapsed().as_secs();
        let near_limit = self.fraction() >= WARN_AT;
        let left_pt = ((self.max_px() - self.height_px.min(self.max_px())) as f32 / self.scale).round();
        let bar = if near_limit { T.warn } else { T.text };
        div()
            .id("scroll-hud")
            .size_full()
            .flex()
            .flex_col()
            .bg(T.chrome)
            .rounded(px(10.))
            .border_1()
            .border_color(rgba(0xFFFFFF14))
            .overflow_hidden()
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(T.text)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                "enter" => cx.emit(HudAction::Stop),
                "space" => cx.emit(HudAction::TogglePause),
                "escape" => cx.emit(HudAction::Cancel),
                _ => {}
            }))
            .child(
                div()
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(T.line)
                    .child(div().size(px(8.)).rounded(px(4.)).when(self.paused, |d| d.border_1().border_color(T.text3)).when(!self.paused, |d| d.bg(T.text)))
                    .child(div().flex_1().font_weight(FontWeight::SEMIBOLD).child(if self.paused { "Scrolling capture · paused" } else { "Scrolling capture" }))
                    .child(div().font_family(MONO_FONT).text_size(px(12.)).text_color(T.text3).child(format!("{:02}:{:02}", elapsed / 60, elapsed % 60))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(11.)).text_color(T.text3).font_weight(FontWeight::MEDIUM).child("HEIGHT"))
                            .child(div().font_family(MONO_FONT).text_size(px(22.)).line_height(px(28.)).child(format!("{pt} pt")))
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(px(11.))
                                    .text_color(T.text3)
                                    .child(format!("{} px · {} frames{}", self.height_px, self.frames, if self.lost > 0 { format!(" · {} skipped", self.lost) } else { String::new() })),
                            ),
                    )
                    .child(
                        div()
                            .h(px(4.))
                            .rounded(px(2.))
                            .bg(T.hover)
                            .overflow_hidden()
                            .child(div().h_full().w(relative(self.fraction())).bg(bar)),
                    )
                    .when(near_limit, |d| {
                        d.child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .items_center()
                                .text_size(px(12.))
                                .text_color(T.warn)
                                .child(icon(Icon::Timer).size(px(14.)).text_color(T.warn))
                                .child(format!("{left_pt} pt left before the {} pt limit", MAX_POINTS as u32)),
                        )
                    })
                    .when(!near_limit, |d| {
                        d.child(div().text_size(px(12.)).text_color(T.text3).child("Auto-scrolling · your scroll wheel works too"))
                    }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .p(px(12.))
                    .border_t_1()
                    .border_color(T.line)
                    .child(Self::button("stop", "Stop", "↵", true).flex_1().on_click(cx.listener(|_, _, _, cx| cx.emit(HudAction::Stop))))
                    .child(
                        Self::button("pause", if self.paused { "Resume" } else { "Pause" }, "space", false)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(HudAction::TogglePause))),
                    )
                    .child(Self::button("cancel", "Cancel", "esc", false).on_click(cx.listener(|_, _, _, cx| cx.emit(HudAction::Cancel)))),
            )
    }
}

/// Scrolls whatever is under the cursor down by `notches` wheel notches.
pub fn wheel_down(notches: i32) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput};
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: (-120 * notches) as u32, dwFlags: MOUSEEVENTF_WHEEL, time: 0, dwExtraInfo: 0 },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
    #[cfg(not(windows))]
    let _ = notches;
}

pub fn move_cursor(x: i32, y: i32) {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
    #[cfg(not(windows))]
    let _ = (x, y);
}
