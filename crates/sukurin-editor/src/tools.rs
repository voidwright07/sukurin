//! The tool table. The strip, tooltips, properties bar, key dispatch and
//! command palette all read this one table.

use crate::keys::Shortcut;
use crate::icons::EIcon;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Select,
    Crop,
    Arrow,
    Line,
    Rect,
    Oval,
    Pen,
    Highlighter,
    Text,
    Step,
    Spotlight,
    Blur,
    Erase,
    Magnifier,
    Measure,
    Guides,
    Overlay,
    Backdrop,
    Combine,
}

/// Which branch of the contextual properties bar a tool shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Props {
    Select,
    Arrow,
    Stroke,
    Text,
    Spot,
    Redact,
    Measure,
    Overlay,
    Backdrop,
    Combine,
}

pub struct ToolSpec {
    pub tool: Tool,
    pub name: &'static str,
    pub key: Shortcut,
    pub hint: &'static str,
    /// Strip group; a separator is drawn between groups.
    pub group: u8,
    pub props: Props,
    pub icon: EIcon,
}

const fn spec(
    tool: Tool,
    name: &'static str,
    key: Shortcut,
    hint: &'static str,
    group: u8,
    props: Props,
    icon: EIcon,
) -> ToolSpec {
    ToolSpec { tool, name, key, hint, group, props, icon }
}

pub const TOOLS: [ToolSpec; 19] = [
    spec(Tool::Select, "Select", Shortcut::key("v"), "Move, resize, delete · ⇧ constrain", 0, Props::Select, EIcon::Select),
    spec(Tool::Crop, "Crop", Shortcut::key("c"), "Drag edges · ↵ apply · Esc cancel", 0, Props::Select, EIcon::Crop),
    spec(Tool::Arrow, "Arrow", Shortcut::key("a"), "Drag to draw · drag the midpoint to bend · ⇧ snaps 15°", 1, Props::Arrow, EIcon::Arrow),
    spec(Tool::Line, "Line", Shortcut::key("l"), "⇧ snaps 15° · ⌥ from centre", 1, Props::Stroke, EIcon::Line),
    spec(Tool::Rect, "Rectangle", Shortcut::key("r"), "⇧ square · ⌥ from centre", 1, Props::Stroke, EIcon::Rect),
    spec(Tool::Oval, "Oval", Shortcut::key("o"), "⇧ circle · ⌥ from centre", 1, Props::Stroke, EIcon::Oval),
    spec(Tool::Pen, "Freehand", Shortcut::key("p"), "Hold ⇧ for straight segments", 1, Props::Stroke, EIcon::Pen),
    spec(Tool::Highlighter, "Highlighter", Shortcut::key("h"), "Snaps to lines of text", 1, Props::Stroke, EIcon::Highlighter),
    spec(Tool::Text, "Text label", Shortcut::key("t"), "Click to place · Esc to finish", 1, Props::Text, EIcon::Text),
    spec(Tool::Step, "Step counter", Shortcut::key("n"), "Click to drop the next number", 1, Props::Text, EIcon::Step),
    spec(Tool::Spotlight, "Spotlight", Shortcut::key("s"), "Dims everything outside the shape", 2, Props::Spot, EIcon::Spotlight),
    spec(Tool::Blur, "Blur · pixelate", Shortcut::key("b"), "Drag over secrets · B again toggles mode", 2, Props::Redact, EIcon::Blur),
    spec(Tool::Erase, "Erase", Shortcut::key("e"), "Fills from surroundings · ⇧E text only", 2, Props::Redact, EIcon::Erase),
    spec(Tool::Magnifier, "Magnifier", Shortcut::key("z"), "Drag a source, then place the callout", 2, Props::Stroke, EIcon::Magnifier),
    spec(Tool::Measure, "Measure", Shortcut::key("m"), "Drag between edges · hover shows gaps", 3, Props::Measure, EIcon::Measure),
    spec(Tool::Guides, "Guides", Shortcut::key("g"), "Drag from the canvas edge · double-click removes", 3, Props::Measure, EIcon::Guides),
    spec(Tool::Overlay, "Image overlay", Shortcut::key("i"), "Paste or drop an image · [ ] opacity", 4, Props::Overlay, EIcon::Overlay),
    spec(Tool::Backdrop, "Backdrop", Shortcut::key("f"), "Padding, gradient, shadow, corners", 4, Props::Backdrop, EIcon::Backdrop),
    spec(Tool::Combine, "Combine", Shortcut::secondary_shift("n"), "Add another capture to this canvas", 4, Props::Combine, EIcon::Combine),
];

impl Tool {
    pub fn spec(self) -> &'static ToolSpec {
        TOOLS.iter().find(|s| s.tool == self).expect("every tool has a table row")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_keys_are_unique() {
        for (i, a) in TOOLS.iter().enumerate() {
            for b in &TOOLS[i + 1..] {
                assert_ne!(a.key, b.key, "{} and {} share a key", a.name, b.name);
            }
        }
    }
}
