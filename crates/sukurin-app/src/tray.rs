//! Tray icon, its menu, and the global hotkeys. Both deliver `Command`s over
//! one channel that the foreground loop drains.

use crate::app::Command;
use crate::overlay::Mode;
use crate::settings::Settings;
use crate::shortcuts::{self, ACTIONS, Shortcut};
use anyhow::Result;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
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

/// A shortcut that isn't active, and why.
pub struct Refused {
    pub action: &'static str,
    pub shortcut: String,
    pub problem: Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The text doesn't parse; holds the parse error.
    Invalid(String),
    /// Bare PrtSc, held by Windows for Snipping Tool.
    Snipping,
    /// A Win combination, which Windows keeps for itself.
    Reserved,
    Taken,
}

impl Problem {
    pub fn message(&self) -> String {
        match self {
            Problem::Invalid(e) => e.clone(),
            Problem::Snipping => "Windows opens Snipping Tool with PrtSc".into(),
            Problem::Reserved => "Windows reserves this shortcut".into(),
            Problem::Taken => "Another app already uses it".into(),
        }
    }
}

fn is_bare_prtsc(hotkey: &HotKey) -> bool {
    hotkey.key == Code::PrintScreen && hotkey.mods.is_empty()
}

/// Why the OS refused `hotkey`. The OS only says no, so this is inferred
/// from the key itself.
fn classify(hotkey: &HotKey) -> Problem {
    if hotkey.mods.contains(Modifiers::SUPER) {
        Problem::Reserved
    } else {
        Problem::Taken
    }
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
                    Err(e) => {
                        refused.push(Refused { action: a.id, shortcut: text, problem: Problem::Invalid(e) });
                        continue;
                    }
                };
                match self.register(&parsed) {
                    Ok(()) => {
                        routes.insert(parsed.hotkey.id(), a.command);
                        self.registered.push(parsed.hotkey);
                        first.entry(a.id).or_insert(parsed.text);
                    }
                    Err(problem) => {
                        log::warn!("hotkey {} unavailable: {problem:?}", parsed.text);
                        refused.push(Refused { action: a.id, shortcut: parsed.text, problem });
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

    /// Whether `shortcut` could be registered right now, without keeping it.
    pub fn probe(&mut self, shortcut: &Shortcut) -> Result<(), Problem> {
        if self.registered.contains(&shortcut.hotkey) {
            return Ok(());
        }
        self.register(shortcut)?;
        let _ = self.manager.unregister(shortcut.hotkey);
        Ok(())
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), Problem> {
        // The OS would accept it, but the shell takes the key first.
        if is_bare_prtsc(&shortcut.hotkey) && crate::system_keys::snipping_owns_prtsc() {
            return Err(Problem::Snipping);
        }
        self.manager.register(shortcut.hotkey).map_err(|_| classify(&shortcut.hotkey))
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
        (MenuItem::new("Quit Sukurin", true, None), Command::Quit),
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
        .with_tooltip("Sukurin")
        .with_icon(Icon::from_rgba(logo(size, tray_inks()), size, size)?)
        .build()?;
    let mut resident = Resident { _tray: tray, manager, routes, registered: Vec::new(), labelled };
    let refused = resident.apply(settings);
    Ok((resident, refused))
}


/// Body and shard colours: light body on a dark taskbar, dark body on a light
/// one, with the theme accent that reads on each.
fn tray_inks() -> ([u8; 3], [u8; 3]) {
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
            return ([0x1C, 0x1D, 0x21], [0x4A, 0x6A, 0x00]);
        }
    }
    ([0xF2, 0xF3, 0xF5], [0xC6, 0xF1, 0x35])
}

/// The Sukurin mark rasterised at `size`: a square on a 16-module grid, broken
/// along a stair of 3-module cells, with the shard moved one module down-right.
/// Each pixel gets its exact area coverage, so at multiples of 16 every edge
/// lands on a whole pixel.
fn logo(size: u32, (body, shard): ([u8; 3], [u8; 3])) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let m = size as f32 / 16.;
    let overlap = |p: u32, lo: f32, hi: f32| (hi.min(p as f32 + 1.) - lo.max(p as f32)).max(0.);
    for i in 0..4 {
        for j in 0..4 {
            let (ink, off) = if i + j >= 4 { (shard, 1.) } else { (body, 0.) };
            let (x0, y0) = ((2. + 3. * i as f32 + off) * m, (2. + 3. * j as f32 + off) * m);
            let (x1, y1) = (x0 + 3. * m, y0 + 3. * m);
            for y in y0 as u32..(y1.ceil() as u32).min(size) {
                for x in x0 as u32..(x1.ceil() as u32).min(size) {
                    let k = ((y * size + x) * 4) as usize;
                    let a = px[k + 3] as f32 + overlap(x, x0, x1) * overlap(y, y0, y1) * 255.;
                    px[k..k + 3].copy_from_slice(&ink);
                    px[k + 3] = a.round().min(255.) as u8;
                }
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_lands_on_whole_pixels_at_multiples_of_16() {
        let (body, shard) = ([1, 1, 1], [2, 2, 2]);
        let px = logo(32, (body, shard));
        assert!(px.chunks(4).all(|p| p[3] == 0 || p[3] == 255));
        let at = |x: usize, y: usize| px[(y * 32 + x) * 4];
        assert_eq!(at(4, 4), 1);
        assert_eq!(at(29, 29), 2);
        assert_eq!(px[(22 * 32 + 11) * 4 + 3], 0, "the one-module gap is empty");
    }

    fn hotkey(text: &str) -> HotKey {
        shortcuts::parse(text).unwrap().hotkey
    }

    #[test]
    fn only_unmodified_prtsc_is_held_by_snipping_tool() {
        assert!(is_bare_prtsc(&hotkey("PrtSc")));
        assert!(!is_bare_prtsc(&hotkey("Alt+PrtSc")));
        assert!(!is_bare_prtsc(&hotkey("Win+PrtSc")));
    }

    #[test]
    fn win_combinations_are_reserved_and_the_rest_taken() {
        assert_eq!(classify(&hotkey("Win+Shift+S")), Problem::Reserved);
        assert_eq!(classify(&hotkey("Win+PrtSc")), Problem::Reserved);
        assert_eq!(classify(&hotkey("PrtSc")), Problem::Taken);
        assert_eq!(classify(&hotkey("Ctrl+Shift+S")), Problem::Taken);
    }
}
