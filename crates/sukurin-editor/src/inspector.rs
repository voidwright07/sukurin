//! The inspector dock: dimensions, colour under the cursor in several
//! formats, a contrast checker and the average colour of the selection.

use crate::color::{self, Format, Srgb};
use crate::editor::{Editor, ResizeDraft};
use crate::icons::{EIcon, icon};
use crate::keys::{self, Action};
use crate::ui::tooltip_text;
use crate::widgets::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sukurin_ui::Theme;

pub const WIDTH: f32 = 264.;

fn swatch(c: Srgb) -> Rgba {
    rgb(u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b))
}

fn pass_dot(t: &Theme, ok: bool, text: &'static str) -> Div {
    let d = if ok {
        div().size(px(8.)).rounded(px(4.)).bg(t.text).flex_none()
    } else {
        div().size(px(8.)).rounded(px(4.)).border(px(1.5)).border_color(t.text3).flex_none()
    };
    div().flex().items_center().gap(px(6.)).when(!ok, |r| r.text_color(t.text3)).child(d).child(text)
}

fn section(t: &Theme) -> Div {
    div().px(px(14.)).py(px(12.)).flex().flex_col().gap(px(10.)).border_t_1().border_color(t.line)
}

fn row(t: &Theme, k: &'static str, v: String) -> Div {
    div().h(px(24.)).flex().items_center().gap(px(8.)).child(div().flex_1().text_color(t.text2).child(k)).child(mono(12.).child(v))
}

impl Editor {
    /// The contrast pair: the two most recent picks, falling back to the
    /// colour under the cursor and white.
    pub fn contrast_pair(&self) -> (Srgb, Srgb) {
        let live = self.colour_under_cursor();
        match (self.picks.first(), self.picks.get(1)) {
            (Some(a), Some(b)) => (*a, *b),
            (Some(a), None) => (*a, live.unwrap_or(Srgb::new(255, 255, 255))),
            _ => (Srgb::new(255, 255, 255), live.unwrap_or(Srgb::new(0x25, 0x63, 0xEB))),
        }
    }

    fn dims(&self, w: f32, h: f32) -> String {
        if self.dims_px {
            format!("{} × {}", w.round(), h.round())
        } else {
            format!("{} × {}", (w / self.doc.scale).round(), (h / self.doc.scale).round())
        }
    }

    pub fn inspector_panel(&self, t: &Theme, dark: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.doc.scale;
        let full = self.shown_rect();
        let unit = |v: f32| if self.dims_px { v.round() } else { (v / s).round() };
        let dpi = if cfg!(target_os = "macos") { 72. } else { 96. } * s;
        let sel = self.sample_rect();
        let under = self.colour_under_cursor();
        let (fg, bg) = self.contrast_pair();
        let ratio = color::wcag_ratio(fg, bg);
        let pass = color::wcag_pass(ratio);
        let lc = color::apca_lc(fg, bg);

        let header = div()
            .h(px(40.))
            .flex()
            .items_center()
            .justify_between()
            .pl(px(14.))
            .pr(px(8.))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Inspect"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(kbd(t, keys::label_for(Action::Inspector)))
                    .child(icon_button(t, "inspector-close", EIcon::Close, false).on_click(cx.listener(|this, _, w, cx| this.run(Action::Inspector, w, cx)))),
            );

        let units = self.dims_px;
        let open_resize = cx.listener(|this, _, _, cx| {
            let w = this.doc.layers.first().map_or(0., |l| l.bitmap.width() as f32);
            let h = this.doc.layers.first().map_or(0., |l| l.bitmap.height() as f32);
            let k = if this.dims_px { 1. } else { this.doc.scale };
            this.resize = Some(ResizeDraft { w: format!("{}", (w / k).round()), h: format!("{}", (h / k).round()), editing_w: true, nearest: false });
            cx.notify();
        });
        let open_resize_h = cx.listener(|this, _, _, cx| {
            let w = this.doc.layers.first().map_or(0., |l| l.bitmap.width() as f32);
            let h = this.doc.layers.first().map_or(0., |l| l.bitmap.height() as f32);
            let k = if this.dims_px { 1. } else { this.doc.scale };
            this.resize = Some(ResizeDraft { w: format!("{}", (w / k).round()), h: format!("{}", (h / k).round()), editing_w: false, nearest: false });
            cx.notify();
        });
        let dims = section(t)
            .border_t_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(t, "Dimensions"))
                    .child(
                        seg(t)
                            .w(px(128.))
                            .child(seg_item(t, "dim-pt", !units).flex_1().child("pt").on_click(cx.listener(|this, _, _, cx| {
                                this.dims_px = false;
                                cx.notify();
                            })))
                            .child(seg_item(t, "dim-px", units).flex_1().child("px").on_click(cx.listener(|this, _, _, cx| {
                                this.dims_px = true;
                                cx.notify();
                            }))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(field(t, "img-w", "W", format!("{}", unit(full.w))).flex_1().h(px(26.)).tooltip(tooltip_text(*t, "Resize image".into())).on_click(open_resize))
                    .child(icon(EIcon::Link).size(px(14.)).text_color(t.text3))
                    .child(field(t, "img-h", "H", format!("{}", unit(full.h))).flex_1().h(px(26.)).tooltip(tooltip_text(*t, "Resize image".into())).on_click(open_resize_h)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_color(t.text3)
                    .child(mono(11.).child(format!("{} × {} px", full.w.round(), full.h.round())))
                    .child(mono(11.).child(format!("@{}x · {} dpi", trim(s), dpi.round()))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(row(t, "Selection", sel.map_or("–".into(), |r| self.dims(r.w, r.h))))
                    .child(row(t, "Origin", sel.map_or("–".into(), |r| format!("{}, {}", unit(r.x), unit(r.y)))))
                    .child(row(t, "Cursor", self.cursor.map_or("–".into(), |p| format!("{}, {}", unit(p.x.floor()), unit(p.y.floor()))))),
            );

        let shown = under.or(self.picks.first().copied());
        let mut formats = div().flex().flex_col().gap(px(1.));
        for (i, f) in Format::ALL.into_iter().enumerate() {
            let on = f == self.format;
            let value = shown.map_or("–".into(), |c| f.display(c));
            formats = formats.child(
                div()
                    .id(("format", i))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(6.))
                    .mx(px(-6.))
                    .rounded(px(5.))
                    .when(on, |d| d.bg(t.control))
                    .child(div().w(px(44.)).text_size(px(11.)).text_color(t.text3).child(f.label()))
                    .child(mono(12.).flex_1().child(value))
                    .when(on && self.copied == Some(f), |d| {
                        d.child(div().flex().items_center().gap(px(4.)).text_size(px(11.)).child(icon(EIcon::Check).size(px(12.)).text_color(t.text)).child("Copied"))
                    })
                    .when(on && self.copied != Some(f), |d| d.child(kbd(t, "Tab")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.format = f;
                        this.copied = None;
                        cx.notify();
                    })),
            );
        }
        let colour = section(t)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(t, "Colour under cursor"))
                    .child(
                        icon_button(t, "pick", EIcon::Picker, self.picking)
                            .size(px(24.))
                            .tooltip(tooltip_text(*t, "Pick a colour for the contrast check".into()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picking = !this.picking;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().size(px(36.)).rounded(px(6.)).border_1().border_color(rgba(0x7F7F7F40)).when_some(shown, |d, c| d.bg(swatch(c))))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(mono(15.).line_height(px(20.)).font_weight(FontWeight::MEDIUM).child(shown.map_or("–".into(), |c| c.hex())))
                            .child(div().text_size(px(11.)).text_color(t.text3).child(if under.is_some() { "under cursor" } else { "last picked" })),
                    ),
            )
            .child(formats)
            .child(div().text_size(px(11.)).text_color(t.text3).child("Tab copies the highlighted format · ⇧Tab cycles"));

        let chip = |c: Srgb| {
            div()
                .flex_1()
                .h(px(26.))
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(4.))
                .pr(px(8.))
                .rounded(px(6.))
                .bg(t.control)
                .child(div().size(px(18.)).rounded(px(4.)).bg(swatch(c)).border_1().border_color(rgba(0x7F7F7F59)))
                .child(mono(12.).child(c.hex()))
        };
        let nearest = (!pass.aa_text).then(|| color::nearest_passing(fg, bg, 4.5)).flatten();
        let contrast = section(t)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(t, "Contrast"))
                    .child(
                        icon_button(t, "swap", EIcon::Swap, false)
                            .size(px(24.))
                            .tooltip(tooltip_text(*t, format!("Swap  {}", keys::label_for(Action::SwapContrast))))
                            .on_click(cx.listener(|this, _, w, cx| this.run(Action::SwapContrast, w, cx))),
                    ),
            )
            .child(div().flex().gap(px(6.)).child(chip(fg)).child(chip(bg)))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(72.))
                            .h(px(48.))
                            .rounded(px(6.))
                            .bg(swatch(bg))
                            .text_color(swatch(fg))
                            .border_1()
                            .border_color(rgba(0x7F7F7F33))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Aa"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_size(px(11.)).text_color(t.text3).child("WCAG 2.2"))
                                    .child(mono(16.).font_weight(FontWeight::MEDIUM).child(format!("{ratio:.2}:1"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_size(px(11.)).text_color(t.text3).child("APCA"))
                                    .child(mono(16.).font_weight(FontWeight::MEDIUM).child(format!("Lc {:.1}", lc.abs()))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .child(div().flex().gap(px(12.)).child(pass_dot(t, pass.aa_text, "AA text").flex_1()).child(pass_dot(t, pass.aa_large, "AA large").flex_1()))
                    .child(div().flex().gap(px(12.)).child(pass_dot(t, pass.aaa_text, "AAA text").flex_1()).child(pass_dot(t, pass.aaa_large, "AAA large").flex_1()))
                    .child(pass_dot(t, lc.abs() >= 75., "APCA body text (Lc 75+)")),
            )
            .when_some(nearest, |d, n| {
                d.child(
                    div()
                        .id("nearest")
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .p(px(8.))
                        .rounded(px(6.))
                        .bg(t.control)
                        .child(div().size(px(18.)).rounded(px(4.)).bg(swatch(n)).flex_none())
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(t.text2)
                                .child(format!("Nearest passing AA: {} · {:.2}:1", n.hex(), color::wcag_ratio(n, bg))),
                        )
                        .tooltip(tooltip_text(*t, "Click to copy".into()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(this.format.css(n)));
                            this.flash(format!("Copied {}", this.format.css(n)));
                            cx.notify();
                        })),
                )
            });

        let avg = sel.and_then(|r| self.average(r));
        let average = section(t).flex_1().child(label(t, "Average of selection")).child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().size(px(36.)).rounded(px(6.)).border_1().border_color(rgba(0x7F7F7F4D)).when_some(avg, |d, c| d.bg(swatch(c))))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .child(mono(13.).child(avg.map_or("–".into(), |c| c.hex())))
                        .child(mono(11.).text_color(t.text3).child(match sel {
                            Some(r) => format!("{} · {} {}²", self.dims(r.w, r.h), group_thousands((unit(r.w) * unit(r.h)) as u64), if units { "px" } else { "pt" }),
                            None => "Select an annotation or crop".into(),
                        })),
                )
                .when_some(avg, |d, c| {
                    d.child(
                        icon_button(t, "copy-avg", EIcon::Copy, false).tooltip(tooltip_text(*t, "Copy average colour".into())).on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(this.format.css(c)));
                            this.flash(format!("Copied {}", this.format.css(c)));
                            cx.notify();
                        })),
                    )
                }),
        );

        div()
            .flex()
            .gap(px(8.))
            .items_start()
            .when(self.resize.is_some(), |d| d.child(self.resize_popover(t, dark, cx)))
            .child(
                panel(t, dark)
                    .id("inspector")
                    .w(px(WIDTH))
                    .max_h(self.area.size.height - px(8.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                        .child(header)
                    .child(dims)
                    .child(colour)
                    .child(contrast)
                    .child(average),
            )
    }

    fn resize_popover(&self, t: &Theme, dark: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let r = self.resize.as_ref().expect("resize is open");
        let s = self.doc.scale;
        let k = if self.dims_px { 1. } else { s };
        let base = self.doc.layers.first().map_or((1., 1.), |l| (l.bitmap.width() as f32, l.bitmap.height() as f32));
        let target_px = r.w.parse::<f32>().unwrap_or(0.) * k;
        let fld = |id: &'static str, key: &'static str, v: &str, focused: bool| {
            let mut d = div()
                .id(id)
                .flex_1()
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(6.))
                .bg(t.control)
                .border_1()
                .border_color(transparent_black())
                .child(div().text_size(px(11.)).text_color(t.text3).child(key))
                .child(mono(12.).child(v.to_string()));
            if focused {
                d = d.border_color(t.accent).child(div().w(px(1.)).h(px(14.)).bg(t.text));
            }
            d
        };
        let pct = (target_px / base.0 * 100.).round();
        panel(t, dark)
            .id("resize")
            .w(px(300.))
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Resize image"))
                    .child(mono(11.).text_color(t.text3).child(format!("from {} × {}", (base.0 / k).round(), (base.1 / k).round()))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(fld("rs-w", "W", &r.w, r.editing_w).on_click(cx.listener(|this, _, _, cx| {
                        if let Some(r) = &mut this.resize {
                            r.editing_w = true;
                        }
                        cx.notify();
                    })))
                    .child(icon(EIcon::Link).size(px(14.)).text_color(t.text2))
                    .child(fld("rs-h", "H", &r.h, !r.editing_w).on_click(cx.listener(|this, _, _, cx| {
                        if let Some(r) = &mut this.resize {
                            r.editing_w = false;
                        }
                        cx.notify();
                    }))),
            )
            .child(
                seg(t)
                    .child(seg_item(t, "rs-pt", !self.dims_px).flex_1().child("pt").on_click(cx.listener(|this, _, _, cx| {
                        this.set_resize_units(false);
                        cx.notify();
                    })))
                    .child(seg_item(t, "rs-px", self.dims_px).flex_1().child("px").on_click(cx.listener(|this, _, _, cx| {
                        this.set_resize_units(true);
                        cx.notify();
                    }))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_color(t.text2).child("Resample"))
                    .child(
                        seg(t)
                            .w(px(170.))
                            .child(seg_item(t, "rs-smooth", !r.nearest).flex_1().child("Smooth").on_click(cx.listener(|this, _, _, cx| {
                                if let Some(r) = &mut this.resize {
                                    r.nearest = false;
                                }
                                cx.notify();
                            })))
                            .child(seg_item(t, "rs-nearest", r.nearest).flex_1().child("Nearest").on_click(cx.listener(|this, _, _, cx| {
                                if let Some(r) = &mut this.resize {
                                    r.nearest = true;
                                }
                                cx.notify();
                            }))),
                    ),
            )
            .child(mono(11.).text_color(t.text3).child(format!(
                "Output {} × {} px @{}x · {}%",
                target_px.round(),
                (target_px * base.1 / base.0).round(),
                trim(s),
                pct
            )))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("rs-cancel")
                            .h(px(28.))
                            .px(px(12.))
                            .rounded(px(6.))
                            .bg(t.control)
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Cancel")
                            .child(kbd(t, "esc"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.resize = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("rs-apply")
                            .h(px(28.))
                            .px(px(12.))
                            .rounded(px(6.))
                            .bg(t.text)
                            .text_color(t.chrome)
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Resize")
                            .child(kbd(t, "↵").bg(rgba(0x0000001F)).text_color(t.chrome))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_resize();
                                cx.notify();
                            })),
                    ),
            )
    }

    fn set_resize_units(&mut self, px: bool) {
        if px == self.dims_px {
            return;
        }
        let s = self.doc.scale;
        let k = if px { s } else { 1. / s };
        if let Some(r) = &mut self.resize {
            let conv = |v: &str| v.parse::<f32>().map(|v| format!("{}", (v * k).round())).unwrap_or_default();
            r.w = conv(&r.w);
            r.h = conv(&r.h);
        }
        self.dims_px = px;
    }

    fn apply_resize(&mut self) {
        let Some(r) = self.resize.take() else { return };
        let k = if self.dims_px { 1. } else { self.doc.scale };
        if let Ok(w) = r.w.parse::<f32>() {
            self.resize_to((w * k).round().max(1.) as u32, r.nearest);
        }
    }

    pub fn resize_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let base = self.doc.layers.first().map_or((1., 1.), |l| (l.bitmap.width() as f32, l.bitmap.height() as f32));
        let Some(r) = self.resize.as_mut() else { return };
        match k.key.as_str() {
            "escape" => self.resize = None,
            "enter" => self.apply_resize(),
            "tab" => r.editing_w = !r.editing_w,
            "backspace" => {
                if r.editing_w { r.w.pop() } else { r.h.pop() };
            }
            key if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() && (if r.editing_w { &r.w } else { &r.h }).len() < 5 => {
                if r.editing_w { r.w.push_str(key) } else { r.h.push_str(key) }
            }
            _ => {}
        }
        if let Some(r) = self.resize.as_mut() {
            let aspect = base.1 / base.0;
            if r.editing_w {
                r.h = r.w.parse::<f32>().map(|w| format!("{}", (w * aspect).round())).unwrap_or_default();
            } else {
                r.w = r.h.parse::<f32>().map(|h| format!("{}", (h / aspect).round())).unwrap_or_default();
            }
        }
        cx.notify();
    }
}

fn trim(s: f32) -> String {
    let t = format!("{s:.2}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{group_thousands, trim};

    #[test]
    fn numbers_format_like_the_design() {
        assert_eq!(group_thousands(28308), "28,308");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(trim(1.5), "1.5");
        assert_eq!(trim(2.0), "2");
    }
}
