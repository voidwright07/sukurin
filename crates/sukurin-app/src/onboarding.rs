//! First-run setup: pick the capture shortcut, take PrtSc back from Snipping
//! Tool if needed, then set the few choices worth making up front. Closing the
//! window at any step counts as done.

use crate::app::{self, Command};
use crate::autostart;
use crate::overlay::Mode;
use crate::settings::{AiChat, Settings};
use crate::shortcuts::{self, ACTIONS, RecordKey};
use crate::system_keys;
use crate::tray::{Problem, Resident};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::time::Duration;
use sukurin_ui::theme::{DARK as T, MONO_FONT, UI_FONT};
use sukurin_ui::{Icon, icon, kbd, mark};

const CAPTURE: &str = "capture-area";

/// The cards besides Custom, and what each key is.
const PRESETS: [(&str, &str); 2] = [("PrtSc", "The Print Screen key"), ("Ctrl+Shift+S", "Works on any keyboard")];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Welcome,
    Shortcut,
    Options,
    Ready,
}

const STEPS: [Step; 4] = [Step::Welcome, Step::Shortcut, Step::Options, Step::Ready];

/// What a shortcut card says under its key.
enum Status {
    Ready,
    Available,
    Blocked(Problem),
}

pub struct Onboarding {
    step: Step,
    settings: Settings,
    /// Waiting for the keys of a custom shortcut.
    recording: bool,
    /// Why the last pick was refused (malformed or used by another action).
    notice: Option<String>,
    /// Starts on: a tray app that doesn't start at sign-in is gone after a reboot.
    autostart: bool,
    problems: HashMap<(&'static str, String), Problem>,
    /// Whether each preset could be registered right now.
    presets: HashMap<&'static str, Result<(), Problem>>,
    /// Whether Snipping Tool held PrtSc at the last apply.
    snipping: bool,
    /// Sukurin switched Snipping Tool off PrtSc from this window.
    released: bool,
    focus: FocusHandle,
}

impl Onboarding {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            step: Step::Welcome,
            settings: Settings::load(),
            recording: false,
            notice: None,
            autostart: true,
            problems: HashMap::new(),
            presets: HashMap::new(),
            snipping: false,
            released: false,
            focus: cx.focus_handle(),
        };
        cx.on_release(|this, cx| {
            this.settings.onboarded = true;
            if let Err(e) = this.settings.save() {
                log::error!("could not save settings: {e:#}");
            }
            cx.update_global::<Resident, _>(|r, _| r.apply(&this.settings));
        })
        .detach();
        // Picks up the user flipping the setting in Windows Settings.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let checked = this.update(cx, |this, cx| {
                if !this.recording && system_keys::snipping_owns_prtsc() != this.snipping {
                    this.apply(cx);
                }
            });
            if checked.is_err() {
                break;
            }
        })
        .detach();
        this.apply(cx);
        this
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        self.snipping = system_keys::snipping_owns_prtsc();
        let refused = cx.update_global::<Resident, _>(|r, _| r.apply(&self.settings));
        self.problems = refused.into_iter().map(|r| ((r.action, r.shortcut), r.problem)).collect();
        self.presets = cx.update_global::<Resident, _>(|r, _| {
            PRESETS.iter().map(|(k, _)| (*k, r.probe(&shortcuts::parse(k).expect("presets parse")))).collect()
        });
        cx.notify();
    }

    fn selected(&self) -> Option<String> {
        self.settings.shortcuts(CAPTURE).into_iter().next()
    }

    /// The first shortcut of `id` that the OS accepted.
    fn active(&self, id: &'static str) -> Option<String> {
        self.settings.shortcuts(id).into_iter().find(|t| !self.problems.contains_key(&(id, t.clone())))
    }

    fn status(&self, text: &str, selected: bool) -> Option<Status> {
        if selected {
            return Some(match self.problems.get(&(CAPTURE, text.to_string())) {
                Some(p) => Status::Blocked(p.clone()),
                None => Status::Ready,
            });
        }
        self.presets.get(text).map(|r| match r {
            Ok(()) => Status::Available,
            Err(p) => Status::Blocked(p.clone()),
        })
    }

    fn pick(&mut self, text: &str, cx: &mut Context<Self>) {
        self.recording = false;
        match self.choose(text, cx) {
            Ok(()) => self.notice = None,
            Err(e) => {
                self.notice = Some(e);
                self.apply(cx);
            }
        }
    }

    /// Makes `text` the capture shortcut. While the OS refuses it, a free
    /// preset stays bound too, so setup never ends without a working key.
    fn choose(&mut self, text: &str, cx: &mut Context<Self>) -> Result<(), String> {
        self.settings.set_sole_shortcut(CAPTURE, text)?;
        self.apply(cx);
        let chosen = self.selected();
        let blocked = chosen.as_ref().is_some_and(|c| self.problems.contains_key(&(CAPTURE, c.clone())));
        let fallback = PRESETS.iter().map(|(k, _)| *k).find(|k| chosen.as_deref() != Some(*k) && self.presets.get(k).is_some_and(Result::is_ok));
        if blocked
            && let Some(k) = fallback
            && self.settings.set_shortcut(CAPTURE, 1, Some(k)).is_ok()
        {
            self.apply(cx);
        }
        Ok(())
    }

    fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.update_global::<Resident, _>(|r, _| r.apply(&Settings::unbound()));
        self.recording = true;
        self.notice = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = false;
        self.apply(cx);
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

    fn go(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.recording {
            self.stop_recording(cx);
        }
        self.step = step;
        cx.notify();
    }

    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step {
            Step::Welcome => self.go(Step::Shortcut, cx),
            Step::Shortcut => self.go(Step::Options, cx),
            Step::Options => self.go(Step::Ready, cx),
            Step::Ready => self.finish(window),
        }
    }

    /// Saving and re-registering happen on release, which closing the window
    /// by its title bar also triggers.
    fn finish(&mut self, window: &mut Window) {
        if cfg!(windows)
            && self.autostart != autostart::is_enabled()
            && let Err(e) = autostart::set(self.autostart)
        {
            log::error!("could not change start-up setting: {e:#}");
        }
        window.remove_window();
    }

    fn try_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(window);
        // Started any sooner, the capture would include this window.
        App::spawn(cx, async move |cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            cx.update(|cx| app::run(Command::Capture(Mode::Area), cx));
        })
        .detach();
    }

    fn key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recording {
            if ev.keystroke.key == "enter" {
                self.advance(window, cx);
            }
            return;
        }
        let m = ev.keystroke.modifiers;
        match shortcuts::record_key(&ev.keystroke.key, m.control, m.alt, m.shift, m.platform) {
            RecordKey::Cancel | RecordKey::Clear => self.stop_recording(cx),
            RecordKey::Record(text) => match self.choose(&text, cx) {
                Ok(()) => {
                    self.recording = false;
                    self.notice = None;
                    cx.notify();
                }
                Err(e) => {
                    self.notice = Some(e);
                    cx.notify();
                }
            },
            RecordKey::Ignore => {}
        }
        cx.stop_propagation();
    }

    fn welcome(&self) -> Div {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .child(mark(px(48.), &T))
            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Sukurin"))
            .child(div().max_w(px(380.)).text_center().text_color(T.text2).child(
                "Sukurin captures, measures and annotates your screen from the system tray, out of sight until you press its shortcut.",
            ))
    }

    fn shortcut_step(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.selected();
        let mut cards = div().flex().gap(px(12.));
        for (key, about) in PRESETS {
            let on = !self.recording && selected.as_deref() == Some(key);
            cards = cards.child(
                card(key, key.into(), about, on, self.status(key, on)).on_click(cx.listener(move |this, _, _, cx| this.pick(key, cx))),
            );
        }
        let custom = selected.filter(|s| PRESETS.iter().all(|(k, _)| k != s));
        let (title, about) = match (&custom, self.recording) {
            (_, true) => ("Press keys…".to_string(), "Esc cancels"),
            (Some(c), false) => (c.clone(), "Click to record another"),
            (None, false) => ("Custom".to_string(), "Click, then press your keys"),
        };
        let status = custom.as_deref().filter(|_| !self.recording).and_then(|c| self.status(c, true));
        cards = cards.child(
            card("custom", title, about, self.recording || custom.is_some(), status)
                .on_click(cx.listener(|this, _, window, cx| this.start_recording(window, cx))),
        );

        let snipping =
            !self.recording && self.selected().is_some_and(|s| self.problems.get(&(CAPTURE, s)) == Some(&Problem::Snipping));
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(header("Pick your capture shortcut", "It opens the capture overlay from any app. The other shortcuts can be changed later in Settings."))
            .child(cards)
            .children(self.notice.clone().map(|n| div().text_size(px(12.)).text_color(T.warn).child(n)))
            .when(self.released, |d| d.child(div().text_size(px(12.)).text_color(T.text3).child(system_keys::SIGN_OUT_HINT)))
            .when(snipping, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .p(px(14.))
                        .rounded(px(8.))
                        .bg(T.control)
                        .child(
                            div().text_color(T.text2).child("Windows opens Snipping Tool when you press PrtSc. Sukurin can switch that off."),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    button("release-prtsc", "Use PrtSc for Sukurin", true)
                                        .on_click(cx.listener(|this, _, _, cx| this.release_prtsc(cx))),
                                )
                                .child(
                                    button("windows-settings", "Open Windows settings", false)
                                        .on_click(|_, _, _| system_keys::open_keyboard_settings()),
                                ),
                        ),
                )
            })
    }

    fn options_step(&self, cx: &mut Context<Self>) -> Div {
        let mut rows = div().flex().flex_col().rounded(px(8.)).bg(T.control);
        if cfg!(windows) {
            rows = rows.child(
                option("autostart", "Start Sukurin when you sign in", "Its shortcuts only work while it's running.", switch(self.autostart))
                    .hover(|d| d.bg(T.hover))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.autostart = !this.autostart;
                        cx.notify();
                    })),
            );
        }
        rows = rows
            .child(
                option(
                    "auto-save",
                    "Save every capture",
                    "Keeps a copy in Pictures\\Sukurin. Captures always go to the clipboard.",
                    switch(self.settings.auto_save),
                )
                .hover(|d| d.bg(T.hover))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings.auto_save = !this.settings.auto_save;
                    cx.notify();
                })),
            )
            .child(option("ai-chat", "Send to AI opens", "Copies the capture and its text, then opens the chat.", self.chats(cx)));
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(header("A few preferences", "Run setup again from Settings to change them."))
            .child(rows)
    }

    fn chats(&self, cx: &mut Context<Self>) -> Div {
        div().flex_none().flex().gap(px(2.)).p(px(2.)).rounded(px(6.)).bg(T.chrome).children(AiChat::ALL.map(|chat| {
            let on = self.settings.ai_chat == chat;
            div()
                .id(chat.label())
                .h(px(24.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_size(px(12.))
                .map(|d| if on { d.bg(T.hover).text_color(T.text) } else { d.text_color(T.text3).hover(|d| d.text_color(T.text)) })
                .child(chat.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.ai_chat = chat;
                    cx.notify();
                }))
        }))
    }

    fn ready_step(&self, cx: &mut Context<Self>) -> Div {
        let Some(key) = self.active(CAPTURE) else {
            let why = match self.selected().and_then(|s| self.problems.get(&(CAPTURE, s.clone())).map(|p| (s, p.message()))) {
                Some((s, p)) => format!("{s}: {p}. Go back and pick another, or capture from the tray icon."),
                None => "Go back and pick one, or capture from the tray icon.".into(),
            };
            return div().flex().flex_col().gap(px(16.)).child(header("No capture shortcut works yet", why));
        };
        let others: Vec<_> = ACTIONS.iter().filter(|a| a.id != CAPTURE).filter_map(|a| self.active(a.id).map(|k| (a.label, k))).collect();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(28.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .pt(px(20.))
                    .child(
                        div()
                            .size(px(40.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(T.accent_soft)
                            .child(icon(Icon::Check).size(px(20.)).text_color(T.accent)),
                    )
                    .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child("You're set"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_color(T.text2)
                            .child("Press")
                            .child(kbd(key))
                            .child("anywhere to capture."),
                    )
                    .child(
                        button("try", "Try a capture", false)
                            .mt(px(4.))
                            .on_click(cx.listener(|this, _, window, cx| this.try_capture(window, cx))),
                    ),
            )
            .when(!others.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().pb(px(4.)).text_size(px(12.)).text_color(T.text3).child("Also"))
                        .children(others.into_iter().map(|(label, k)| {
                            div().flex().items_center().h(px(22.)).child(div().flex_1().text_color(T.text2).child(label)).child(kbd(k))
                        })),
                )
            })
    }
}

impl Focusable for Onboarding {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Onboarding {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (primary, back) = match self.step {
            Step::Welcome => ("Get started", None),
            Step::Shortcut => ("Continue", Some(Step::Welcome)),
            Step::Options => ("Continue", Some(Step::Shortcut)),
            Step::Ready => ("Finish", Some(Step::Options)),
        };
        let body = match self.step {
            Step::Welcome => self.welcome(),
            Step::Shortcut => self.shortcut_step(cx),
            Step::Options => self.options_step(cx),
            Step::Ready => self.ready_step(cx),
        };
        div()
            .id("onboarding")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(20.))
            .p(px(32.))
            .bg(T.chrome)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .text_color(T.text)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev, window, cx| this.key_down(ev, window, cx)))
            .child(div().flex().gap(px(6.)).children(STEPS.map(|s| {
                div().size(px(6.)).rounded_full().bg(if s == self.step { T.accent } else { T.line2 })
            })))
            .child(div().flex_1().flex().flex_col().child(body))
            .child(
                div()
                    .flex()
                    .items_center()
                    .children(back.map(|step| button("back", "Back", false).on_click(cx.listener(move |this, _, _, cx| this.go(step, cx)))))
                    .child(div().flex_1())
                    .child(button("primary", primary, true).on_click(cx.listener(|this, _, window, cx| this.advance(window, cx)))),
            )
    }
}

fn header(title: &'static str, about: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(title))
        .child(div().text_size(px(12.)).text_color(T.text3).child(about.into()))
}

/// One row of the preferences list, with its control on the right.
fn option(id: &'static str, title: &'static str, about: &'static str, control: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(14.))
        .py(px(12.))
        .rounded(px(8.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(title)
                .child(div().text_size(px(12.)).text_color(T.text3).child(about)),
        )
        .child(control)
}

fn card(id: &'static str, title: String, about: &'static str, on: bool, status: Option<Status>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_w(px(0.))
        .min_h(px(108.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(14.))
        .rounded(px(8.))
        .border_1()
        .bg(T.control)
        .border_color(if on { T.accent } else { T.line })
        .when(!on, |d| d.hover(|d| d.border_color(T.line2)))
        .child(div().font_family(MONO_FONT).text_size(px(15.)).child(title))
        .child(div().text_size(px(12.)).text_color(T.text3).child(about))
        .child(div().flex_1())
        .children(status.map(|s| match s {
            Status::Ready => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .child(icon(Icon::Check).size(px(14.)).text_color(T.accent))
                .child("Ready"),
            Status::Available => div().text_size(px(12.)).text_color(T.text3).child("Available"),
            Status::Blocked(p) => div().text_size(px(12.)).text_color(T.warn).child(p.message()),
        }))
}

/// Primary is the one action that moves the step forward.
fn button(id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(32.))
        .px(px(16.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .map(|d| {
            if primary {
                d.bg(T.text).text_color(T.chrome).font_weight(FontWeight::SEMIBOLD).hover(|d| d.opacity(0.9))
            } else {
                d.bg(T.control).text_color(T.text).hover(|d| d.bg(T.hover))
            }
        })
        .child(label)
}

fn switch(on: bool) -> Div {
    div()
        .flex_none()
        .w(px(32.))
        .h(px(18.))
        .p(px(2.))
        .rounded_full()
        .flex()
        .when(on, |d| d.justify_end())
        .bg(if on { T.accent } else { T.line2 })
        .child(div().size(px(14.)).rounded_full().bg(if on { T.accent_ink } else { T.text2 }))
}
