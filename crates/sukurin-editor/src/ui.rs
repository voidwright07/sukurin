//! The editor window's chrome and canvas: title bar, properties bar, tool
//! strip, canvas area and status bar.

use crate::editor::{self, Editor, Gesture, Grab, STRIP_W, theme};
use crate::geom::{self, Handle};
use crate::icons::{EIcon, icon};
use crate::keys::{self, Action};
use crate::paint;
use crate::viewport::Viewport;
use crate::tools::{Props, TOOLS, Tool};
use crate::widgets::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use shattr_annotate::{ArrowKind, Backdrop, Dash, Document, Fill as ShapeFill, Pt, RectF, RedactMode, Shape, TextBackground};
use shattr_core::Rgba as CoreRgba;
use shattr_ui::theme::ANNOTATION_INK;
use shattr_ui::{MONO_FONT, Theme, UI_FONT};
use std::time::Duration;

pub struct BackdropPreset {
    pub name: &'static str,
    pub from: u32,
    pub to: u32,
}

pub const BACKDROPS: [BackdropPreset; 6] = [
    BackdropPreset { name: "None", from: 0, to: 0 },
    BackdropPreset { name: "Dusk", from: 0x2B2F6B, to: 0xC25A6A },
    BackdropPreset { name: "Sea", from: 0x0F4C5C, to: 0x5FB49C },
    BackdropPreset { name: "Paper", from: 0xECE8E1, to: 0xECE8E1 },
    BackdropPreset { name: "Graphite", from: 0x2A2C31, to: 0x2A2C31 },
    BackdropPreset { name: "Sample from image", from: 0, to: 0 },
];

fn core(hex: u32) -> CoreRgba {
    CoreRgba::opaque((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn gpui(c: CoreRgba) -> Rgba {
    Rgba { r: c.r as f32 / 255., g: c.g as f32 / 255., b: c.b as f32 / 255., a: c.a as f32 / 255. }
}

impl BackdropPreset {
    pub fn backdrop(&self, doc: &Document) -> Backdrop {
        let s = doc.scale;
        let (from, to) = if self.name == "Sample from image" {
            let c = doc
                .layers
                .first()
                .and_then(|l| l.bitmap.average(shattr_core::RectPx::new(0, 0, l.bitmap.width(), 2)))
                .unwrap_or(core(0xF3F4F6));
            (c, c)
        } else {
            (core(self.from), core(self.to))
        };
        let keep = doc.backdrop;
        Backdrop {
            from,
            to,
            padding: keep.map_or(44. * s, |b| b.padding),
            shadow: keep.map_or(24. * s, |b| b.shadow),
            radius: keep.map_or(10. * s, |b| b.radius),
            aspect: keep.and_then(|b| b.aspect),
        }
    }

    fn fill(&self) -> Background {
        if self.from == self.to {
            gpui(core(self.from)).into()
        } else {
            linear_gradient(135., linear_color_stop(gpui(core(self.from)), 0.), linear_color_stop(gpui(core(self.to)), 1.))
        }
    }
}

/// The spotlight's dim at full opacity, fixed by the annotation pipeline.
const SPOT_DIM: f32 = 0.6;
const INK_NAMES: [&str; 8] = ["Red", "Orange", "Yellow", "Green", "Blue", "Magenta", "White", "Black"];
const MEASURE_INK: [u32; 3] = [0xE23D9B, 0x2F7BF5, 0xFF8A1F];
const ASPECTS: [(&str, Option<(u32, u32)>); 4] = [("Free", None), ("16:9", Some((16, 9))), ("4:3", Some((4, 3))), ("1:1", Some((1, 1)))];

fn scale_label(s: f32) -> String {
    if (s - s.round()).abs() < 0.01 { format!("@{}x", s.round() as i32) } else { format!("@{s:.2}x").replace("0x", "x") }
}

impl Editor {
    fn pt(&self, v: f32) -> String {
        format!("{}", (v / self.doc.scale).round() as i32)
    }

    fn title_bar(&self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (w, h) = (self.shot.bitmap.width() as f32 / self.shot.scale, self.shot.bitmap.height() as f32 / self.shot.scale);
        let is_mac = cfg!(target_os = "macos");
        let is_win = cfg!(windows);
        let drag = |d: Div| -> Div {
            let d = d.window_control_area(WindowControlArea::Drag);
            if is_win {
                d
            } else {
                d.on_mouse_down(MouseButton::Left, |ev, window, _| {
                    if ev.click_count >= 2 {
                        if cfg!(target_os = "macos") {
                            window.titlebar_double_click();
                        } else {
                            window.zoom_window();
                        }
                    } else {
                        window.start_window_move();
                    }
                })
            }
        };
        let action_button = |id: &'static str, i: EIcon, name: &'static str, action: Action, t: &Theme, cx: &mut Context<Self>| {
            let tip = format!("{name}  {}", keys::label_for(action));
            icon_button(t, id, i, false)
                .tooltip(tooltip_text(*t, tip))
                .on_click(cx.listener(move |this, _, window, cx| this.run(action, window, cx)))
        };
        let mut bar = div()
            .h(px(editor::TITLE_H))
            .flex_none()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(t.line)
            .bg(t.chrome);
        if is_mac {
            bar = bar.child(drag(div().w(px(78.)).h_full().flex_none()));
        } else if is_win {
            bar = bar.child(drag(div().h_full().flex().items_center().pl(px(12.)).pr(px(10.)).flex_none().child(icon(EIcon::Logo).text_color(t.text))));
        } else {
            bar = bar.child(drag(div().w(px(14.)).h_full().flex_none()));
        }
        bar = bar
            .child(drag(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .min_w_0()
                    .child(div().font_weight(FontWeight::MEDIUM).whitespace_nowrap().child(self.title.clone()))
                    .child(mono(12.).text_color(t.text3).child(format!("{} × {} {}", w.round(), h.round(), scale_label(self.shot.scale))))
                    .when(self.edited, |d| d.child(div().text_size(px(12.)).text_color(t.text3).whitespace_nowrap().child("· edited"))),
            ))
            .child(drag(div().flex_1().h_full()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .pr(px(8.))
                    .child(action_button("undo", EIcon::Undo, "Undo", Action::Undo, t, cx))
                    .child(action_button("redo", EIcon::Redo, "Redo", Action::Redo, t, cx))
                    .child(sep(t).mx(px(6.)))
                    .child(action_button("copy", EIcon::Copy, "Copy", Action::Copy, t, cx))
                    .child(action_button("save", EIcon::Save, "Save", Action::Save, t, cx))
                    .child(action_button("pin", EIcon::Pin, "Pin", Action::Pin, t, cx))
                    .child(disabled_icon(t, "upload", EIcon::Upload).tooltip(tooltip_text(*t, format!("Upload  {} · not built yet", keys::label_for(Action::Upload)))))
                    .child(action_button("palette", EIcon::Search, "Commands", Action::Palette, t, cx))
                    .child(
                        div()
                            .id("ai")
                            .ml(px(6.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .pl(px(10.))
                            .pr(px(6.))
                            .rounded(px(6.))
                            .bg(t.control)
                            .text_color(t.text3)
                            .font_weight(FontWeight::MEDIUM)
                            .whitespace_nowrap()
                            .tooltip(tooltip_text(*t, "Send to AI · not built yet".to_string()))
                            .child(icon(EIcon::Ai).text_color(t.text3))
                            .child("Send to AI")
                            .child(kbd(t, keys::label_for(Action::SendToAi))),
                    ),
            );
        if is_win {
            bar = bar.child(window_controls(t, window));
        } else if !is_mac {
            bar = bar.child(
                div().pr(px(8.)).child(
                    div()
                        .id("close")
                        .size(px(24.))
                        .rounded_full()
                        .bg(t.hover)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(EIcon::Close).size(px(14.)).text_color(t.text))
                        .on_click(|_, window, _| window.remove_window()),
                ),
            );
        }
        bar
    }

    fn props_bar(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let tool = self.props_tool();
        let spec = tool.spec();
        let pct = self.vp.percent(self.scale_factor).round();
        div()
            .h(px(editor::PROPS_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(12.))
            .pr(px(8.))
            .border_b_1()
            .border_color(t.line)
            .bg(t.chrome)
            .child(
                div()
                    .w(px(148.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().font_weight(FontWeight::MEDIUM).whitespace_nowrap().child(spec.name))
                    .child(kbd(t, spec.key.label())),
            )
            .child(sep(t))
            .child(self.props_branch(t, spec.props, cx))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        icon_button(t, "zoom-out", EIcon::Minus, false)
                            .tooltip(tooltip_text(*t, format!("Zoom out  {}", keys::label_for(Action::ZoomOut))))
                            .on_click(cx.listener(|this, _, w, cx| this.run(Action::ZoomOut, w, cx))),
                    )
                    .child(
                        field(t, "zoom", "", format!("{pct}%"))
                            .w(px(58.))
                            .justify_center()
                            .tooltip(tooltip_text(*t, format!("Actual size  {}", keys::label_for(Action::Zoom100))))
                            .on_click(cx.listener(|this, _, w, cx| this.run(Action::Zoom100, w, cx))),
                    )
                    .child(
                        icon_button(t, "zoom-in", EIcon::Plus, false)
                            .tooltip(tooltip_text(*t, format!("Zoom in  {}", keys::label_for(Action::ZoomIn))))
                            .on_click(cx.listener(|this, _, w, cx| this.run(Action::ZoomIn, w, cx))),
                    )
                    .child(
                        icon_button(t, "fit", EIcon::Fit, false)
                            .tooltip(tooltip_text(*t, format!("Fit  {}", keys::label_for(Action::ZoomFit))))
                            .on_click(cx.listener(|this, _, w, cx| this.run(Action::ZoomFit, w, cx))),
                    ),
            )
    }

    fn ink_dots(&self, t: &Theme, inks: &[u32], cx: &mut Context<Self>) -> Div {
        let current = self.current_style().color;
        let mut row = div().flex().flex_none().items_center().gap(px(6.));
        for (i, hex) in inks.iter().copied().enumerate() {
            let c = core(hex);
            let name = ANNOTATION_INK.iter().position(|h| *h == hex).map_or("Colour", |i| INK_NAMES[i]);
            row = row.child(
                dot(t, ("ink", i), gpui(c), current.r == c.r && current.g == c.g && current.b == c.b)
                    .tooltip(tooltip_text(*t, name.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_style(|s| s.color = c);
                        cx.notify();
                    })),
            );
        }
        row
    }

    fn width_field(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let s = self.doc.scale;
        let w = self.current_style().width;
        stepper(t, field(t, "width", "W", self.pt(w)), "Thickness", cx, move |this, d| {
            let next = ((this.current_style().width / s).round() + d as f32).clamp(1., 40.) * s;
            this.set_style(|st| st.width = next);
        })
    }

    fn opacity_field(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let o = self.current_style().opacity;
        stepper(t, field(t, "opacity", "α", format!("{}%", (o * 100.).round())), "Opacity", cx, move |this, d| {
            let next = ((this.current_style().opacity * 10.).round() + d as f32).clamp(1., 10.) / 10.;
            this.set_style(|st| st.opacity = next);
        })
    }

    fn dash_seg(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let cur = self.current_style().dash;
        let mut s = seg(t);
        for (i, (dash, ic, name)) in [(Dash::Solid, EIcon::Solid, "Solid"), (Dash::Dashed, EIcon::Dashed, "Dashed"), (Dash::Dotted, EIcon::Dotted, "Dotted")]
            .into_iter()
            .enumerate()
        {
            s = s.child(
                seg_item(t, ("dash", i), cur == dash)
                    .child(seg_icon(t, ic, cur == dash))
                    .tooltip(tooltip_text(*t, name.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_style(|st| st.dash = dash);
                        cx.notify();
                    })),
            );
        }
        s
    }

    fn hand_drawn(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        toggle(t, "hand", self.current_style().hand_drawn, "Hand-drawn", Some(keys::label_for(Action::HandDrawn)))
            .on_click(cx.listener(|this, _, w, cx| this.run(Action::HandDrawn, w, cx)))
    }

    fn text_seg<T: Copy + PartialEq + 'static>(
        &self,
        t: &Theme,
        id: &'static str,
        items: &[(&'static str, T)],
        current: T,
        cx: &mut Context<Self>,
        set: impl Fn(&mut Editor, T) + Copy + 'static,
    ) -> Div {
        let mut s = seg(t);
        for (i, (name, v)) in items.iter().copied().enumerate() {
            s = s.child(
                seg_item(t, (id, i), current == v).child(name).on_click(cx.listener(move |this, _, _, cx| {
                    set(this, v);
                    cx.notify();
                })),
            );
        }
        s
    }

    fn props_branch(&self, t: &Theme, props: Props, cx: &mut Context<Self>) -> Div {
        let row = div().flex().items_center().gap(px(10.)).min_w_0().overflow_hidden();
        let st = self.current_style();
        let s = self.doc.scale;
        match props {
            Props::Arrow => {
                let kind = match self.selected_shape() {
                    Some(Shape::Arrow { kind, .. }) => *kind,
                    _ => self.defaults.arrow,
                };
                let mut kinds = seg(t);
                for (i, (k, ic, name)) in [
                    (ArrowKind::Straight, EIcon::ArrowStraight, "Straight  1"),
                    (ArrowKind::Curved, EIcon::ArrowCurved, "Curved  2"),
                    (ArrowKind::Bendable, EIcon::ArrowBendable, "Bendable  3"),
                ]
                .into_iter()
                .enumerate()
                {
                    kinds = kinds.child(
                        seg_item(t, ("kind", i), kind == k)
                            .child(seg_icon(t, ic, kind == k))
                            .tooltip(tooltip_text(*t, name.to_string()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_arrow_kind(k);
                                cx.notify();
                            })),
                    );
                }
                row.child(kinds)
                    .child(self.ink_dots(t, &ANNOTATION_INK, cx))
                    .child(sep(t))
                    .child(self.width_field(t, cx))
                    .child(self.dash_seg(t, cx))
                    .child(self.opacity_field(t, cx))
                    .child(self.hand_drawn(t, cx))
            }
            Props::Stroke => {
                let r = st.corner_radius;
                row.child(self.ink_dots(t, &ANNOTATION_INK, cx))
                    .child(sep(t))
                    .child(self.width_field(t, cx))
                    .child(self.dash_seg(t, cx))
                    .child(self.text_seg(
                        t,
                        "fill",
                        &[("No fill", ShapeFill::None), ("Tint", ShapeFill::Tint), ("Solid", ShapeFill::Solid)],
                        st.fill,
                        cx,
                        |this, f| this.set_style(|s| s.fill = f),
                    ))
                    .child(stepper(t, field(t, "radius", "R", self.pt(r)), "Corner radius", cx, move |this, d| {
                        let next = ((this.current_style().corner_radius / s).round() + d as f32).clamp(0., 64.) * s;
                        this.set_style(|st| st.corner_radius = next);
                    }))
                    .child(self.opacity_field(t, cx))
                    .child(self.hand_drawn(t, cx))
            }
            Props::Text => {
                let next_step = self.doc.next_step();
                row.child(stepper(t, field(t, "size", "Size", self.pt(st.font_size)), "Font size", cx, move |this, d| {
                    let next = ((this.current_style().font_size / s).round() + d as f32 * 2.).clamp(8., 128.) * s;
                    this.set_style(|st| st.font_size = next);
                }))
                .child(self.text_seg(t, "weight", &[("Regular", false), ("Bold", true)], st.bold, cx, |this, b| {
                    this.set_style(|s| s.bold = b)
                }))
                .child(self.ink_dots(t, &ANNOTATION_INK, cx))
                .child(sep(t))
                .child(self.text_seg(
                    t,
                    "bg",
                    &[("None", TextBackground::None), ("Pill", TextBackground::Pill), ("Outline", TextBackground::Outline)],
                    st.text_background,
                    cx,
                    |this, b| this.set_style(|s| s.text_background = b),
                ))
                .child(field(t, "next", "Next", next_step.to_string()).tooltip(tooltip_text(*t, "Next step number".to_string())))
                .child(self.opacity_field(t, cx))
            }
            Props::Redact => {
                let mode = self.redact_mode();
                let block = match self.selected_shape() {
                    Some(Shape::Redact { block, .. }) => *block,
                    _ => self.defaults.block,
                };
                let text_only = mode == RedactMode::EraseText;
                let shown = match mode {
                    RedactMode::EraseText => RedactMode::Erase,
                    m => m,
                };
                row.child(self.text_seg(
                    t,
                    "mode",
                    &[("Pixelate", RedactMode::Pixelate), ("Blur", RedactMode::Blur), ("Erase", RedactMode::Erase)],
                    shown,
                    cx,
                    |this, m| this.set_redact_mode(m),
                ))
                .child(stepper(
                    t,
                    field(t, "block", if mode == RedactMode::Blur { "Radius" } else { "Block" }, format!("{} px", block.round())),
                    "Strength",
                    cx,
                    move |this, d| {
                        let b = match this.selected_shape() {
                            Some(Shape::Redact { block, .. }) => *block,
                            _ => this.defaults.block,
                        };
                        this.set_block((b + d as f32 * 2.).clamp(2., 64.));
                    },
                ))
                .child(sep(t))
                .child(
                    toggle(t, "text-only", text_only, "Text only", Some(keys::label_for(Action::EraseTextOnly)))
                        .on_click(cx.listener(|this, _, w, cx| this.run(Action::EraseTextOnly, w, cx))),
                )
                .child(div().text_size(px(12.)).text_color(t.text3).whitespace_nowrap().child("Keeps backgrounds and borders, removes glyphs"))
            }
            Props::Spot => {
                let oval = match self.selected_shape() {
                    Some(Shape::Spotlight { oval, .. }) => *oval,
                    _ => self.defaults.spot_oval,
                };
                let mut shape = seg(t);
                for (i, (o, ic, name)) in [(false, EIcon::Rect, "Rectangle"), (true, EIcon::Oval, "Oval")].into_iter().enumerate() {
                    shape = shape.child(
                        seg_item(t, ("spot", i), oval == o)
                            .child(seg_icon(t, ic, oval == o))
                            .tooltip(tooltip_text(*t, name.to_string()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_spot_oval(o);
                                cx.notify();
                            })),
                    );
                }
                row.child(shape)
                    .child(stepper(t, field(t, "dim", "Dim", format!("{}%", (st.opacity * SPOT_DIM * 100.).round())), "Dim amount", cx, |this, d| {
                        let next = ((this.current_style().opacity * SPOT_DIM * 10.).round() + d as f32).clamp(1., SPOT_DIM * 10.) / 10. / SPOT_DIM;
                        this.set_style(|st| st.opacity = next);
                    }))
                    .child(field(t, "feather", "Feather", "0".into()).opacity(0.5).tooltip(tooltip_text(*t, "Feather · not built yet".into())))
                    .child(stepper(t, field(t, "spot-r", "R", self.pt(st.corner_radius)), "Corner radius", cx, move |this, d| {
                        let next = ((this.current_style().corner_radius / s).round() + d as f32).clamp(0., 64.) * s;
                        this.set_style(|st| st.corner_radius = next);
                    }))
            }
            Props::Measure => row
                .child(self.text_seg(t, "units", &[("pt", false), ("px", true)], self.defaults.measure_px, cx, |this, px| {
                    this.defaults.measure_px = px;
                    this.dims_px = px;
                }))
                .child(toggle(t, "gaps", false, "Gaps on hover", None).opacity(0.5).tooltip(tooltip_text(*t, "Not built yet".into())))
                .child(toggle(t, "snap", false, "Snap to edges", None).opacity(0.5).tooltip(tooltip_text(*t, "Not built yet".into())))
                .child(sep(t))
                .child(self.ink_dots(t, &MEASURE_INK, cx))
                .child(field_button(t, "bake", "Bake into image", None).opacity(0.5).tooltip(tooltip_text(*t, "Measurements already export with the image".into()))),
            Props::Overlay => {
                let opacity = match self.selected_shape() {
                    Some(Shape::Overlay { opacity, .. }) => *opacity,
                    _ => self.defaults.overlay_opacity,
                };
                row.child(field_button(t, "choose", "Choose image…", None).on_click(cx.listener(|_, _, _, cx| {
                    let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
                    cx.spawn(async move |this, cx| {
                        if let Ok(Ok(Some(paths))) = paths.await {
                            let _ = this.update(cx, |this, cx| this.drop_paths(&ExternalPaths(paths.into()), cx));
                        }
                    })
                    .detach();
                })))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().text_size(px(11.)).text_color(t.text3).child("Opacity"))
                        .child(slider(t, opacity, cx))
                        .child(mono(12.).w(px(34.)).child(format!("{}%", (opacity * 100.).round()))),
                )
                .child(
                    seg(t)
                        .child(seg_item(t, ("blend", 0usize), true).child("Normal"))
                        .child(seg_item(t, ("blend", 1usize), false).child("Difference").opacity(0.5))
                        .child(seg_item(t, ("blend", 2usize), false).child("Onion").opacity(0.5)),
                )
                .child(div().text_size(px(12.)).text_color(t.text3).whitespace_nowrap().child("[ ] opacity · ⇧ arrows nudge 10"))
            }
            Props::Backdrop => {
                let current = self.doc.backdrop;
                let mut dots = div().flex().flex_none().items_center().gap(px(6.));
                for (i, p) in BACKDROPS.iter().enumerate() {
                    let selected = match current {
                        None => i == 0,
                        Some(b) => i != 0 && i != 5 && b.from == core(p.from) && b.to == core(p.to),
                    };
                    let fill: Background = match i {
                        0 => transparent_black().into(),
                        5 => self.doc.layers.first().and_then(|l| l.bitmap.average(shattr_core::RectPx::new(0, 0, l.bitmap.width(), 2))).map_or(gpui(core(0xF3F4F6)).into(), |c| gpui(c).into()),
                        _ => p.fill(),
                    };
                    dots = dots.child(
                        dot(t, ("backdrop", i), fill, selected).tooltip(tooltip_text(*t, p.name.to_string())).on_click(cx.listener(move |this, _, _, cx| {
                            let b = (i != 0).then(|| BACKDROPS[i].backdrop(&this.doc));
                            this.set_backdrop(b);
                            cx.notify();
                        })),
                    );
                }
                let b = current.unwrap_or_else(|| BACKDROPS[1].backdrop(&self.doc));
                let set = move |f: fn(&mut Backdrop, f32), d: i32| {
                    move |this: &mut Editor, step: i32| {
                        let mut b = this.doc.backdrop.unwrap_or_else(|| BACKDROPS[1].backdrop(&this.doc));
                        f(&mut b, (step * d) as f32 * this.doc.scale);
                        this.set_backdrop(Some(b));
                    }
                };
                row.child(dots)
                    .child(sep(t))
                    .child(stepper(t, field(t, "pad", "Pad", self.pt(b.padding)), "Padding", cx, set(|b, v| b.padding = (b.padding + v).max(0.), 4)))
                    .child(stepper(t, field(t, "shadow", "Shadow", self.pt(b.shadow)), "Shadow", cx, set(|b, v| b.shadow = (b.shadow + v).max(0.), 4)))
                    .child(stepper(t, field(t, "bd-r", "R", self.pt(b.radius)), "Corner radius", cx, set(|b, v| b.radius = (b.radius + v).max(0.), 2)))
                    .child(self.text_seg(t, "bd-aspect", &[("Auto", None), ("16:9", Some((16, 9))), ("4:3", Some((4, 3))), ("1:1", Some((1, 1)))], b.aspect, cx, |this, a| {
                        let mut b = this.doc.backdrop.unwrap_or_else(|| BACKDROPS[1].backdrop(&this.doc));
                        b.aspect = a;
                        this.set_backdrop(Some(b));
                    }))
            }
            Props::Select => {
                let r = if self.tool == Tool::Crop { self.crop_draft } else { None }.or(self.sample_rect()).unwrap_or(self.shown_rect());
                row.child(self.text_seg(t, "aspect", &ASPECTS, self.defaults.aspect, cx, |this, a| {
                    this.defaults.aspect = a;
                    if let Some(r) = this.crop_draft {
                        this.crop_draft = Some(geom::with_aspect(r, a));
                    }
                }))
                .child(field(t, "sel-w", "W", self.pt(r.w)))
                .child(field(t, "sel-h", "H", self.pt(r.h)))
                .child(sep(t))
                .child(field_button(t, "trim", "Trim to content", Some(keys::label_for(Action::TrimToContent))).on_click(cx.listener(|this, _, w, cx| this.run(Action::TrimToContent, w, cx))))
                .child(
                    field_button(t, "apply-crop", "Crop to selection", Some(keys::label_for(Action::ApplyCrop)))
                        .when(self.crop_draft.is_none(), |d| d.opacity(0.5))
                        .on_click(cx.listener(|this, _, w, cx| this.run(Action::ApplyCrop, w, cx))),
                )
            }
            Props::Combine => row
                .child(field_button(t, "add-capture", "Add capture", Some(keys::label_for(Action::Tool(Tool::Combine)))).on_click(cx.listener(|this, _, _, cx| {
                    this.paste_layer(cx);
                    cx.notify();
                })))
                .child(field_button(t, "history", "From history…", None).opacity(0.5).tooltip(tooltip_text(*t, "Not built yet".into())))
                .child(
                    seg(t)
                        .child(seg_item(t, ("arr", 0usize), false).child("Free"))
                        .child(seg_item(t, ("arr", 1usize), false).child("Row").on_click(cx.listener(|this, _, w, cx| this.run(Action::ArrangeRow, w, cx))))
                        .child(seg_item(t, ("arr", 2usize), false).child("Column").on_click(cx.listener(|this, _, w, cx| this.run(Action::ArrangeColumn, w, cx))))
                        .child(seg_item(t, ("arr", 3usize), false).child("Grid").on_click(cx.listener(|this, _, w, cx| this.run(Action::ArrangeGrid, w, cx)))),
                )
                .child(stepper(t, field(t, "gap", "Gap", format!("{}", self.defaults.gap.round())), "Gap", cx, |this, d| {
                    this.defaults.gap = (this.defaults.gap + d as f32 * 4.).clamp(0., 200.);
                })),
        }
    }

    fn strip(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mut strip = div()
            .w(px(STRIP_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .py(px(8.))
            .border_r_1()
            .border_color(t.line)
            .bg(t.chrome);
        let mut group = 0;
        for spec in TOOLS.iter() {
            if spec.group != group {
                group = spec.group;
                strip = strip.child(div().w(px(20.)).h(px(1.)).my(px(2.)).bg(t.line2).flex_none());
            }
            let tool = spec.tool;
            strip = strip.child(
                icon_button(t, spec.name, spec.icon, self.tool == tool)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hovered_tool = Some(tool);
                        } else if this.hovered_tool == Some(tool) {
                            this.hovered_tool = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, w, cx| this.run(Action::Tool(tool), w, cx))),
            );
        }
        strip
            .child(div().flex_1())
            .child(
                icon_button(t, "inspector", EIcon::Inspector, false)
                    .when(self.inspector, |d| d.bg(t.hover))
                    .tooltip(tooltip_text(*t, format!("Inspector  {}", keys::label_for(Action::Inspector))))
                    .on_click(cx.listener(|this, _, w, cx| this.run(Action::Inspector, w, cx))),
            )
            .child(
                icon_button(t, "keys", EIcon::Keyboard, false)
                    .tooltip(tooltip_text(*t, "Shortcuts  ?".to_string()))
                    .on_click(cx.listener(|this, _, w, cx| this.run(Action::Shortcuts, w, cx))),
            )
    }

    /// The design's tool tooltip: name, key and hint, beside the strip.
    fn tool_tip(&self, t: &Theme) -> Option<impl IntoElement> {
        let tool = self.hovered_tool?;
        let i = TOOLS.iter().position(|s| s.tool == tool)?;
        let spec = &TOOLS[i];
        let y = 8. + i as f32 * 32. + spec.group as f32 * 9. + 14. - 25.;
        Some(
            div()
                .absolute()
                .left(px(50.))
                .top(px(y))
                .flex()
                .items_center()
                .child(div().w_0().h_0().border_t(px(5.)).border_b(px(5.)).border_r(px(5.)).border_color(transparent_black()).border_r(px(5.)))
                .child(
                    div()
                        .bg(t.hover)
                        .rounded(px(7.))
                        .px(px(10.))
                        .py(px(7.))
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .whitespace_nowrap()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(div().font_weight(FontWeight::MEDIUM).text_color(t.text).child(spec.name))
                                .child(kbd(t, spec.key.label()).bg(t.chrome)),
                        )
                        .child(div().text_size(px(12.)).text_color(t.text2).child(spec.hint)),
                ),
        )
    }

    fn status_bar(&self, t: &Theme) -> impl IntoElement {
        let under = self.colour_under_cursor();
        let sel = self.sample_rect();
        let tool = self.props_tool();
        let st = self.current_style();
        let mut summary = vec![tool.spec().name.to_string()];
        match tool.spec().props {
            Props::Arrow => {
                let kind = match self.selected_shape() {
                    Some(Shape::Arrow { kind, .. }) => *kind,
                    _ => self.defaults.arrow,
                };
                summary.push(format!("{kind:?}").to_lowercase());
                summary.push(format!("{} pt", self.pt(st.width)));
            }
            Props::Stroke => summary.push(format!("{} pt", self.pt(st.width))),
            Props::Redact => {
                summary = vec![
                    match self.redact_mode() {
                        RedactMode::Pixelate => "Pixelate",
                        RedactMode::Blur => "Blur",
                        RedactMode::Erase => "Erase",
                        RedactMode::EraseText => "Erase text",
                    }
                    .to_string(),
                ];
            }
            _ => {}
        }
        let full = self.canvas_bounds();
        div()
            .h(px(editor::STATUS_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(12.))
            .border_t_1()
            .border_color(t.line)
            .bg(t.chrome)
            .text_size(px(12.))
            .text_color(t.text2)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(mono(12.).min_w(px(108.)).child(match self.cursor {
                Some(p) => format!("x {:>4}  y {:>4}", p.x.floor() as i32, p.y.floor() as i32),
                None => "x –  y –".to_string(),
            }))
            .when_some(under, |d, c| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().size(px(10.)).rounded(px(2.)).bg(rgb(u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b))))
                        .child(mono(12.).child(c.hex())),
                )
            })
            .when_some(sel, |d, r| d.child(mono(12.).child(format!("sel {} × {}", r.w.round(), r.h.round()))))
            .child(div().text_color(t.text3).child(summary.join(" · ")))
            .child(div().flex_1())
            .when_some(self.flash.as_ref(), |d, (text, _)| {
                d.child(div().flex().items_center().gap(px(6.)).text_color(t.text3).child(icon(EIcon::Check).size(px(14.)).text_color(t.text3)).child(text.clone()))
            })
            .child(mono(12.).text_color(t.text3).child(format!(
                "{} × {} px · {}%",
                full.w.round(),
                full.h.round(),
                self.vp.percent(self.scale_factor).round()
            )))
            .child(div().flex().items_center().gap(px(6.)).child(kbd(t, keys::label_for(Action::Palette))).child(div().text_color(t.text3).child("Commands")))
            .child(div().flex().items_center().gap(px(6.)).child(kbd(t, "?")).child(div().text_color(t.text3).child("Keys")))
    }

    fn canvas_area(&mut self, t: &Theme, dark: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = self.vp;
        let scene = self.scene.clone();
        let shown = self.shown_rect();
        let crop_mode = self.tool == Tool::Crop;
        let backdrop = self.doc.backdrop.filter(|_| !crop_mode);
        let clip = (!crop_mode).then_some(shown);
        let arrow_path = self.selected_arrow_view();
        let accent = t.accent;
        let corners = backdrop.map(|b| corner_pieces(&b, shown, &vp)).unwrap_or_default();
        let painter = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let mut cache = scene.borrow_mut();
                cache.release(window);
                let origin = bounds.origin;
                let draw = |window: &mut Window| {
                    paint::paint(&cache.layers, &vp, origin, window);
                    paint::paint(&cache.crisp, &vp, origin, window);
                    paint::paint(&cache.base, &vp, origin, window);
                    paint::paint(&cache.live, &vp, origin, window);
                    paint::paint(&cache.above, &vp, origin, window);
                };
                match clip {
                    Some(c) => {
                        let mask = ContentMask { bounds: paint::view_bounds(&vp, origin, c) };
                        window.with_content_mask(Some(mask), draw);
                    }
                    None => draw(window),
                }
                for (poly, color) in &corners {
                    let mut pb = PathBuilder::fill();
                    let pts: Vec<Point<Pixels>> = poly.iter().map(|p| point(origin.x + px(p.x), origin.y + px(p.y))).collect();
                    pb.add_polygon(&pts, true);
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, *color);
                    }
                }
                if let Some((a, c, b)) = arrow_path {
                    let o = |p: Pt| point(origin.x + px(p.x), origin.y + px(p.y));
                    let mut pb = PathBuilder::stroke(px(1.)).dash_array(&[px(3.), px(3.)]);
                    pb.move_to(o(a));
                    match c {
                        Some(c) => pb.curve_to(o(b), o(c)),
                        None => pb.line_to(o(b)),
                    }
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, accent);
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full();

        let mut area = div()
            .id("canvas-area")
            .flex_1()
            .min_w_0()
            .relative()
            .overflow_hidden()
            .bg(t.canvas)
            .cursor(self.cursor_style())
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev, w, cx| this.mouse_down(ev, w, cx)))
            .on_mouse_down(MouseButton::Middle, cx.listener(|this, ev, w, cx| this.mouse_down(ev, w, cx)))
            .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.scroll(ev, cx)))
            .on_pinch(cx.listener(|this, ev, _, cx| this.pinch(ev, cx)));
        if let Some(b) = backdrop {
            let scene = self.scene.clone();
            area = area.child(
                canvas(|_, _, _| {}, move |bounds, _, window, _| {
                    if let Some(prim) = &scene.borrow().backdrop {
                        paint::paint(std::slice::from_ref(prim), &vp, bounds.origin, window);
                    }
                })
                .absolute()
                .inset_0()
                .size_full(),
            );
            area = area.child(self.shadow_view(&b, shown));
        }
        area = area.child(painter);
        area = area.children(self.selection_overlay(t));
        if crop_mode {
            area = area.children(self.crop_overlay(t));
        }
        area = area.children(self.caret());
        if self.inspector {
            area = area.child(div().absolute().top(px(4.)).right(px(4.)).child(self.inspector_panel(t, dark, cx)));
        }
        area
    }

    fn shadow_view(&self, b: &Backdrop, content: RectF) -> impl IntoElement {
        let img = self.vp.rect_to_view(content);
        let z = self.vp.zoom;
        div()
            .absolute()
            .left(px(img.x))
            .top(px(img.y))
            .w(px(img.w))
            .h(px(img.h))
            .rounded(px(b.radius * z))
            .when(b.shadow > 0., |d| {
                d.shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.43),
                    offset: point(px(0.), px(b.shadow * 0.35 * z)),
                    blur_radius: px(b.shadow * z),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
    }

    fn selected_arrow_view(&self) -> Option<(Pt, Option<Pt>, Pt)> {
        if matches!(self.gesture, Gesture::Draw { .. }) {
            return None;
        }
        match self.selected_shape()? {
            Shape::Arrow { from, to, control, kind } => {
                let c = geom::arrow_control(*from, *to, *control, *kind);
                Some((self.vp.to_view(*from), c.map(|c| self.vp.to_view(c)), self.vp.to_view(*to)))
            }
            _ => None,
        }
    }

    fn selection_overlay(&self, t: &Theme) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if matches!(self.gesture, Gesture::Draw { .. } | Gesture::Place { .. }) || self.text.is_some() {
            if let Some(r) = self.text.as_ref().and_then(|e| self.doc.bounds(e.id)) {
                let v = self.vp.rect_to_view(r);
                out.push(div().absolute().left(px(v.x - 3.)).top(px(v.y - 3.)).w(px(v.w + 6.)).h(px(v.h + 6.)).border_1().border_dashed().border_color(t.accent).into_any_element());
            }
            return out;
        }
        let Some(id) = self.selected else { return out };
        let Some(a) = self.doc.get(id) else { return out };
        let handles = geom::handles(&a.shape);
        // A rectangle's own stroke already outlines it; a frame on top would hide it.
        let frame = match &a.shape {
            Shape::Arrow { .. } | Shape::Line { .. } | Shape::Measure { .. } | Shape::Rect { .. } => None,
            s => geom::frame(s).or_else(|| self.doc.bounds(id)),
        };
        if let Some(r) = frame {
            let v = self.vp.rect_to_view(r);
            out.push(div().absolute().left(px(v.x)).top(px(v.y)).w(px(v.w)).h(px(v.h)).border_1().border_color(t.accent).into_any_element());
        }
        let moving = matches!(self.gesture, Gesture::Edit { grab: Grab::Move, .. });
        for (h, p) in handles {
            let v = self.vp.to_view(p);
            let el = match h {
                Handle::Bend => div()
                    .absolute()
                    .left(px(v.x - 5.))
                    .top(px(v.y - 5.))
                    .size(px(10.))
                    .rounded_full()
                    .bg(t.accent)
                    .border(px(1.5))
                    .border_color(rgb(0xFFFFFF)),
                _ => div()
                    .absolute()
                    .left(px(v.x - 4.))
                    .top(px(v.y - 4.))
                    .size(px(8.))
                    .bg(rgb(0xFFFFFF))
                    .border(px(1.5))
                    .border_color(t.accent),
            };
            out.push(el.into_any_element());
            if h == Handle::Bend && !moving {
                out.push(
                    div()
                        .absolute()
                        .left(px(v.x + 12.))
                        .top(px(v.y + 14.))
                        .h(px(18.))
                        .px(px(5.))
                        .rounded(px(4.))
                        .bg(t.accent)
                        .text_color(t.accent_ink)
                        .font_family(MONO_FONT)
                        .text_size(px(11.))
                        .flex()
                        .items_center()
                        .whitespace_nowrap()
                        .child("bend · drag")
                        .into_any_element(),
                );
            }
        }
        out
    }

    fn crop_overlay(&self, t: &Theme) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let Some(r) = self.crop_draft else { return out };
        let full = self.vp.rect_to_view(self.canvas_bounds());
        let v = self.vp.rect_to_view(r);
        let scrim = rgba(0x0607098F);
        let piece = |x: f32, y: f32, w: f32, h: f32| {
            div().absolute().left(px(x)).top(px(y)).w(px(w.max(0.))).h(px(h.max(0.))).bg(scrim).into_any_element()
        };
        out.push(piece(full.x, full.y, full.w, v.y - full.y));
        out.push(piece(full.x, v.y + v.h, full.w, full.y + full.h - v.y - v.h));
        out.push(piece(full.x, v.y, v.x - full.x, v.h));
        out.push(piece(v.x + v.w, v.y, full.x + full.w - v.x - v.w, v.h));
        out.push(div().absolute().left(px(v.x)).top(px(v.y)).w(px(v.w)).h(px(v.h)).border_1().border_color(t.accent).into_any_element());
        for g in geom::BOX {
            let (x, y) = (v.x + v.w * g.0 as f32 / 2., v.y + v.h * g.1 as f32 / 2.);
            out.push(div().absolute().left(px(x - 4.)).top(px(y - 4.)).size(px(8.)).bg(rgb(0xFFFFFF)).border(px(1.5)).border_color(t.accent).into_any_element());
        }
        out.push(
            div()
                .absolute()
                .left(px(v.x))
                .top(px(v.y + v.h + 8.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(6.))
                .bg(t.hud)
                .flex()
                .items_center()
                .gap(px(8.))
                .child(mono(12.).text_color(t.text).child(format!("{} × {}", self.pt(r.w), self.pt(r.h))))
                .child(mono(11.).text_color(t.text3).child(format!("{} × {} px", r.w.round(), r.h.round())))
                .child(kbd(t, "↵"))
                .into_any_element(),
        );
        out
    }

    fn caret(&self) -> Option<AnyElement> {
        let edit = self.text.as_ref()?;
        let a = self.doc.get(edit.id)?;
        let Shape::Text { origin, text } = &a.shape else { return None };
        let fs = a.style.font_size * self.vp.zoom;
        let (pad_x, pad_y) = match a.style.text_background {
            TextBackground::Pill => (0.42 * fs, 0.16 * fs),
            TextBackground::Outline => (2. * self.vp.zoom, 2. * self.vp.zoom),
            TextBackground::None => (0., 0.),
        };
        let line_h = fs * 1.2;
        let pos = match self.doc.bounds(edit.id).filter(|_| !text.is_empty()) {
            Some(r) => {
                let v = self.vp.rect_to_view(r);
                Pt::new(v.x + v.w - pad_x + 1., v.y + v.h - pad_y - line_h)
            }
            None => self.vp.to_view(*origin),
        };
        Some(
            div()
                .absolute()
                .left(px(pos.x))
                .top(px(pos.y))
                .w(px(1.5))
                .h(px(line_h))
                .bg(caret_colour(&a.style))
                .into_any_element(),
        )
    }
}

/// The backdrop showing through the image's rounded corners, as view-space
/// polygons with the gradient's colour at each corner (the corners are small
/// enough for one colour each).
fn corner_pieces(b: &Backdrop, content: RectF, vp: &Viewport) -> Vec<(Vec<Pt>, Rgba)> {
    let frame = editor::backdrop_frame(content, b);
    let r = b.radius.min(content.w / 2.).min(content.h / 2.);
    if r <= 0. {
        return Vec::new();
    }
    let colour_at = |p: Pt| {
        let (dx, dy) = (p.x - frame.x, p.y - frame.y);
        let t = ((dx * frame.w + dy * frame.h) / (frame.w * frame.w + frame.h * frame.h)).clamp(0., 1.);
        let mix = |a: u8, b: u8| a as f32 / 255. * (1. - t) + b as f32 / 255. * t;
        Rgba { r: mix(b.from.r, b.to.r), g: mix(b.from.g, b.to.g), b: mix(b.from.b, b.to.b), a: 1. }
    };
    let (x0, y0, x1, y1) = (content.x, content.y, content.x + content.w, content.y + content.h);
    [(x0, y0, r, r), (x1, y0, -r, r), (x1, y1, -r, -r), (x0, y1, r, -r)]
        .into_iter()
        .map(|(cx, cy, sx, sy)| {
            let centre = Pt::new(cx + sx, cy + sy);
            let mut poly = vec![vp.to_view(Pt::new(cx, cy))];
            for i in 0..=12 {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / 12.;
                poly.push(vp.to_view(Pt::new(centre.x - sx * a.cos(), centre.y - sy * a.sin())));
            }
            (poly, colour_at(Pt::new(cx, cy)))
        })
        .collect()
}

/// Inside a pill the caret sits on the annotation colour, so it takes the
/// glyph ink instead.
fn caret_colour(s: &shattr_annotate::Style) -> Rgba {
    let c = s.color;
    if s.text_background != TextBackground::Pill {
        return gpui(c);
    }
    let luma = 0.2126 * c.r as f32 + 0.7152 * c.g as f32 + 0.0722 * c.b as f32;
    if luma > 160. { rgb(0x111111) } else { rgb(0xFFFFFF) }
}

/// Click steps up, shift-click steps down, the wheel steps either way.
fn stepper(
    t: &Theme,
    f: Stateful<Div>,
    name: &'static str,
    cx: &mut Context<Editor>,
    step: impl Fn(&mut Editor, i32) + Copy + 'static,
) -> Stateful<Div> {
    f.tooltip(tooltip_text(*t, format!("{name} · click, ⇧click or scroll")))
        .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
            step(this, if ev.modifiers().shift { -1 } else { 1 });
            cx.notify();
        }))
        .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, cx| {
            let dy = ev.delta.pixel_delta(px(20.)).y.as_f32();
            if dy.abs() >= 1. {
                step(this, if dy > 0. { 1 } else { -1 });
                cx.notify();
            }
            cx.stop_propagation();
        }))
}

fn slider(t: &Theme, value: f32, cx: &mut Context<Editor>) -> impl IntoElement {
    const W: f32 = 120.;
    const CELLS: usize = 20;
    let x = value.clamp(0., 1.) * W;
    let mut hit = div().absolute().inset_0().flex();
    for i in 0..CELLS {
        let v = (i + 1) as f32 / CELLS as f32;
        hit = hit.child(div().id(("opacity-cell", i)).flex_1().h_full().on_click(cx.listener(move |this, _, _, cx| {
            this.set_overlay_opacity(v);
            cx.notify();
        })));
    }
    div()
        .w(px(W))
        .h(px(14.))
        .flex()
        .items_center()
        .relative()
        .child(div().w(px(W)).h(px(4.)).rounded(px(2.)).bg(t.hover).child(div().w(px(x)).h_full().rounded(px(2.)).bg(t.text2)))
        .child(div().absolute().left(px(x - 7.)).top_0().size(px(14.)).rounded(px(7.)).bg(t.text).border(px(3.)).border_color(t.chrome))
        .child(hit)
}

fn window_controls(t: &Theme, window: &Window) -> impl IntoElement {
    let hover = t.hover;
    let control = |id: &'static str, i: EIcon, area: WindowControlArea, close: bool, t: &Theme| {
        let fg = t.text2;
        div()
            .id(id)
            .w(px(46.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(area)
            .hover(move |s| if close { s.bg(rgb(0xC42B1C)) } else { s.bg(hover) })
            .child(icon(i).size(px(16.)).text_color(fg))
    };
    let max_icon = if window.is_maximized() { EIcon::WinRestore } else { EIcon::WinMax };
    div()
        .flex()
        .flex_none()
        .h_full()
        .child(control("win-min", EIcon::WinMin, WindowControlArea::Min, false, t))
        .child(control("win-max", max_icon, WindowControlArea::Max, false, t))
        .child(control("win-close", EIcon::WinClose, WindowControlArea::Close, true, t))
}

struct Tip {
    text: String,
    theme: Theme,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(t.hover)
            .text_color(t.text)
            .font_family(UI_FONT)
            .text_size(px(12.))
            .shadow_lg()
            .whitespace_nowrap()
            .child(self.text.clone())
    }
}

pub fn tooltip_text(t: Theme, text: String) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Tip { text: text.clone(), theme: t }).into()
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.layout(window);
        self.refresh_scene();
        self.refresh_crisp(cx);
        let (t, dark) = theme(window);
        if self.flash.is_some() {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(1700)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.tick_flash() {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let title = self.title_bar(&t, window, cx).into_any_element();
        let props = self.props_bar(&t, cx).into_any_element();
        let strip = self.strip(&t, cx).into_any_element();
        let area = self.canvas_area(&t, dark, cx).into_any_element();
        let status = self.status_bar(&t).into_any_element();
        let tip = self.tool_tip(&t).map(|e| e.into_any_element());
        let palette = self.palette.is_some().then(|| self.palette_view(&t, dark, cx).into_any_element());
        div()
            .id("editor")
            .key_context("ShattrEditor")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(t.chrome)
            .text_color(t.text)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .line_height(px(18.))
            .on_key_down(cx.listener(|this, ev, w, cx| this.key_down(ev, w, cx)))
            .on_key_up(cx.listener(|this, ev, _, cx| this.key_up(ev, cx)))
            .on_mouse_move(cx.listener(|this, ev, _, cx| this.mouse_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev, _, cx| this.mouse_up(ev, cx)))
            .on_mouse_up(MouseButton::Middle, cx.listener(|this, ev, _, cx| this.mouse_up(ev, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, ev, _, cx| this.mouse_up(ev, cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.drop_paths(paths, cx)))
            .child(title)
            .child(props)
            .child(div().flex_1().min_h_0().flex().relative().child(strip).child(area).children(tip))
            .child(status)
            .children(palette)
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
