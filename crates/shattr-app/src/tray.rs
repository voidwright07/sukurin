//! Tray icon, its menu, and the global hotkeys. Both deliver `Command`s over
//! one channel that the foreground loop drains.

use crate::app::Command;
use crate::overlay::Mode;
use crate::settings::Settings;
use crate::shortcuts::{self, ACTIONS};
use anyhow::Result;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui_kit::Global;
use smol::channel::Sender;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Lives as long as the app; dropping it removes the icon and unregisters
/// the hotkeys.
pub struct Resident {
    _tray: TrayIcon,
    manager: GlobalHotKeyManager,
    /// Hotkey id to command, read by the OS hotkey callback.
    routes: Arc<Mutex<HashMap<u32, Command>>>,
    registered: Vec<HotKey>,
    /// Menu items whose label carries the action's first shortcut.
    labelled: Vec<(MenuItem, &'static str, &'static str)>,
}

impl Global for Resident {}

/// A shortcut Windows refused, usually because another app owns it.
pub struct Refused {
    pub action: &'static str,
    pub shortcut: String,
    pub reason: String,
}

impl Resident {
    /// Registers exactly the shortcuts in `settings`, replacing whatever was
    /// registered before, and refreshes the menu labels.
    pub fn apply(&mut self, settings: &Settings) -> Vec<Refused> {
        let _ = self.manager.unregister_all(&self.registered);
        self.registered.clear();
        let mut routes = HashMap::new();
        let mut refused = Vec::new();
        let mut first: HashMap<&'static str, String> = HashMap::new();
        for a in ACTIONS {
            for text in settings.shortcuts(a.id) {
                let parsed = match shortcuts::parse(&text) {
                    Ok(p) => p,
                    Err(reason) => {
                        refused.push(Refused { action: a.id, shortcut: text, reason });
                        continue;
                    }
                };
                match self.manager.register(parsed.hotkey) {
                    Ok(()) => {
                        routes.insert(parsed.hotkey.id(), a.command);
                        self.registered.push(parsed.hotkey);
                        first.entry(a.id).or_insert(parsed.text);
                    }
                    Err(e) => {
                        log::warn!("hotkey {} unavailable: {e}", parsed.text);
                        refused.push(Refused { action: a.id, shortcut: parsed.text, reason: "Another app already uses it".into() });
                    }
                }
            }
        }
        *self.routes.lock().unwrap() = routes;
        for (item, id, label) in &self.labelled {
            item.set_text(match first.get(id) {
                Some(k) => format!("{label}\t{k}"),
                None => label.to_string(),
            });
        }
        refused
    }
}

pub fn install(tx: Sender<Command>, settings: &Settings) -> Result<(Resident, Vec<Refused>)> {
    let manager = GlobalHotKeyManager::new()?;
    let routes: Arc<Mutex<HashMap<u32, Command>>> = Arc::default();
    let hk_routes = routes.clone();
    let hk_tx = tx.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
        if e.state == HotKeyState::Pressed
            && let Some(c) = hk_routes.lock().unwrap().get(&e.id)
        {
            let _ = hk_tx.try_send(*c);
        }
    }));

    let labelled: Vec<(MenuItem, &'static str, &'static str)> =
        ACTIONS.iter().map(|a| (MenuItem::new(a.label, true, None), a.id, a.label)).collect();
    let plain = [
        (MenuItem::new("Settings…", true, None), Command::OpenSettings),
        (MenuItem::new("Open captures folder", true, None), Command::OpenFolder),
        (MenuItem::new("Quit Shattr", true, None), Command::Quit),
    ];
    let autostart = CheckMenuItem::new("Start with Windows", cfg!(windows), crate::autostart::is_enabled(), None);
    let menu = Menu::new();
    for (item, _, _) in &labelled {
        menu.append(item)?;
    }
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&plain[0].0)?;
    menu.append(&plain[1].0)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&autostart)?;
    menu.append(&plain[2].0)?;

    let mut by_menu: HashMap<_, _> = labelled
        .iter()
        .map(|(item, id, _)| (item.id().clone(), shortcuts::action(id).unwrap().command))
        .collect();
    by_menu.extend(plain.iter().map(|(item, c)| (item.id().clone(), *c)));
    by_menu.insert(autostart.id().clone(), Command::ToggleAutostart);
    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if let Some(c) = by_menu.get(&e.id) {
            let _ = menu_tx.try_send(*c);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            let _ = tx.try_send(Command::Capture(Mode::Area));
        }
    }));

    let size = 32;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Shattr")
        .with_icon(Icon::from_rgba(logo(size, tray_ink()), size, size)?)
        .build()?;
    let mut resident = Resident { _tray: tray, manager, routes, registered: Vec::new(), labelled };
    let refused = resident.apply(settings);
    Ok((resident, refused))
}


/// Light strokes for a dark taskbar, dark strokes for a light one.
fn tray_ink() -> [u8; 3] {
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
        use windows::core::w;
        let mut v = 0u32;
        let mut n = 4u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("SystemUsesLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut v as *mut u32 as *mut _),
                Some(&mut n),
            )
        };
        if ok.is_ok() && v == 1 {
            return [0x1C, 0x1D, 0x21];
        }
    }
    [0xF2, 0xF3, 0xF5]
}

/// The Shattr mark (corner brackets cut by a diagonal) rasterised at `size`.
fn logo(size: u32, ink: [u8; 3]) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = size as f32 / 20.;
    let stroke = (1.8 * s).max(2.0);
    let mut plot = |x: f32, y: f32| {
        let r = stroke / 2.;
        for yy in (y - r).floor() as i32..=(y + r).ceil() as i32 {
            for xx in (x - r).floor() as i32..=(x + r).ceil() as i32 {
                if xx < 0 || yy < 0 || xx >= size as i32 || yy >= size as i32 {
                    continue;
                }
                let d = ((xx as f32 + 0.5 - x).powi(2) + (yy as f32 + 0.5 - y).powi(2)).sqrt();
                let a = (r + 0.5 - d).clamp(0., 1.);
                let i = ((yy as u32 * size + xx as u32) * 4) as usize;
                let cur = px[i + 3] as f32 / 255.;
                if a > cur {
                    px[i..i + 3].copy_from_slice(&ink);
                    px[i + 3] = (a * 255.) as u8;
                }
            }
        }
    };
    let mut line = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let steps = (((x1 - x0).abs().max((y1 - y0).abs())) * s * 3.) as i32 + 1;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            plot((x0 + (x1 - x0) * t) * s, (y0 + (y1 - y0) * t) * s);
        }
    };
    for (a, b, c) in [((2., 7.), (2., 2.), (7., 2.)), ((13., 2.), (18., 2.), (18., 7.)), ((18., 13.), (18., 18.), (13., 18.)), ((7., 18.), (2., 18.), (2., 13.))] {
        line(a.0, a.1, b.0, b.1);
        line(b.0, b.1, c.0, c.1);
    }
    line(6.5, 13.5, 13.5, 6.5);
    px
}
