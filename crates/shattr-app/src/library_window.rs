//! The Library: every screenshot already on this computer, newest first,
//! plus the groups of copies that could be cleaned up. Read-only: it reveals
//! and copies files, it never deletes them.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use shattr_library::{Entry, IndexStatus, Library, LibraryEvent};
use shattr_ui::icons::{Icon, icon};
use shattr_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use shattr_ui::widgets::icon_button;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const CARD_W: f32 = 212.;
const THUMB_H: f32 = 128.;
const CARD_H: f32 = THUMB_H + 50.;
const GAP: f32 = 14.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    All,
    Duplicates,
}

pub enum LibraryAction {
    Reveal(PathBuf),
    Edit(PathBuf),
    Copy(PathBuf),
    Pin(PathBuf),
}

pub struct LibraryWindow {
    library: Library,
    entries: Vec<Entry>,
    groups: Vec<Vec<Entry>>,
    status: IndexStatus,
    view: View,
    search: Entity<InputState>,
    query: String,
    selected: Option<i64>,
    columns: usize,
    focus: FocusHandle,
}

impl EventEmitter<LibraryAction> for LibraryWindow {}

impl LibraryWindow {
    pub fn new(library: Library, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search file names"));
        cx.subscribe(&search, |this, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.query = input.read(cx).value().to_lowercase();
                cx.notify();
            }
        })
        .detach();

        let events = library.subscribe();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let mut changed = false;
            while let Ok(ev) = events.try_recv() {
                changed |= matches!(ev, LibraryEvent::Indexed(_) | LibraryEvent::Removed { .. } | LibraryEvent::ScanFinished);
            }
            let alive = this
                .update(cx, |this, cx| {
                    if changed {
                        this.reload(cx);
                    } else {
                        let s = this.library.status();
                        if s.pending != this.status.pending || s.scanning != this.status.scanning {
                            this.status = s;
                            cx.notify();
                        }
                    }
                })
                .is_ok();
            if !alive {
                break;
            }
        })
        .detach();

        let mut this = Self {
            entries: Vec::new(),
            groups: Vec::new(),
            status: library.status(),
            library,
            view: View::All,
            search,
            query: String::new(),
            selected: None,
            columns: 4,
            focus: cx.focus_handle(),
        };
        this.reload(cx);
        this
    }

    /// Duplicate grouping is quadratic in the worst case, so it runs off the
    /// UI thread and lands when ready.
    pub fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |s, cx| s.focus(window, cx));
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.entries = self.library.entries();
        self.status = self.library.status();
        let lib = self.library.clone();
        cx.spawn(async move |this, cx| {
            let groups = cx.background_executor().spawn(async move { lib.duplicate_groups() }).await;
            let _ = this.update(cx, |this, cx| {
                this.groups = groups;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn visible(&self) -> Vec<Entry> {
        if self.query.is_empty() {
            return self.entries.clone();
        }
        self.entries
            .iter()
            .filter(|e| e.path.to_string_lossy().to_lowercase().contains(&self.query))
            .cloned()
            .collect()
    }

    fn card(&self, e: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let id = e.id;
        let selected = self.selected == Some(id);
        let name = e.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let folder = e.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (reveal, copy, pin, open, edit) = (e.path.clone(), e.path.clone(), e.path.clone(), e.path.clone(), e.path.clone());
        let group: SharedString = format!("card-{id}").into();
        let thumb = match &e.thumb {
            Some(p) => img(p.clone()).size_full().object_fit(ObjectFit::Contain).into_any_element(),
            None => div().size_full().into_any_element(),
        };
        div()
            .id(("card", id as u64))
            .group(group.clone())
            .w(px(CARD_W))
            .h(px(CARD_H))
            .flex_none()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(T.chrome)
            .border_1()
            .border_color(if selected { T.accent } else { T.line })
            .overflow_hidden()
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                window.focus(&this.focus, cx);
                this.selected = Some(id);
                if ev.click_count() >= 2 {
                    cx.emit(LibraryAction::Edit(open.clone()));
                }
                cx.notify();
            }))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(THUMB_H))
                    .flex_none()
                    .overflow_hidden()
                    .bg(T.canvas)
                    .child(thumb)
                    .child(
                        div()
                            .absolute()
                            .top(px(6.))
                            .right(px(6.))
                            .flex()
                            .gap(px(2.))
                            .p(px(2.))
                            .rounded(px(7.))
                            .bg(T.hud)
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .child(icon_button(("edit", id as u64), Icon::Edit, "Edit", &T).on_click(cx.listener(move |_, _, _, cx| cx.emit(LibraryAction::Edit(edit.clone())))))
                            .child(icon_button(("copy", id as u64), Icon::Copy, "Copy image", &T).on_click(cx.listener(move |_, _, _, cx| cx.emit(LibraryAction::Copy(copy.clone())))))
                            .child(icon_button(("pin", id as u64), Icon::Pin, "Pin", &T).on_click(cx.listener(move |_, _, _, cx| cx.emit(LibraryAction::Pin(pin.clone())))))
                            .child(icon_button(("reveal", id as u64), Icon::Save, "Show in folder", &T).on_click(cx.listener(move |_, _, _, cx| cx.emit(LibraryAction::Reveal(reveal.clone()))))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(10.))
                    .py(px(7.))
                    .child(div().text_size(px(12.)).whitespace_nowrap().overflow_hidden().text_ellipsis().child(name))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .text_size(px(11.))
                            .text_color(T.text3)
                            .child(div().font_family(MONO_FONT).child(format!("{} × {}", e.width, e.height)))
                            .child(div().child("·"))
                            .child(div().child(age(e.modified)))
                            .child(div().flex_1().min_w_0().whitespace_nowrap().overflow_hidden().text_ellipsis().text_right().child(folder)),
                    ),
            )
            .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dup_files: usize = self.groups.iter().map(|g| g.len()).sum();
        let status = if self.status.scanning || self.status.pending > 0 {
            format!("Indexing · {} left", self.status.pending)
        } else {
            "Up to date".into()
        };
        let seg = |id: &'static str, label: String, on: bool, v: View, cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(26.))
                .px(px(10.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .when(on, |d| d.bg(T.hover).text_color(T.text))
                .when(!on, |d| d.text_color(T.text3).hover(|d| d.text_color(T.text)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.view = v;
                    cx.notify();
                }))
                .child(label)
        };
        div()
            .h(px(52.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(18.))
            .bg(T.chrome)
            .border_b_1()
            .border_color(T.line)
            .child(icon(Icon::Library).text_color(T.text2))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Library"))
            .child(
                div()
                    .flex()
                    .p(px(2.))
                    .rounded(px(7.))
                    .bg(T.control)
                    .child(seg("view-all", format!("All  {}", self.entries.len()), self.view == View::All, View::All, cx))
                    .child(seg(
                        "view-dups",
                        format!("Duplicates  {}", self.groups.len()),
                        self.view == View::Duplicates,
                        View::Duplicates,
                        cx,
                    )),
            )
            .child(div().w(px(240.)).child(Input::new(&self.search)))
            .child(div().flex_1())
            .when(dup_files > 0, |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(T.warn)
                        .child(format!("{dup_files} files in {} duplicate groups", self.groups.len())),
                )
            })
            .child(div().font_family(MONO_FONT).text_size(px(11.)).text_color(T.text3).child(status))
    }

    fn all_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let items = self.visible();
        if items.is_empty() {
            return empty(if self.query.is_empty() {
                "No screenshots found yet. Shattr looks in Pictures\\Screenshots, OneDrive, Game Bar captures, ShareX, Pictures\\Shattr, and screenshot-named files on the Desktop."
            } else {
                "Nothing matches that name."
            });
        }
        let width = window.bounds().size.width.as_f32() - 36.;
        self.columns = ((width + GAP) / (CARD_W + GAP)).floor().max(1.) as usize;
        let cols = self.columns;
        let rows = items.len().div_ceil(cols);
        uniform_list(
            "library-grid",
            rows,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|r| {
                        let start = r * cols;
                        let row: Vec<AnyElement> = items[start..(start + cols).min(items.len())].iter().map(|e| this.card(e, cx)).collect();
                        div().flex().gap(px(GAP)).pb(px(GAP)).children(row)
                    })
                    .collect()
            }),
        )
        .size_full()
        .px(px(18.))
        .pt(px(18.))
        .into_any_element()
    }

    fn duplicates_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.groups.is_empty() {
            return empty("No duplicate screenshots. Every capture you keep is unique.");
        }
        let groups = self.groups.clone();
        div()
            .id("dups")
            .size_full()
            .overflow_y_scroll()
            .p(px(18.))
            .flex()
            .flex_col()
            .gap(px(22.))
            .children(groups.into_iter().enumerate().map(|(i, g)| {
                let exact = g.windows(2).all(|w| w[0].width == w[1].width && w[0].height == w[1].height && w[0].hash.as_bytes() == w[1].hash.as_bytes());
                let bytes: u64 = g.iter().skip(1).map(|e| e.bytes).sum();
                div()
                    .id(("group", i))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_size(px(12.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(T.text).child(if exact { "Same image" } else { "Look alike · review" }))
                            .child(div().text_color(T.text3).child(format!("{} files · {} reclaimable", g.len(), human_bytes(bytes)))),
                    )
                    .child(div().flex().flex_wrap().gap(px(GAP)).children(g.iter().map(|e| self.card(e, cx)).collect::<Vec<_>>()))
            }))
            .into_any_element()
    }
}

fn empty(text: &'static str) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(div().max_w(px(460.)).text_center().text_size(px(13.)).text_color(T.text3).child(text))
        .into_any_element()
}

fn age(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        86400..2_592_000 => format!("{}d ago", s / 86400),
        _ => chrono::DateTime::<chrono::Local>::from(t).format("%d %b %Y").to_string(),
    }
}

fn human_bytes(b: u64) -> String {
    match b {
        0..1024 => format!("{b} B"),
        1024..1_048_576 => format!("{:.0} KB", b as f64 / 1024.),
        _ => format!("{:.1} MB", b as f64 / 1_048_576.),
    }
}

impl Render for LibraryWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.view {
            View::All => self.all_view(window, cx),
            View::Duplicates => self.duplicates_view(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let k = &ev.keystroke;
                if (k.modifiers.control || k.modifiers.platform) && (k.key == "k" || k.key == "f") {
                    this.focus_search(window, cx);
                    cx.stop_propagation();
                }
            }))
            .bg(T.canvas)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(T.text)
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}
