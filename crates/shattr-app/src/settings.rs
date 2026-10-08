//! User settings, stored as JSON in the platform config folder.

use crate::shortcuts::{self, ACTIONS};
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Action id to its shortcuts. An action that is absent uses its
    /// defaults; an empty list means the user removed every shortcut.
    #[serde(default)]
    pub hotkeys: BTreeMap<String, Vec<String>>,
    /// The first-run setup was finished or dismissed.
    #[serde(default)]
    pub onboarded: bool,
}

impl Settings {
    /// Every action with no shortcut. Applied while a shortcut is being
    /// recorded, or pressing an existing one would trigger it instead.
    pub fn unbound() -> Self {
        Self { hotkeys: ACTIONS.iter().map(|a| (a.id.to_string(), vec![])).collect(), ..Default::default() }
    }

    pub fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("Shattr").join("settings.json"))
    }

    /// Missing or unreadable settings fall back to defaults; Shattr must
    /// start even if the file was hand-edited into nonsense.
    pub fn load() -> Self {
        let Some(path) = Self::path() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("ignoring unreadable {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path().context("no config folder")?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// The shortcuts in effect for `id`, one entry per slot.
    pub fn shortcuts(&self, id: &str) -> Vec<String> {
        match self.hotkeys.get(id) {
            Some(list) => list.clone(),
            None => shortcuts::action(id).map(|a| a.defaults.iter().map(|s| s.to_string()).collect()).unwrap_or_default(),
        }
    }

    /// Puts `text` in `slot` of `id` (or clears it with None). Fails if the
    /// shortcut is malformed or already used by another action.
    pub fn set_shortcut(&mut self, id: &str, slot: usize, text: Option<&str>) -> Result<(), String> {
        let canonical = match text {
            Some(t) => Some(self.free(t, |a, i| a == id && i == slot)?),
            None => None,
        };
        let mut list = self.shortcuts(id);
        list.resize(shortcuts::SLOTS.max(list.len()), String::new());
        list[slot] = canonical.unwrap_or_default();
        list.retain(|s| !s.is_empty());
        self.hotkeys.insert(id.to_string(), list);
        Ok(())
    }

    /// Makes `text` the only shortcut of `id`, with the same checks as
    /// `set_shortcut`.
    pub fn set_sole_shortcut(&mut self, id: &str, text: &str) -> Result<(), String> {
        let canonical = self.free(text, |a, _| a == id)?;
        self.hotkeys.insert(id.to_string(), vec![canonical]);
        Ok(())
    }

    /// The canonical spelling of `text` if it parses and no slot outside
    /// `replacing` already uses it.
    fn free(&self, text: &str, replacing: impl Fn(&str, usize) -> bool) -> Result<String, String> {
        let parsed = shortcuts::parse(text)?;
        for a in ACTIONS {
            for (i, other) in self.shortcuts(a.id).iter().enumerate() {
                if !replacing(a.id, i) && shortcuts::parse(other).is_ok_and(|o| o.hotkey == parsed.hotkey) {
                    return Err(format!("{} is already used by {}", parsed.text, a.label));
                }
            }
        }
        Ok(parsed.text)
    }

    pub fn reset(&mut self, id: &str) {
        self.hotkeys.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_actions_use_defaults_and_cleared_ones_stay_empty() {
        let mut s = Settings::default();
        assert_eq!(s.shortcuts("capture-area"), vec!["Ctrl+Shift+S", "PrtSc"]);
        s.set_shortcut("capture-window", 0, None).unwrap();
        assert!(s.shortcuts("capture-window").is_empty());
        s.reset("capture-window");
        assert_eq!(s.shortcuts("capture-window"), vec!["Alt+PrtSc"]);
    }

    #[test]
    fn setting_canonicalises_and_refuses_duplicates() {
        let mut s = Settings::default();
        s.set_shortcut("library", 0, Some("shift+alt+l")).unwrap();
        assert_eq!(s.shortcuts("library"), vec!["Alt+Shift+L"]);
        let err = s.set_shortcut("library", 1, Some("ctrl+shift+s")).unwrap_err();
        assert!(err.contains("Capture area"), "{err}");
        s.set_shortcut("capture-area", 0, Some("Ctrl+Shift+S")).unwrap();
    }

    #[test]
    fn round_trips_through_json() {
        let mut s = Settings::default();
        s.set_shortcut("capture-area", 1, Some("F9")).unwrap();
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.shortcuts("capture-area"), vec!["Ctrl+Shift+S", "F9"]);
    }

    #[test]
    fn onboarded_defaults_to_false_for_older_files() {
        let s: Settings = serde_json::from_str(r#"{"hotkeys":{"library":["F9"]}}"#).unwrap();
        assert!(!s.onboarded);
        assert_eq!(s.shortcuts("library"), vec!["F9"]);
        let done = Settings { onboarded: true, ..Default::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&done).unwrap()).unwrap();
        assert!(back.onboarded);
    }

    #[test]
    fn sole_shortcut_replaces_every_slot() {
        let mut s = Settings::default();
        s.set_sole_shortcut("capture-area", "prtsc").unwrap();
        assert_eq!(s.shortcuts("capture-area"), vec!["PrtSc"]);
        s.set_sole_shortcut("capture-area", "shift+ctrl+s").unwrap();
        assert_eq!(s.shortcuts("capture-area"), vec!["Ctrl+Shift+S"]);
    }

    #[test]
    fn sole_shortcut_refuses_another_actions_key_and_leaves_settings_alone() {
        let mut s = Settings::default();
        let before = s.clone();
        let err = s.set_sole_shortcut("capture-area", "Alt+PrtSc").unwrap_err();
        assert!(err.contains("Capture window"), "{err}");
        assert_eq!(s, before);
        assert!(s.set_sole_shortcut("capture-area", "Ctrl+Nope").is_err());
        assert_eq!(s, before);
    }
}
