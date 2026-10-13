//! Result card for Text captures: what was recognised, already on the
//! clipboard. Never takes focus.

use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use sukurin_ui::icons::{Icon, icon};
use sukurin_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use sukurin_ui::widgets::{icon_button, kbd};
use std::time::{Duration, Instant};

pub const WIDTH: f32 = 380.;
const LINGER: Duration = Duration::from_secs(8);
const MAX_LINES: usize = 8;

#[derive(Clone)]
pub enum Content {
    Text { lines: Vec<String>, language: String, millis: u128 },
    Code { text: String },
    Nothing { reason: String },
    Saved { path: std::path::PathBuf },
    /// A short notice: a title and a few wrapped lines.
    Note { title: String, lines: Vec<String> },
}

#[derive(Clone)]
pub enum ToastAction {
    Copy(String),
    Open(String),
    Reveal(std::path::PathBuf),
    Dismiss,
}

pub struct Toast {
    content: Content,
    joined: bool,
    hovered: bool,
    shown_at: Instant,
}

impl EventEmitter<ToastAction> for Toast {}

impl Toast {
    pub fn new(content: Content) -> Self {
        Self { content, joined: false, hovered: false, shown_at: Instant::now() }
    }

    pub fn height(content: &Content) -> f32 {
        match content {
            Content::Text { lines, .. } => {
                let more = if lines.len() > MAX_LINES { 18. } else { 0. };
                40. + 20. + 18. * lines.len().clamp(1, MAX_LINES) as f32 + more + 46.
            }
            Content::Code { .. } => 40. + 58. + 46.,
            Content::Nothing { .. } | Content::Saved { .. } => 44.,
            Content::Note { lines, .. } => 44. + 18. * lines.len() as f32,
        }
    }

    pub fn should_close(&self) -> bool {
        !self.hovered && self.shown_at.elapsed() >= LINGER
    }

    /// What goes on the clipboard: lines as recognised, or joined into one
    /// paragraph for text that wrapped on screen.
    pub fn clipboard_text(&self) -> String {
        match &self.content {
            Content::Text { lines, .. } if self.joined => lines.join(" "),
            Content::Text { lines, .. } => lines.join("\n"),
            Content::Code { text } => text.clone(),
            Content::Nothing { .. } | Content::Saved { .. } | Content::Note { .. } => String::new(),
        }
    }

    fn header(&self, i: Icon, title: String, meta: Option<String>, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(8.))
            .child(icon(i).text_color(T.text2))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .when_some(meta, |d, m| d.child(div().font_family(MONO_FONT).text_size(px(11.)).text_color(T.text3).child(m)))
            .child(div().flex_1())
            .child(icon_button("dismiss", Icon::Close, "Dismiss  Esc", &T).on_click(cx.listener(|_, _, _, cx| cx.emit(ToastAction::Dismiss))))
    }

    fn button(id: &'static str, label: &'static str, key: &'static str) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(26.))
            .pl(px(8.))
            .pr(px(6.))
            .rounded(px(6.))
            .bg(T.control)
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .hover(|d| d.bg(T.hover))
            .child(label)
            .child(kbd(key))
    }
}

impl Render for Toast {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = div()
            .id("toast")
            .size_full()
            .flex()
            .flex_col()
            .bg(T.chrome)
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xFFFFFF14))
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(T.text)
            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                this.hovered = *h;
                if !*h {
                    this.shown_at = Instant::now();
                }
                cx.notify();
            }));
        match self.content.clone() {
            Content::Text { lines, language, millis } => {
                let shown = lines.len().min(MAX_LINES);
                let more = lines.len() - shown;
                let body = div()
                    .mx(px(12.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(T.control)
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .flex()
                    .flex_col()
                    .children(lines.iter().take(shown).map(|l| {
                        div().whitespace_nowrap().overflow_hidden().text_ellipsis().child(l.clone())
                    }))
                    .when(more > 0, |d| d.child(div().text_color(T.text3).child(format!("+{more} more lines"))));
                let copy_text = self.clipboard_text();
                shell
                    .child(self.header(
                        Icon::Text,
                        format!("Copied {} line{}", lines.len(), if lines.len() == 1 { "" } else { "s" }),
                        Some(format!("{} · {} ms", language.to_uppercase(), millis)),
                        cx,
                    ))
                    .child(body)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .p(px(10.))
                            .px(px(12.))
                            .child(
                                Self::button("join", if self.joined { "Keep lines" } else { "Join lines" }, "J").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.joined = !this.joined;
                                        cx.emit(ToastAction::Copy(this.clipboard_text()));
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(Self::button("copy", "Copy again", "C").on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(ToastAction::Copy(copy_text.clone()))
                            }))),
                    )
            }
            Content::Code { text } => {
                let (open, copy) = (text.clone(), text.clone());
                let is_link = text.starts_with("http://") || text.starts_with("https://");
                shell
                    .child(self.header(Icon::Text, if is_link { "QR code · URL".into() } else { "QR code".into() }, Some("copied".into()), cx))
                    .child(
                        div()
                            .mx(px(12.))
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .bg(T.control)
                            .font_family(MONO_FONT)
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .overflow_hidden()
                            .child(text.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .p(px(10.))
                            .px(px(12.))
                            .when(is_link, |d| {
                                d.child(Self::button("open", "Open", "↵").on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(ToastAction::Open(open.clone()))
                                })))
                            })
                            .child(Self::button("copy", "Copy", "C").on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(ToastAction::Copy(copy.clone()))
                            }))),
                    )
            }
            Content::Saved { path } => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                shell.child(
                    div()
                        .h(px(44.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(14.))
                        .child(icon(Icon::Check).text_color(T.text2))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Saved"))
                        .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(T.text3).overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
                        .child(Self::button("reveal", "Show", "↵").on_click(cx.listener(move |_, _, _, cx| cx.emit(ToastAction::Reveal(path.clone()))))),
                )
            }
            Content::Nothing { reason } => shell.child(
                div()
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(14.))
                    .child(icon(Icon::Text).text_color(T.text3))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("No text found"))
                    .child(div().flex_1().text_size(px(12.)).text_color(T.text3).overflow_hidden().text_ellipsis().whitespace_nowrap().child(reason)),
            ),
            Content::Note { title, lines } => shell.child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(14.))
                    .py(px(12.))
                    .child(div().h(px(20.)).flex().items_center().gap(px(8.)).child(icon(Icon::Screen).text_color(T.text2)).child(div().font_weight(FontWeight::SEMIBOLD).child(title)))
                    .children(lines.into_iter().map(|l| div().text_size(px(12.)).line_height(px(18.)).text_color(T.text3).whitespace_nowrap().overflow_hidden().text_ellipsis().child(l))),
            ),
        }
    }
}
