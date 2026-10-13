//! Settings: rebinding the global shortcuts. Click a shortcut, press the new
//! keys; changes apply immediately and are saved.

use crate::app::{self, Command};
#[cfg(target_os = "linux")]
use crate::screenshot_grant::{self, Grant};
use crate::settings::Settings;
use crate::shortcuts::{self, ACTIONS, RecordKey, SLOTS};
use crate::system_keys;
use crate::tray::{Problem, Resident};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sukurin_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use std::collections::HashMap;

pub struct SettingsWindow {
    settings: Settings,
    /// The slot waiting for a key press.
    recording: Option<(&'static str, usize)>,
    /// Why a slot's shortcut isn't active, keyed by action and shortcut.
    problems: HashMap<(&'static str, String), Problem>,
    /// A message for the row being edited (bad or duplicate shortcut).
    notice: Option<(&'static str, String)>,
    /// Sukurin switched Snipping Tool off PrtSc from this window.
    released: bool,
    /// What GNOME answered the screenshot permission request made from here.
    #[cfg(target_os = "linux")]
    grant: Grant,
    focus: FocusHandle,
}

impl SettingsWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            settings: Settings::load(),
            recording: None,
            problems: HashMap::new(),
            notice: None,
            released: false,
            #[cfg(target_os = "linux")]
            grant: Grant::default(),
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
        self.problems = refused.into_iter().map(|r| ((r.action, r.shortcut), r.problem)).collect();
        cx.notify();
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.settings.save() {
            log::error!("could not save settings: {e:#}");
        }
        self.apply(cx);
    }

    fn start_recording(&mut self, id: &'static str, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.update_global::<Resident, _>(|r, _| r.apply(&Settings::unbound()));
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
        let m = ev.keystroke.modifiers;
        match shortcuts::record_key(&ev.keystroke.key, m.control, m.alt, m.shift, m.platform) {
            RecordKey::Cancel => self.stop_recording(cx),
            RecordKey::Clear => self.record(None, cx),
            RecordKey::Record(text) => self.record(Some(&text), cx),
            RecordKey::Ignore => {}
        }
        cx.stop_propagation();
    }

    fn release_prtsc(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            // Broadcasting the setting change waits on every top-level window.
            let released = cx.background_executor().spawn(async { system_keys::release_prtsc() }).await;
            if let Err(e) = &released {
                log::error!("could not release PrtSc: {e:#}");
            }
            let _ = this.update(cx, |this, cx| {
                this.released |= released.is_ok();
                this.apply(cx);
            });
        })
        .detach();
    }

    #[cfg(target_os = "linux")]
    fn ask_grant(&mut self, cx: &mut Context<Self>) {
        self.grant = Grant::Asking;
        cx.notify();
        Grant::ask(cx, |this, grant, cx| {
            this.grant = grant;
            cx.notify();
        });
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
                .filter_map(|t| self.problems.get(&(a.id, t.clone())).map(|p| format!("{t}: {}", p.message())))
                .collect();
            let notice = self.notice.as_ref().filter(|(n, _)| *n == a.id).map(|(_, m)| m.clone());
            rows = rows.child(row).children(
                notice
                    .into_iter()
                    .chain(problems)
                    .map(|m| div().pl(px(2.)).pb(px(4.)).text_size(px(12.)).text_color(T.warn).child(m)),
            );
        }
        let root = div()
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
                        .child(small_button("use-prtsc", "Use PrtSc").on_click(cx.listener(|this, _, _, cx| this.record(Some("PrtSc"), cx)))),
                )
            })
            .when(self.problems.values().any(|p| *p == Problem::Snipping), |d| {
                d.child(
                    div().flex().child(
                        small_button("release-prtsc", "Let Sukurin use PrtSc").on_click(cx.listener(|this, _, _, cx| this.release_prtsc(cx))),
                    ),
                )
            })
            .when(self.released, |d| d.child(div().text_size(px(12.)).text_color(T.text2).child(system_keys::SIGN_OUT_HINT)));
        #[cfg(target_os = "linux")]
        let root = root.when(crate::capture::wayland(), |d| {
            d.child(screenshot_grant::section(self.grant, cx.listener(|this, _, _, cx| this.ask_grant(cx))))
        });
        root.child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().text_size(px(11.)).text_color(T.text3).font_family(MONO_FONT).child(path))
                    .child(
                        div()
                            .id("run-setup")
                            .text_size(px(12.))
                            .text_color(T.text2)
                            .hover(|d| d.text_color(T.text))
                            .child("Run setup again")
                            .on_click(|_, window, cx| {
                                window.remove_window();
                                cx.defer(|cx| app::run(Command::OpenOnboarding, cx));
                            }),
                    ),
            )
    }
}

fn small_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(10.))
        .h(px(26.))
        .rounded(px(6.))
        .bg(T.control)
        .flex()
        .items_center()
        .text_size(px(12.))
        .text_color(T.text)
        .hover(|d| d.bg(T.hover))
        .child(label)
}
