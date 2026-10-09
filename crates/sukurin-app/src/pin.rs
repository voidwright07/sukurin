//! A capture pinned above every other window. Drag to move, scroll to
//! resize, Alt+scroll or 1–0 for opacity, Esc or double-click to close.

use sukurin_ui::icons::Icon;
use sukurin_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use sukurin_ui::widgets::{icon_button, render_image};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use sukurin_core::Shot;
use std::sync::Arc;

pub struct Pin {
    shot: Shot,
    image: Arc<RenderImage>,
    opacity: f32,
    hovered: bool,
    focus: FocusHandle,
}

impl Pin {
    pub fn new(shot: Shot, cx: &mut Context<Self>) -> Self {
        Self { image: render_image(&shot.bitmap), shot, opacity: 1.0, hovered: false, focus: cx.focus_handle() }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Logical size at 100%.
    pub fn natural_size(shot: &Shot) -> Size<Pixels> {
        size(
            px(shot.bitmap.width() as f32 / shot.scale),
            px(shot.bitmap.height() as f32 / shot.scale),
        )
    }

    fn zoom(&self, window: &Window) -> f32 {
        window.bounds().size.width.as_f32() / Self::natural_size(&self.shot).width.as_f32()
    }

    fn scroll(&mut self, ev: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let dy = match ev.delta {
            ScrollDelta::Lines(p) => p.y,
            ScrollDelta::Pixels(p) => p.y.as_f32() / 40.,
        };
        if ev.modifiers.alt {
            self.opacity = (self.opacity + dy * 0.05).clamp(0.1, 1.0);
        } else {
            let natural = Self::natural_size(&self.shot);
            let z = (self.zoom(window) * (1.0 + dy * 0.08)).clamp(0.1, 4.0);
            window.resize(size(natural.width * z, natural.height * z));
        }
        cx.notify();
    }

    fn key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "escape" => window.remove_window(),
            "0" => self.opacity = 1.0,
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => {
                self.opacity = (k.as_bytes()[0] - b'0') as f32 / 10.0;
            }
            "c" if ev.keystroke.modifiers.control || ev.keystroke.modifiers.platform => {
                crate::app::copy_to_clipboard(&self.shot);
            }
            _ => {}
        }
        cx.notify();
    }
}

impl Focusable for Pin {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Pin {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = (self.zoom(window) * 100.).round();
        let shown = self.hovered;
        div()
            .id("pin")
            .size_full()
            .relative()
            .font_family(UI_FONT)
            .text_size(px(12.))
            .text_color(T.text)
            .track_focus(&self.focus)
            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                this.hovered = *h;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, ev, window, cx| this.key(ev, window, cx)))
            .on_scroll_wheel(cx.listener(|this, ev, window, cx| this.scroll(ev, window, cx)))
            .on_mouse_down(MouseButton::Left, |ev, window, _| {
                if ev.click_count >= 2 {
                    window.remove_window();
                } else {
                    window.start_window_move();
                }
            })
            .child(
                img(ImageSource::Render(self.image.clone()))
                    .size_full()
                    .opacity(self.opacity),
            )
            .child(div().absolute().inset_0().border_1().border_color(rgba(0xFFFFFF2E)))
            .when(shown, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .right(px(8.))
                        .flex()
                        .gap(px(2.))
                        .p(px(3.))
                        .rounded(px(9.))
                        .bg(T.hud)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(icon_button("copy", Icon::Copy, "Copy  Ctrl+C", &T).on_click(cx.listener(|this, _, _, _| {
                            crate::app::copy_to_clipboard(&this.shot);
                        })))
                        .child(icon_button("close", Icon::Close, "Close  Esc", &T).on_click(|_, window, _| window.remove_window())),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .bottom(px(8.))
                        .h(px(22.))
                        .px(px(7.))
                        .rounded(px(5.))
                        .bg(T.hud)
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .font_family(MONO_FONT)
                        .text_size(px(11.))
                        .child(format!("{zoom}%"))
                        .child(div().text_color(T.text3).child(format!("α {}%", (self.opacity * 100.).round()))),
                )
            })
    }
}
