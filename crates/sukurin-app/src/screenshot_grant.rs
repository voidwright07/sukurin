//! GNOME shows "Allow Apps to Take Screenshots?" only for the app whose
//! window is focused (gnome-shell's AccessDialog), which a tray click never
//! is. So a focused Sukurin window asks instead: one silent portal request,
//! whose screenshot is thrown away. The permission GNOME stores is the point.

use crate::capture::portal;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sukurin_ui::theme::DARK as T;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Grant {
    #[default]
    Unasked,
    Asking,
    Granted,
    Refused,
}

impl Grant {
    /// Sends the request from the window that owns `cx`; `done` gets the answer.
    pub fn ask<V: 'static>(cx: &mut Context<V>, done: impl Fn(&mut V, Grant, &mut Context<V>) + 'static) {
        cx.spawn(async move |this, cx| {
            let answer = match cx.background_executor().spawn(portal::screenshot(false)).await {
                Ok(Some(_)) => Grant::Granted,
                Ok(None) => Grant::Refused,
                Err(e) => {
                    log::error!("screenshot permission request failed: {e:#}");
                    Grant::Refused
                }
            };
            log::info!("screenshot permission: {}", if answer == Grant::Granted { "granted" } else { "refused" });
            let _ = this.update(cx, |this, cx| done(this, answer, cx));
        })
        .detach();
    }
}

/// The "Allow screenshots" row for a window that is in front, with what GNOME answered.
pub fn section(grant: Grant, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Div {
    let label = match grant {
        Grant::Unasked | Grant::Refused => "Allow screenshots",
        Grant::Asking => "Waiting for GNOME…",
        Grant::Granted => "Allowed",
    };
    let status = match grant {
        Grant::Granted => Some(("Screenshots allowed. Captures from the tray now open Sukurin's overlay.", T.text2)),
        Grant::Refused => Some((
            "GNOME did not allow it. Keep this window in front and try again.",
            T.warn,
        )),
        Grant::Unasked | Grant::Asking => None,
    };
    let idle = matches!(grant, Grant::Unasked | Grant::Refused);
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child("Screenshots"))
        .child(div().text_size(px(12.)).text_color(T.text3).child(
            "GNOME lets Sukurin take screenshots silently once you allow it, and asks only while a Sukurin window is in front. Until then GNOME's own picker takes captures from the tray.",
        ))
        .child(
            div()
                .id("allow-screenshots")
                .flex_none()
                .w(px(170.))
                .px(px(10.))
                .h(px(26.))
                .rounded(px(6.))
                .bg(T.control)
                .flex()
                .items_center()
                .text_size(px(12.))
                .text_color(if idle { T.text } else { T.text3 })
                .when(idle, |d| d.hover(|d| d.bg(T.hover)).on_click(on_click))
                .child(label),
        )
        .children(status.map(|(text, color)| div().text_size(px(12.)).text_color(color).child(text)))
}
