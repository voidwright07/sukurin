use crate::icons::{Icon, icon};
use crate::theme::{DARK as T, MONO_FONT, Theme};
use gpui_kit::*;
use shattr_core::Bitmap;
use std::sync::Arc;

/// GPUI consumes BGRA, which is how `Bitmap` already stores pixels.
pub fn render_image(b: &Bitmap) -> Arc<RenderImage> {
    let buf = image::RgbaImage::from_raw(b.width(), b.height(), b.bgra().to_vec()).expect("bitmap size is consistent");
    Arc::new(RenderImage::new(vec![image::Frame::new(buf)]))
}

pub fn kbd(label: impl Into<SharedString>) -> Div {
    div()
        .h(px(18.))
        .min_w(px(18.))
        .px(px(4.))
        .rounded(px(4.))
        .bg(T.hover)
        .text_color(T.text2)
        .font_family(MONO_FONT)
        .text_size(px(11.))
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .child(label.into())
}

/// A floating readout chip, positioned absolutely by the caller.
pub fn pill() -> Div {
    div()
        .absolute()
        .h(px(24.))
        .px(px(8.))
        .rounded(px(6.))
        .bg(T.hud)
        .border_1()
        .border_color(rgba(0xFFFFFF14))
        .flex()
        .items_center()
        .gap(px(8.))
        .whitespace_nowrap()
}

/// SVGs don't inherit text colour in GPUI, so the icon is tinted explicitly
/// and re-tinted through a group hover.
pub fn icon_button(id: impl Into<ElementId>, i: Icon, label: &'static str, t: &Theme) -> Stateful<Div> {
    let hover = t.hover;
    let fg = t.text;
    let group: SharedString = format!("icon-button-{label}").into();
    div()
        .id(id.into())
        .group(group.clone())
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .hover(move |d| d.bg(hover))
        .tooltip(move |_, cx| cx.new(|_| Tip(label)).into())
        .child(icon(i).text_color(t.text2).group_hover(group, move |s| s.text_color(fg)))
}

struct Tip(&'static str);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(T.hover)
            .text_color(T.text)
            .text_size(px(12.))
            .child(self.0)
    }
}
