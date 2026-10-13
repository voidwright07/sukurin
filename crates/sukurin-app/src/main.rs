#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod capture;
mod countdown;
#[cfg(target_os = "linux")]
mod identity;
mod instance;
mod overlay;
mod library_window;
mod onboarding;
mod pin;
mod recognize;
#[cfg(target_os = "linux")]
mod screenshot_grant;
mod scroll;
mod settings;
mod settings_window;
mod share;
mod shortcuts;
mod system_keys;
mod thumbnail;
mod toast;
mod tray;

use gpui_kit::*;

#[cfg(not(target_os = "linux"))]
struct Idle;

#[cfg(not(target_os = "linux"))]
impl Render for Idle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let listener = match instance::claim() {
        instance::Claim::First(listener) => listener,
        instance::Claim::Forwarded => {
            log::info!("Sukurin is already running");
            return;
        }
    };
    // Before anything talks to the desktop portal, which fixes a connection's
    // app identity at first contact.
    #[cfg(target_os = "linux")]
    match identity::adopt() {
        Ok(scope) => log::info!("running as {scope}"),
        Err(e) => log::warn!("could not take Sukurin's own app identity: {e:#}; GNOME will attribute screenshots to the launcher"),
    }
    let application = gpui_kit::application().with_assets(sukurin_editor::EditorAssets);
    // Linux maps every window, hidden or not, so a keep-alive window would sit
    // in the dock. Quitting only on request keeps Sukurin alive with none.
    #[cfg(target_os = "linux")]
    let application = application.with_quit_mode(QuitMode::Explicit);
    application.run(move |cx| {
        gpui_kit::init(cx);
        if let Err(e) = start(cx, listener) {
            log::error!("{e:#}");
            cx.quit();
        }
    });
}

fn start(cx: &mut App, listener: instance::Listener) -> anyhow::Result<()> {
    gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    app::install(cx)?;
    // GPUI quits when its last window closes; Sukurin lives in the tray, so
    // one invisible window keeps the app alive between captures.
    #[cfg(not(target_os = "linux"))]
    open_window(
        WindowOptions {
            app_id: Some(sukurin_ui::APP_ID.into()),
            show: false,
            focus: false,
            kind: WindowKind::PopUp,
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(1.), px(1.))))),
            ..Default::default()
        },
        cx,
        |window, cx| {
            consume_startup_show_state(window);
            cx.new(|_| Idle)
        },
    )?;
    let (tx, rx) = smol::channel::unbounded();
    let settings = settings::Settings::load();
    let mut requests = instance::requests(std::env::args().skip(1));
    if !settings.onboarded && !requests.iter().any(|c| matches!(c, app::Command::OpenOnboarding)) {
        requests.push(app::Command::OpenOnboarding);
    }
    for cmd in requests {
        let _ = tx.try_send(cmd);
    }
    instance::serve(listener, tx.clone());
    let (resident, refused) = tray::install(tx, &settings)?;
    for r in refused {
        log::warn!("{} shortcut {} not active: {}", r.action, r.shortcut, r.problem.message());
    }
    cx.set_global(resident);
    cx.spawn(async move |cx| {
        while let Ok(cmd) = rx.recv().await {
            cx.update(|cx| app::run(cmd, cx));
        }
    })
    .detach();
    log::info!("Sukurin ready");
    Ok(())
}

/// Windows applies the launcher's show state (Hidden, Minimized) to the first
/// window a process shows. Spend it on the invisible keep-alive window so the
/// first capture overlay isn't the one that gets hidden.
#[cfg(windows)]
fn consume_startup_show_state(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
    if let Ok(h) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::Win32(h) = h.as_raw()
    {
        unsafe {
            let _ = ShowWindow(HWND(h.hwnd.get() as *mut _), SW_HIDE);
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn consume_startup_show_state(_: &Window) {}
