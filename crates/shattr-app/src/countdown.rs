//! The delayed-capture countdown. It never takes focus: the point of a delay
//! is to open a menu or hover state first, and stealing focus would close it.

use gpui_kit::*;
use shattr_ui::theme::{DARK as T, MONO_FONT, UI_FONT};

pub const SIZE: f32 = 104.;

pub struct Countdown {
    pub remaining: u32,
}

pub struct CancelCountdown;

impl EventEmitter<CancelCountdown> for Countdown {}

impl Render for Countdown {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("countdown")
            .size_full()
            .rounded(px(14.))
            .bg(T.hud)
            .border_1()
            .border_color(rgba(0xFFFFFF14))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .font_family(UI_FONT)
            .text_color(T.text)
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(CancelCountdown)))
            .child(div().font_family(MONO_FONT).text_size(px(44.)).line_height(px(48.)).font_weight(FontWeight::MEDIUM).child(self.remaining.to_string()))
            .child(div().text_size(px(11.)).text_color(T.text3).child("click to cancel"))
    }
}
