#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod capture;
mod countdown;
mod overlay;
mod library_window;
mod onboarding;
mod pin;
mod recognize;
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

struct Idle;

impl Render for Idle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if !single_instance() {
        log::info!("Sukurin is already running");
        return;
    }
    gpui_kit::application().with_assets(sukurin_editor::EditorAssets).run(|cx| {
        gpui_kit::init(cx);
        if let Err(e) = start(cx) {
            log::error!("{e:#}");
            cx.quit();
        }
    });
}

fn start(cx: &mut App) -> anyhow::Result<()> {
    gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    app::install(cx)?;
    // GPUI quits when its last window closes; Sukurin lives in the tray, so
    // one invisible window keeps the app alive between captures.
    open_window(
        WindowOptions {
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
    if std::env::args().any(|a| a == "--library") {
        let _ = tx.try_send(app::Command::OpenLibrary);
    }
    let settings = settings::Settings::load();
    if !settings.onboarded || std::env::args().any(|a| a == "--onboarding") {
        let _ = tx.try_send(app::Command::OpenOnboarding);
    }
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

#[cfg(not(windows))]
fn consume_startup_show_state(_: &Window) {}

#[cfg(windows)]
fn single_instance() -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::w;
    unsafe {
        let handle = CreateMutexW(None, true, w!("Local\\SukurinSingleInstance"));
        let first = handle.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS;
        // The mutex must outlive the process, so its handle is leaked on purpose.
        std::mem::forget(handle);
        first
    }
}

#[cfg(not(windows))]
fn single_instance() -> bool {
    true
}
