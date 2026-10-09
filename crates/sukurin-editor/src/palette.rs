//! The command palette: fuzzy search over every tool and command.

use crate::editor::Editor;
use crate::icons::{EIcon, icon};
use crate::keys::{self, Action, Entry};
use crate::widgets::{kbd, label};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sukurin_ui::Theme;

const GROUPS: [&str; 6] = ["Tools", "Edit", "Canvas", "View", "Inspect", "Export"];
const VISIBLE_ROWS: usize = 12;

pub struct Hit {
    pub entry: Entry,
    pub matched: Vec<usize>,
}

/// Matching entries, grouped in palette order, best match first within a group.
pub fn search(query: &str) -> Vec<Hit> {
    let mut hits: Vec<(usize, i32, Hit)> = keys::entries()
        .into_iter()
        .filter_map(|entry| {
            let (score, matched) = keys::fuzzy(query, entry.name)?;
            let group = GROUPS.iter().position(|g| *g == entry.group).unwrap_or(GROUPS.len());
            Some((group, score, Hit { entry, matched }))
        })
        .collect();
    if !query.is_empty() {
        let best: Vec<i32> = (0..=GROUPS.len()).map(|g| hits.iter().filter(|h| h.0 == g).map(|h| h.1).min().unwrap_or(i32::MAX)).collect();
        hits.sort_by_key(|h| (best[h.0], h.0, h.1));
    }
    hits.into_iter().map(|h| h.2).collect()
}

impl Editor {
    pub fn palette_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.palette.as_mut() else { return };
        let k = &ev.keystroke;
        let count = search(&p.query).len();
        match k.key.as_str() {
            "escape" => self.palette = None,
            "up" => p.selected = p.selected.checked_sub(1).unwrap_or(count.saturating_sub(1)),
            "down" => p.selected = if p.selected + 1 >= count { 0 } else { p.selected + 1 },
            "enter" => {
                let keep = k.modifiers.secondary();
                let hits = search(&p.query);
                if let Some(hit) = hits.get(p.selected).filter(|h| !h.entry.soon) {
                    let action = hit.entry.action;
                    if !keep {
                        self.palette = None;
                    }
                    if action != Action::Palette {
                        self.run(action, window, cx);
                    }
                }
            }
            "backspace" => {
                p.query.pop();
                p.selected = 0;
            }
            _ if k.modifiers.secondary() && k.key == "k" => self.palette = None,
            _ => {
                if let Some(c) = k.key_char.as_deref().filter(|c| !c.chars().any(char::is_control))
                    && crate::editor::is_typing(k)
                {
                    p.query.push_str(c);
                    p.selected = 0;
                }
            }
        }
        cx.notify();
    }

    pub fn palette_view(&self, t: &Theme, dark: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette.as_ref().expect("palette is open");
        let hits = search(&p.query);
        let selected = p.selected.min(hits.len().saturating_sub(1));
        let start = selected.saturating_sub(VISIBLE_ROWS - 1);
        let mut list = div().p(px(8.)).flex().flex_col().gap(px(2.));
        let mut last_group = "";
        for (i, hit) in hits.iter().enumerate().skip(start).take(VISIBLE_ROWS) {
            if hit.entry.group != last_group {
                last_group = hit.entry.group;
                let top = if i == start { 6. } else { 10. };
                list = list.child(div().pt(px(top)).pb(px(4.)).px(px(10.)).child(label(t, hit.entry.group)));
            }
            list = list.child(self.palette_row(t, i, hit, i == selected, cx));
        }
        if hits.is_empty() {
            list = list.child(div().h(px(32.)).px(px(10.)).flex().items_center().text_color(t.text3).child("No matching commands"));
        }
        let scrim = if dark { rgba(0x08090B59) } else { rgba(0x14151826) };
        let caret = t.accent;
        div()
            .id("palette-scrim")
            .absolute()
            .inset_0()
            .bg(scrim)
            .flex()
            .justify_center()
            .items_start()
            .pt(px(96.))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.palette = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("palette")
                    .w(px(600.))
                    .max_h(px(600.))
                    .flex()
                    .flex_col()
                    .bg(t.chrome)
                    .rounded(px(12.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(if dark { rgba(0xFFFFFF14) } else { rgba(0x14151814) })
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., if dark { 0.55 } else { 0.2 }),
                        offset: point(px(0.), px(24.)),
                        blur_radius: px(64.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(48.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(10.))
                            .px(px(16.))
                            .border_b_1()
                            .border_color(t.line)
                            .child(icon(EIcon::Search).text_color(t.text3))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .text_size(px(15.))
                                    .when(p.query.is_empty(), |d| d.child(div().w(px(1.5)).h(px(18.)).bg(caret)).child(div().text_color(t.text3).child("Search commands")))
                                    .when(!p.query.is_empty(), |d| d.child(p.query.clone()).child(div().w(px(1.5)).h(px(18.)).bg(caret))),
                            )
                            .child(kbd(t, "esc")),
                    )
                    .child(list)
                    .child(
                        div()
                            .h(px(36.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(16.))
                            .px(px(16.))
                            .border_t_1()
                            .border_color(t.line)
                            .text_size(px(12.))
                            .text_color(t.text3)
                            .child(div().flex().items_center().gap(px(6.)).child(kbd(t, "↑")).child(kbd(t, "↓")).child("move"))
                            .child(div().flex().items_center().gap(px(6.)).child(kbd(t, "↵")).child("run"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(kbd(t, if cfg!(target_os = "macos") { "⌘↵" } else { "Ctrl+↵" }))
                                    .child("run and keep open"),
                            )
                            .child(div().flex_1())
                            .child(format!("{} commands", hits.len())),
                    ),
            )
    }

    fn palette_row(&self, t: &Theme, i: usize, hit: &Hit, selected: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let e = &hit.entry;
        let mut highlights = Vec::new();
        let starts: Vec<(usize, char)> = e.name.char_indices().collect();
        for &ci in &hit.matched {
            if let Some(&(b, ch)) = starts.get(ci) {
                highlights.push((b..b + ch.len_utf8(), HighlightStyle { color: Some(t.text.into()), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() }));
            }
        }
        let text = StyledText::new(e.name).with_highlights(highlights);
        let fg = if e.soon { t.line2 } else if selected { t.text } else { t.text2 };
        let action = e.action;
        let soon = e.soon;
        let accent_soft = t.accent_soft;
        div()
            .id(("palette-row", i))
            .h(px(32.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .rounded(px(6.))
            .text_color(fg)
            .when(selected, |d| d.bg(accent_soft))
            .child(icon(e.icon).text_color(if selected { t.text } else { t.text3 }))
            .child(div().flex_1().whitespace_nowrap().overflow_hidden().child(text))
            .when(soon, |d| d.child(div().text_size(px(12.)).text_color(t.text3).child("not built yet")))
            .when(!e.hint.is_empty() && selected, |d| d.child(div().text_size(px(12.)).text_color(t.text3).whitespace_nowrap().child(e.hint)))
            .when_some(e.key, |d, k| d.child(kbd(t, k.label())))
            .on_click(cx.listener(move |this, _, window, cx| {
                if soon {
                    return;
                }
                this.palette = None;
                this.run(action, window, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::search;
    use crate::keys;

    #[test]
    fn arr_finds_the_arrow_tool_first() {
        let hits = search("arr");
        assert_eq!(hits[0].entry.name, "Arrow");
        assert!(hits.iter().any(|h| h.entry.name == "Arrange captures · row"));
    }

    #[test]
    fn empty_query_lists_every_tool_and_command_grouped() {
        let hits = search("");
        assert_eq!(hits.len(), keys::entries().len());
        let first_export = hits.iter().position(|h| h.entry.group == "Export").unwrap();
        assert!(hits[..first_export].iter().all(|h| h.entry.group != "Export"));
    }
}
