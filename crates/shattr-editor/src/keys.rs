//! Shortcuts and the command table. Key dispatch, tooltips and the command
//! palette all read [`COMMANDS`] and the tool table, so a binding is defined
//! exactly once.

use crate::icons::EIcon;
use crate::tools::{TOOLS, Tool};
use gpui_kit::Keystroke;
use shattr_annotate::ArrowKind;

/// One key plus modifiers. `secondary` is ⌘ on macOS and Ctrl elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub key: &'static str,
    pub secondary: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    pub const fn key(key: &'static str) -> Self {
        Self { key, secondary: false, shift: false, alt: false }
    }

    pub const fn shift(key: &'static str) -> Self {
        Self { key, secondary: false, shift: true, alt: false }
    }

    pub const fn secondary(key: &'static str) -> Self {
        Self { key, secondary: true, shift: false, alt: false }
    }

    pub const fn secondary_shift(key: &'static str) -> Self {
        Self { key, secondary: true, shift: true, alt: false }
    }

    pub fn matches(&self, k: &Keystroke) -> bool {
        let m = k.modifiers;
        let secondary = if cfg!(target_os = "macos") { m.platform } else { m.control };
        let other = if cfg!(target_os = "macos") { m.control } else { m.platform };
        if secondary != self.secondary || m.alt != self.alt || other {
            return false;
        }
        // Shifted punctuation arrives as its own character on most layouts.
        if !self.key.chars().all(|c| c.is_ascii_alphanumeric()) && k.key_char.as_deref() == Some(self.key) {
            return true;
        }
        m.shift == self.shift && k.key.eq_ignore_ascii_case(self.key)
    }

    pub fn label(&self) -> String {
        let key = key_label(self.key);
        if cfg!(target_os = "macos") {
            let mut s = String::new();
            if self.secondary {
                s.push('⌘');
            }
            if self.alt {
                s.push('⌥');
            }
            if self.shift {
                s.push('⇧');
            }
            s + &key
        } else if self.secondary || self.alt {
            let mut parts = Vec::new();
            if self.secondary {
                parts.push("Ctrl".to_string());
            }
            if self.alt {
                parts.push("Alt".to_string());
            }
            if self.shift {
                parts.push("Shift".to_string());
            }
            parts.push(key);
            parts.join("+")
        } else if self.shift {
            format!("⇧{key}")
        } else {
            key
        }
    }
}

fn key_label(key: &str) -> String {
    match key {
        "enter" => "↵".into(),
        "escape" => "esc".into(),
        "tab" => "Tab".into(),
        "delete" => "Del".into(),
        "backspace" => "⌫".into(),
        "-" => "−".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        k => k.to_uppercase(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Tool(Tool),
    ArrowKind(ArrowKind),
    Undo,
    Redo,
    Copy,
    Save,
    Pin,
    OpenLibrary,
    Upload,
    SendToAi,
    Palette,
    Inspector,
    Shortcuts,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    Zoom100,
    Delete,
    Duplicate,
    HandDrawn,
    EraseTextOnly,
    TrimToContent,
    ApplyCrop,
    Paste,
    CopyColour,
    CycleFormat,
    SwapContrast,
    ArrangeRow,
    ArrangeColumn,
    ArrangeGrid,
    Close,
}

pub struct Command {
    pub action: Action,
    pub name: &'static str,
    pub group: &'static str,
    /// The first entry is the one shown; the rest also trigger it.
    pub keys: &'static [Shortcut],
    pub icon: EIcon,
    /// Shown dimmed in the palette and never dispatched.
    pub soon: bool,
}

const fn cmd(action: Action, name: &'static str, group: &'static str, keys: &'static [Shortcut], icon: EIcon) -> Command {
    Command { action, name, group, keys, icon, soon: false }
}

#[cfg(windows)]
const REDO: &[Shortcut] = &[Shortcut::secondary("y"), Shortcut::secondary_shift("z")];
#[cfg(not(windows))]
const REDO: &[Shortcut] = &[Shortcut::secondary_shift("z"), Shortcut::secondary("y")];

pub const COMMANDS: &[Command] = &[
    cmd(Action::ArrowKind(ArrowKind::Straight), "Arrow · straight", "Tools", &[Shortcut::key("1")], EIcon::ArrowStraight),
    cmd(Action::ArrowKind(ArrowKind::Curved), "Arrow · curved", "Tools", &[Shortcut::key("2")], EIcon::ArrowCurved),
    cmd(Action::ArrowKind(ArrowKind::Bendable), "Arrow · bendable", "Tools", &[Shortcut::key("3")], EIcon::ArrowBendable),
    cmd(Action::HandDrawn, "Hand-drawn style", "Tools", &[Shortcut::shift("d")], EIcon::Pen),
    cmd(Action::EraseTextOnly, "Erase text only", "Tools", &[Shortcut::shift("e")], EIcon::Erase),
    cmd(Action::Undo, "Undo", "Edit", &[Shortcut::secondary("z")], EIcon::Undo),
    cmd(Action::Redo, "Redo", "Edit", REDO, EIcon::Redo),
    cmd(Action::Delete, "Delete selection", "Edit", &[Shortcut::key("delete"), Shortcut::key("backspace")], EIcon::Close),
    cmd(Action::Duplicate, "Duplicate selection", "Edit", &[Shortcut::secondary("d")], EIcon::Copy),
    cmd(Action::Paste, "Paste image as overlay", "Edit", &[Shortcut::secondary("v")], EIcon::Overlay),
    cmd(Action::TrimToContent, "Trim to content", "Canvas", &[Shortcut::shift("t")], EIcon::Crop),
    cmd(Action::ApplyCrop, "Apply crop", "Canvas", &[Shortcut::key("enter")], EIcon::Crop),
    cmd(Action::ArrangeRow, "Arrange captures · row", "Canvas", &[], EIcon::Combine),
    cmd(Action::ArrangeColumn, "Arrange captures · column", "Canvas", &[], EIcon::Combine),
    cmd(Action::ArrangeGrid, "Arrange captures · grid", "Canvas", &[], EIcon::Combine),
    cmd(Action::ZoomIn, "Zoom in", "View", &[Shortcut::secondary("="), Shortcut::secondary("+"), Shortcut::secondary_shift("=")], EIcon::Plus),
    cmd(Action::ZoomOut, "Zoom out", "View", &[Shortcut::secondary("-")], EIcon::Minus),
    cmd(Action::ZoomFit, "Fit to window", "View", &[Shortcut::secondary("0")], EIcon::Fit),
    cmd(Action::Zoom100, "Actual size", "View", &[Shortcut::secondary("1")], EIcon::Search),
    cmd(Action::Inspector, "Toggle inspector", "View", &[Shortcut::secondary("i")], EIcon::Inspector),
    cmd(Action::Palette, "Command palette", "View", &[Shortcut::secondary("k")], EIcon::Search),
    cmd(Action::Shortcuts, "Keyboard shortcuts", "View", &[Shortcut::key("?")], EIcon::Keyboard),
    cmd(Action::CopyColour, "Copy colour under cursor", "Inspect", &[Shortcut::key("tab")], EIcon::Picker),
    cmd(Action::CycleFormat, "Cycle colour format", "Inspect", &[Shortcut::shift("tab")], EIcon::Picker),
    cmd(Action::SwapContrast, "Swap contrast colours", "Inspect", &[Shortcut::key("x")], EIcon::Swap),
    cmd(Action::Copy, "Copy image", "Export", &[Shortcut::secondary("c")], EIcon::Copy),
    cmd(Action::Save, "Save", "Export", &[Shortcut::secondary("s")], EIcon::Save),
    cmd(Action::Pin, "Pin to screen", "Export", &[Shortcut::secondary("p")], EIcon::Pin),
    cmd(Action::OpenLibrary, "Search screenshots (Library)", "Shattr", &[Shortcut::secondary_shift("k")], EIcon::Search),
    Command { soon: true, ..cmd(Action::Upload, "Upload and copy link", "Export", &[Shortcut::secondary("u")], EIcon::Upload) },
    Command { soon: true, ..cmd(Action::SendToAi, "Send to AI", "Export", &[Shortcut::secondary("enter")], EIcon::Ai) },
    cmd(Action::Close, "Close editor", "Export", &[Shortcut::key("escape")], EIcon::Close),
];

/// Every palette entry: tools first, then the command table.
pub struct Entry {
    pub action: Action,
    pub name: &'static str,
    pub group: &'static str,
    pub key: Option<Shortcut>,
    pub hint: &'static str,
    pub icon: EIcon,
    pub soon: bool,
}

pub fn entries() -> Vec<Entry> {
    let tools = TOOLS.iter().map(|t| Entry {
        action: Action::Tool(t.tool),
        name: t.name,
        group: "Tools",
        key: Some(t.key),
        hint: t.hint,
        icon: t.icon,
        soon: false,
    });
    let commands = COMMANDS.iter().map(|c| Entry {
        action: c.action,
        name: c.name,
        group: c.group,
        key: c.keys.first().copied(),
        hint: "",
        icon: c.icon,
        soon: c.soon,
    });
    tools.chain(commands).collect()
}

pub fn shortcut_for(action: Action) -> Option<Shortcut> {
    match action {
        Action::Tool(t) => Some(t.spec().key),
        a => COMMANDS.iter().find(|c| c.action == a).and_then(|c| c.keys.first().copied()),
    }
}

pub fn label_for(action: Action) -> String {
    shortcut_for(action).map(|s| s.label()).unwrap_or_default()
}

/// The command a keystroke triggers, ignoring context. Tools win over the
/// command table only for their own unmodified letters.
pub fn lookup(k: &Keystroke) -> Option<Action> {
    if let Some(t) = TOOLS.iter().find(|t| t.key.matches(k)) {
        return Some(Action::Tool(t.tool));
    }
    COMMANDS.iter().find(|c| !c.soon && c.keys.iter().any(|s| s.matches(k))).map(|c| c.action)
}

/// Case-insensitive subsequence match. Returns the score (lower is better)
/// and the matched character indices for highlighting.
pub fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let hay: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).filter(|c| !c.is_whitespace()).collect();
    let mut hits = Vec::with_capacity(needle.len());
    let mut from = 0;
    for n in &needle {
        let at = (from..hay.len()).find(|&i| hay[i] == *n)?;
        hits.push(at);
        from = at + 1;
    }
    let start = hits.first().copied().unwrap_or(0) as i32;
    let gaps: i32 = hits.windows(2).map(|w| (w[1] - w[0] - 1) as i32).sum();
    let word_start = hits.first().is_some_and(|&i| i == 0 || !hay[i - 1].is_alphanumeric());
    Some((gaps * 4 + start + if word_start { 0 } else { 8 }, hits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Modifiers;

    fn ks(key: &str, f: impl FnOnce(&mut Modifiers)) -> Keystroke {
        let mut modifiers = Modifiers::default();
        f(&mut modifiers);
        Keystroke { modifiers, key: key.into(), key_char: None }
    }

    fn secondary(m: &mut Modifiers) {
        if cfg!(target_os = "macos") {
            m.platform = true;
        } else {
            m.control = true;
        }
    }

    #[test]
    fn single_letters_pick_tools_and_chords_pick_commands() {
        assert_eq!(lookup(&ks("a", |_| {})), Some(Action::Tool(Tool::Arrow)));
        assert_eq!(lookup(&ks("e", |m| m.shift = true)), Some(Action::EraseTextOnly));
        assert_eq!(lookup(&ks("n", |m| {
            secondary(m);
            m.shift = true;
        })), Some(Action::Tool(Tool::Combine)));
        assert_eq!(lookup(&ks("c", secondary)), Some(Action::Copy));
        assert_eq!(lookup(&ks("c", |_| {})), Some(Action::Tool(Tool::Crop)));
        assert_eq!(lookup(&ks("y", secondary)), Some(Action::Redo));
        assert_eq!(lookup(&ks("z", |m| {
            secondary(m);
            m.shift = true;
        })), Some(Action::Redo));
        assert_eq!(lookup(&ks("u", secondary)), None, "placeholder commands never dispatch");
    }

    #[test]
    fn labels_follow_the_platform() {
        let l = Shortcut::secondary_shift("n").label();
        if cfg!(target_os = "macos") {
            assert_eq!(l, "⌘⇧N");
        } else {
            assert_eq!(l, "Ctrl+Shift+N");
        }
        assert_eq!(Shortcut::shift("e").label(), "⇧E");
        assert_eq!(Shortcut::key("v").label(), "V");
    }

    #[test]
    fn fuzzy_prefers_tight_word_start_matches() {
        let (arrow, hits) = fuzzy("arr", "Arrow").unwrap();
        assert_eq!(hits, vec![0, 1, 2]);
        let (select_all, _) = fuzzy("arr", "Select all arrows").unwrap();
        assert!(arrow < select_all);
        assert!(fuzzy("zzz", "Arrow").is_none());
    }

    #[test]
    fn every_command_shortcut_is_unique() {
        let mut all: Vec<Shortcut> = TOOLS.iter().map(|t| t.key).collect();
        all.extend(COMMANDS.iter().flat_map(|c| c.keys.iter().copied()));
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "{a:?} bound twice");
        }
    }
}
