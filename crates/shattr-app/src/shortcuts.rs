//! Global shortcuts as the user sees and stores them ("Ctrl+Shift+S"), and
//! the table of actions that can be bound.

use crate::app::Command;
use crate::overlay::Mode;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};

/// An action that can have global shortcuts, and its defaults.
pub struct ActionSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub command: Command,
    pub defaults: &'static [&'static str],
}

/// Shortcut slots per action: a main one and an alternate.
pub const SLOTS: usize = 2;

pub const ACTIONS: &[ActionSpec] = &[
    ActionSpec { id: "capture-area", label: "Capture area", command: Command::Capture(Mode::Area), defaults: &["Ctrl+Shift+S", "PrtSc"] },
    ActionSpec { id: "capture-window", label: "Capture window", command: Command::Capture(Mode::Window), defaults: &["Alt+PrtSc"] },
    ActionSpec { id: "capture-screen", label: "Capture screen", command: Command::Capture(Mode::Screen), defaults: &["Ctrl+PrtSc"] },
    ActionSpec { id: "repeat-last-area", label: "Repeat last area", command: Command::Capture(Mode::Repeat), defaults: &["Ctrl+Shift+PrtSc"] },
    ActionSpec { id: "copy-text", label: "Copy text from screen", command: Command::Capture(Mode::Text), defaults: &["Alt+Shift+PrtSc"] },
    ActionSpec { id: "library", label: "Search screenshots", command: Command::OpenLibrary, defaults: &["Ctrl+Shift+K"] },
];

pub fn action(id: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// A parsed shortcut. `text` is the canonical spelling shown and saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub hotkey: HotKey,
    pub text: String,
}

/// Keys allowed on their own: nothing types them, so binding them globally
/// can't swallow normal input.
fn standalone_ok(code: Code) -> bool {
    use Code::*;
    matches!(
        code,
        PrintScreen | Pause | ScrollLock | Insert | F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12 | F13 | F14 | F15
            | F16 | F17 | F18 | F19 | F20 | F21 | F22 | F23 | F24
    )
}

/// Key tokens and their codes. The first spelling of each code is canonical.
const KEYS: &[(&str, Code)] = &[
    ("PrtSc", Code::PrintScreen),
    ("PrintScreen", Code::PrintScreen),
    ("Pause", Code::Pause),
    ("ScrollLock", Code::ScrollLock),
    ("Insert", Code::Insert),
    ("Delete", Code::Delete),
    ("Home", Code::Home),
    ("End", Code::End),
    ("PageUp", Code::PageUp),
    ("PageDown", Code::PageDown),
    ("Up", Code::ArrowUp),
    ("Down", Code::ArrowDown),
    ("Left", Code::ArrowLeft),
    ("Right", Code::ArrowRight),
    ("Space", Code::Space),
    ("Enter", Code::Enter),
    ("Backspace", Code::Backspace),
    ("Tab", Code::Tab),
    ("`", Code::Backquote),
    ("-", Code::Minus),
    ("=", Code::Equal),
    ("[", Code::BracketLeft),
    ("]", Code::BracketRight),
    ("\\", Code::Backslash),
    (";", Code::Semicolon),
    ("'", Code::Quote),
    (",", Code::Comma),
    (".", Code::Period),
    ("/", Code::Slash),
];

const LETTERS: [Code; 26] = {
    use Code::*;
    [KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ]
};
const DIGITS: [Code; 10] = {
    use Code::*;
    [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9]
};
const FUNCTION: [Code; 24] = {
    use Code::*;
    [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24]
};

fn key_code(token: &str) -> Option<Code> {
    let upper = token.to_ascii_uppercase();
    if upper.len() == 1 {
        let c = upper.as_bytes()[0];
        if c.is_ascii_uppercase() {
            return Some(LETTERS[(c - b'A') as usize]);
        }
        if c.is_ascii_digit() {
            return Some(DIGITS[(c - b'0') as usize]);
        }
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<usize>().ok())
        && (1..=24).contains(&n)
    {
        return Some(FUNCTION[n - 1]);
    }
    KEYS.iter().find(|(name, _)| name.eq_ignore_ascii_case(token)).map(|(_, c)| *c)
}

fn key_name(code: Code) -> String {
    if let Some(i) = LETTERS.iter().position(|c| *c == code) {
        return ((b'A' + i as u8) as char).to_string();
    }
    if let Some(i) = DIGITS.iter().position(|c| *c == code) {
        return i.to_string();
    }
    if let Some(i) = FUNCTION.iter().position(|c| *c == code) {
        return format!("F{}", i + 1);
    }
    KEYS.iter().find(|(_, c)| *c == code).map(|(n, _)| n.to_string()).unwrap_or_default()
}

/// Parses "Ctrl+Shift+S"-style text, in any case and modifier order.
pub fn parse(text: &str) -> Result<Shortcut, String> {
    let mut mods = Modifiers::empty();
    let mut code = None;
    let tokens: Vec<&str> = text.split('+').map(str::trim).filter(|t| !t.is_empty()).collect();
    // A trailing "+" means the plus key itself, which isn't supported.
    if tokens.is_empty() {
        return Err("empty shortcut".into());
    }
    for (i, t) in tokens.iter().enumerate() {
        let m = match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(Modifiers::CONTROL),
            "shift" => Some(Modifiers::SHIFT),
            "alt" | "option" => Some(Modifiers::ALT),
            "win" | "super" | "cmd" | "meta" => Some(Modifiers::SUPER),
            _ => None,
        };
        match m {
            Some(m) if i + 1 < tokens.len() => mods |= m,
            Some(_) => return Err(format!("“{text}” needs a key after the modifiers")),
            None if i + 1 == tokens.len() => code = key_code(t),
            None => return Err(format!("“{t}” isn't a modifier")),
        }
    }
    let code = code.ok_or_else(|| format!("“{}” isn't a key Shattr can bind", tokens.last().unwrap()))?;
    if mods.is_empty() && !standalone_ok(code) {
        return Err("add Ctrl, Alt, Shift or Win so normal typing isn't taken over".into());
    }
    Ok(Shortcut { hotkey: HotKey::new((!mods.is_empty()).then_some(mods), code), text: format(mods, code) })
}

fn format(mods: Modifiers, code: Code) -> String {
    let mut parts = Vec::new();
    for (m, name) in [(Modifiers::CONTROL, "Ctrl"), (Modifiers::ALT, "Alt"), (Modifiers::SHIFT, "Shift"), (Modifiers::SUPER, "Win")] {
        if mods.contains(m) {
            parts.push(name.to_string());
        }
    }
    parts.push(key_name(code));
    parts.join("+")
}

/// What a key press means while a shortcut is being recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordKey {
    Cancel,
    Clear,
    Record(String),
    /// Only modifiers are held so far.
    Ignore,
}

/// Turns a key press seen in a GPUI window into a recording decision.
pub fn record_key(key: &str, ctrl: bool, alt: bool, shift: bool, win: bool) -> RecordKey {
    let plain = !(ctrl || alt || shift || win);
    let token = match key {
        "escape" if plain => return RecordKey::Cancel,
        "backspace" | "delete" if plain => return RecordKey::Clear,
        "control" | "shift" | "alt" | "platform" | "function" | "" => return RecordKey::Ignore,
        "pageup" => "PageUp".to_string(),
        "pagedown" => "PageDown".to_string(),
        k if k.len() > 1 => {
            let mut c = k.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        }
        k => k.to_ascii_uppercase(),
    };
    let mut parts = Vec::new();
    for (on, name) in [(ctrl, "Ctrl"), (alt, "Alt"), (shift, "Shift"), (win, "Win")] {
        if on {
            parts.push(name.to_string());
        }
    }
    parts.push(token);
    RecordKey::Record(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_canonically() {
        let s = parse("shift+ctrl+s").unwrap();
        assert_eq!(s.text, "Ctrl+Shift+S");
        assert_eq!(s.hotkey, HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS));
        assert_eq!(parse("alt+shift+prtsc").unwrap().text, "Alt+Shift+PrtSc");
        assert_eq!(parse("Ctrl+F12").unwrap().hotkey, HotKey::new(Some(Modifiers::CONTROL), Code::F12));
        assert_eq!(parse("Win+`").unwrap().text, "Win+`");
        assert_eq!(parse("Ctrl+Alt+2").unwrap().hotkey, HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Digit2));
    }

    #[test]
    fn bare_typing_keys_are_refused_but_safe_keys_are_allowed() {
        assert!(parse("S").is_err());
        assert!(parse("Space").is_err());
        assert!(parse("PrtSc").is_ok());
        assert!(parse("F9").is_ok());
    }

    #[test]
    fn malformed_text_is_explained() {
        assert!(parse("").is_err());
        assert!(parse("Ctrl+Shift").is_err());
        assert!(parse("Ctrl+Hyper+S").is_err());
        assert!(parse("Ctrl+NoSuchKey").is_err());
    }

    #[test]
    fn every_default_parses_and_no_two_actions_share_one() {
        let mut seen = std::collections::HashSet::new();
        for a in ACTIONS {
            assert!(a.defaults.len() <= SLOTS, "{} has too many defaults", a.id);
            for d in a.defaults {
                let s = parse(d).unwrap_or_else(|e| panic!("{d}: {e}"));
                assert_eq!(&s.text, d, "defaults are written canonically");
                assert!(seen.insert(s.hotkey), "{d} is bound twice");
            }
        }
    }

    #[test]
    fn key_presses_become_recording_decisions() {
        let rec = |t: &str| RecordKey::Record(t.to_string());
        assert_eq!(record_key("s", true, false, true, false), rec("Ctrl+Shift+S"));
        assert_eq!(record_key("f5", false, true, false, false), rec("Alt+F5"));
        assert_eq!(record_key("pageup", true, false, false, false), rec("Ctrl+PageUp"));
        assert_eq!(record_key("shift", false, false, true, false), RecordKey::Ignore);
        assert_eq!(record_key("escape", false, false, false, false), RecordKey::Cancel);
        assert_eq!(record_key("backspace", false, false, false, false), RecordKey::Clear);
        assert_eq!(record_key("delete", false, false, false, false), RecordKey::Clear);
        assert_eq!(record_key("delete", true, true, false, false), rec("Ctrl+Alt+Delete"));
        for k in ["Ctrl+Shift+S", "Alt+F5", "Ctrl+PageUp", "Ctrl+Alt+Delete"] {
            assert!(parse(k).is_ok(), "{k}");
        }
    }
}
