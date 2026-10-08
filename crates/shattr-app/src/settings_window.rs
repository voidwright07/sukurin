//! Settings: rebinding the global shortcuts. Click a shortcut, press the new
//! keys; changes apply immediately and are saved.

use crate::settings::Settings;
use crate::shortcuts::{self, ACTIONS, SLOTS};
use crate::tray::Resident;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use shattr_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use std::collections::HashMap;

pub struct SettingsWindow {
    settings: Settings,
    /// The slot waiting for a key press.
    recording: Option<(&'static str, usize)>,
    /// Why a slot's shortcut isn't active, keyed by action and shortcut.
    problems: HashMap<(&'static str, String), String>,
    /// A message for the row being edited (bad or duplicate shortcut).
    notice: Option<(&'static str, String)>,
    focus: FocusHandle,
}

impl SettingsWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            settings: Settings::load(),
            recording: None,
            problems: HashMap::new(),
            notice: None,
            focus: cx.focus_handle(),
        };
        cx.on_release(|this, cx| {
            cx.update_global::<Resident, _>(|r, _| r.apply(&this.settings));
        })
        .detach();
        this.apply(cx);
        this
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Registers the current settings with the OS and records what it refused.
    fn apply(&mut self, cx: &mut Context<Self>) {
        let refused = cx.update_global::<Resident, _>(|r, _| r.apply(&self.settings));
        self.problems = refused.into_iter().map(|r| ((r.action, r.shortcut), r.reason)).collect();
        cx.notify();
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.settings.save() {
            log::error!("could not save settings: {e:#}");
        }
        self.apply(cx);
    }

    fn start_recording(&mut self, id: &'static str, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Unregister everything while recording, or pressing an existing
        // shortcut would trigger it instead of being recorded.
        cx.update_global::<Resident, _>(|r, _| r.apply(&Settings { hotkeys: ACTIONS.iter().map(|a| (a.id.to_string(), vec![])).collect() }));
        self.recording = Some((id, slot));
        self.notice = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        self.apply(cx);
    }

    fn record(&mut self, text: Option<&str>, cx: &mut Context<Self>) {
        let Some((id, slot)) = self.recording else { return };
        match self.settings.set_shortcut(id, slot, text) {
            Ok(()) => {
                self.recording = None;
                self.notice = None;
                self.commit(cx);
            }
            Err(e) => {
                self.notice = Some((id, e));
                cx.notify();
            }
        }
    }

    fn key_down(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.recording.is_none() {
            return;
        }
        let k = &ev.keystroke;
        let m = k.modifiers;
        let plain = !(m.control || m.alt || m.shift || m.platform);
        match k.key.as_str() {
            "escape" if plain => self.stop_recording(cx),
            "backspace" | "delete" if plain => self.record(None, cx),
            key => {
                if let Some(text) = shortcuts::from_keystroke(key, m.control, m.alt, m.shift, m.platform) {
                    self.record(Some(&text), cx);
                }
            }
        }
        cx.stop_propagation();
    }

    fn chip(&self, id: &'static str, slot: usize, text: Option<String>, cx: &mut Context<Self>) -> impl IntoElement {
        let recording = self.recording == Some((id, slot));
        let problem = text.as_ref().and_then(|t| self.problems.get(&(id, t.clone())));
        let label = if recording {
            "Press keys…".to_string()
        } else {
            text.clone().unwrap_or_else(|| if slot == 0 { "None".into() } else { "Add".into() })
        };
        div()
            .id(SharedString::from(format!("{id}-{slot}")))
            .w(px(178.))
            .h(px(30.))
            .px(px(10.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .border_1()
            .bg(T.control)
            .border_color(if recording { T.accent } else { rgba(0x00000000) })
            .font_family(MONO_FONT)
            .text_size(px(12.))
            .text_color(match (&text, problem) {
                (_, _) if recording => T.text,
                (Some(_), Some(_)) => T.warn,
                (Some(_), None) => T.text,
                (None, _) => T.text3,
            })
            .when(problem.is_some() && !recording, |d| d.line_through())
            .hover(|d| d.border_color(T.line2))
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| this.start_recording(id, slot, window, cx)))
    }
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let path = Settings::path().map(|p| p.display().to_string()).unwrap_or_default();
        let mut rows = div().flex().flex_col().gap(px(2.));
        for a in ACTIONS {
            let current = self.settings.shortcuts(a.id);
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .py(px(6.))
                .child(div().flex_1().child(a.label));
            for slot in 0..SLOTS {
                row = row.child(self.chip(a.id, slot, current.get(slot).cloned(), cx));
            }
            let customised = self.settings.hotkeys.contains_key(a.id);
            let id = a.id;
            row = row.child(
                div()
                    .id(SharedString::from(format!("{id}-reset")))
                    .w(px(52.))
                    .text_size(px(12.))
                    .text_color(if customised { T.text2 } else { T.line2 })
                    .when(customised, |d| {
                        d.hover(|d| d.text_color(T.text)).on_click(cx.listener(move |this, _, _, cx| {
                            this.settings.reset(id);
                            this.recording = None;
                            this.notice = None;
                            this.commit(cx);
                        }))
                    })
                    .child("Reset"),
            );
            let problems: Vec<String> = current
                .iter()
                .filter_map(|t| self.problems.get(&(a.id, t.clone())).map(|r| format!("{t}: {r}")))
                .collect();
            let notice = self.notice.as_ref().filter(|(n, _)| *n == a.id).map(|(_, m)| m.clone());
            rows = rows.child(row).children(
                notice
                    .into_iter()
                    .chain(problems)
                    .map(|m| div().pl(px(2.)).pb(px(4.)).text_size(px(12.)).text_color(T.warn).child(m)),
            );
        }
        div()
            .id("settings")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(24.))
            .bg(T.chrome)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(T.text)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev, _, cx| this.key_down(ev, cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child("Global shortcuts"))
                    .child(div().text_size(px(12.)).text_color(T.text3).child(
                        "These work from any app. Click a shortcut and press the new keys. Esc cancels, Backspace removes it.",
                    )),
            )
            .child(rows)
            .when(self.recording.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(12.))
                        .text_color(T.text2)
                        .child("PrtSc can't be recorded from a window:")
                        .child(
                            div()
                                .id("use-prtsc")
                                .px(px(10.))
                                .h(px(26.))
                                .rounded(px(6.))
                                .bg(T.control)
                                .flex()
                                .items_center()
                                .hover(|d| d.bg(T.hover))
                                .child("Use PrtSc")
                                .on_click(cx.listener(|this, _, _, cx| this.record(Some("PrtSc"), cx))),
                        ),
                )
            })
            .child(div().flex_1())
            .child(div().text_size(px(11.)).text_color(T.text3).font_family(MONO_FONT).child(path))
    }
}
