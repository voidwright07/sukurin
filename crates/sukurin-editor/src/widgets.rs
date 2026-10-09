//! Themed controls from the design: icon buttons, segmented groups, value
//! fields, colour dots, toggles and key caps.

use crate::icons::{EIcon, icon};
use gpui_kit::*;
use sukurin_ui::{MONO_FONT, Theme};

pub fn kbd(t: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .h(px(18.))
        .min_w(px(18.))
        .px(px(4.))
        .rounded(px(4.))
        .bg(t.hover)
        .text_color(t.text2)
        .font_family(MONO_FONT)
        .text_size(px(11.))
        .line_height(px(11.))
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .whitespace_nowrap()
        .child(label.into())
}

pub fn mono(size: f32) -> Div {
    div().font_family(MONO_FONT).text_size(px(size)).whitespace_nowrap()
}

pub fn sep(t: &Theme) -> Div {
    div().w(px(1.)).h(px(16.)).bg(t.line2).flex_none()
}

pub fn label(t: &Theme, text: &'static str) -> Div {
    div().text_size(px(11.)).line_height(px(14.)).text_color(t.text3).font_weight(FontWeight::MEDIUM).child(text.to_uppercase())
}

/// 28 px icon button; `on` paints it in the accent (active tool only).
pub fn icon_button(t: &Theme, id: impl Into<ElementId>, i: EIcon, on: bool) -> Stateful<Div> {
    let id: ElementId = id.into();
    let group: SharedString = format!("ib-{id}").into();
    let (fg, hover_fg) = if on { (t.accent_ink, t.accent_ink) } else { (t.text2, t.text) };
    let hover_bg = if on { t.accent } else { t.hover };
    let mut d = div()
        .id(id)
        .group(group.clone())
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .hover(move |s| s.bg(hover_bg))
        .child(icon(i).text_color(fg).group_hover(group, move |s| s.text_color(hover_fg)));
    if on {
        d = d.bg(t.accent);
    }
    d
}

pub fn disabled_icon(t: &Theme, id: impl Into<ElementId>, i: EIcon) -> Stateful<Div> {
    div().id(id).size(px(28.)).flex().items_center().justify_center().flex_none().child(icon(i).text_color(t.line2))
}

/// A segmented group; the caller appends `seg_item`s.
pub fn seg(t: &Theme) -> Div {
    div().flex().flex_none().p(px(2.)).gap(px(1.)).rounded(px(7.)).bg(t.control)
}

pub fn seg_item(t: &Theme, id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    let hover = t.text2;
    let mut d = div()
        .id(id)
        .h(px(24.))
        .min_w(px(26.))
        .px(px(6.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .text_size(px(12.))
        .whitespace_nowrap();
    if selected {
        d = d.bg(t.hover).text_color(t.text);
    } else {
        d = d.text_color(t.text3).hover(move |s| s.text_color(hover));
    }
    d
}

pub fn seg_icon(t: &Theme, i: EIcon, selected: bool) -> Svg {
    icon(i).text_color(if selected { t.text } else { t.text3 })
}

/// A labelled numeric field ("W 3"). Click to step up, shift-click or
/// scroll to step either way.
pub fn field(t: &Theme, id: impl Into<ElementId>, k: &'static str, value: String) -> Stateful<Div> {
    let line2 = t.line2;
    div()
        .id(id)
        .h(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .rounded(px(6.))
        .bg(t.control)
        .border_1()
        .border_color(transparent_black())
        .hover(move |s| s.border_color(line2))
        .when_some(Some(k).filter(|k| !k.is_empty()), |d, k| d.child(div().text_size(px(11.)).text_color(t.text3).child(k)))
        .child(mono(12.).text_color(t.text).child(value))
}

/// A field-styled text button ("Trim to content ⇧T").
pub fn field_button(t: &Theme, id: impl Into<ElementId>, text: &'static str, key: Option<String>) -> Stateful<Div> {
    let line2 = t.line2;
    div()
        .id(id)
        .h(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .rounded(px(6.))
        .bg(t.control)
        .text_color(t.text)
        .border_1()
        .border_color(transparent_black())
        .hover(move |s| s.border_color(line2))
        .whitespace_nowrap()
        .child(text)
        .when_some(key, |d, k| d.child(kbd(t, k)))
}

pub fn dot(t: &Theme, id: impl Into<ElementId>, fill: impl Into<Fill>, selected: bool) -> Stateful<Div> {
    let ring = t.text;
    let gap = t.chrome;
    let d = div().id(id).size(px(16.)).rounded_full().flex_none().bg(fill).border_1().border_color(rgba(0x7F7F7F59));
    if selected {
        div()
            .id("ring")
            .size(px(22.))
            .m(px(-3.))
            .rounded_full()
            .border(px(1.5))
            .border_color(ring)
            .bg(gap)
            .flex()
            .items_center()
            .justify_center()
            .child(d)
    } else {
        d
    }
}

/// The small switch with a label and optional key cap.
pub fn toggle(t: &Theme, id: impl Into<ElementId>, on: bool, text: &'static str, key: Option<String>) -> Stateful<Div> {
    let track = if on { t.text } else { t.hover };
    let knob = if on { t.chrome } else { t.text3 };
    let hover = t.control;
    div()
        .id(id)
        .h(px(28.))
        .px(px(6.))
        .rounded(px(6.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .text_color(t.text2)
        .hover(move |s| s.bg(hover))
        .whitespace_nowrap()
        .child(
            div()
                .w(px(24.))
                .h(px(14.))
                .rounded(px(7.))
                .bg(track)
                .relative()
                .flex_none()
                .child(div().absolute().top(px(2.)).left(px(if on { 12. } else { 2. })).size(px(10.)).rounded(px(5.)).bg(knob)),
        )
        .child(text)
        .when_some(key, |d, k| d.child(kbd(t, k)))
}

pub fn panel(t: &Theme, dark: bool) -> Div {
    let ring = if dark { rgba(0xFFFFFF0F) } else { rgba(0x14151814) };
    div()
        .bg(t.chrome)
        .rounded(px(10.))
        .border_1()
        .border_color(ring)
        .shadow(vec![BoxShadow {
            color: if dark { hsla(0., 0., 0., 0.45) } else { hsla(0., 0., 0.08, 0.14) },
            offset: point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
}

use gpui_kit::prelude::FluentBuilder as _;
