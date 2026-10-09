//! Editor icons, served as SVG assets under `editor/`. GPUI renders SVGs as
//! alpha masks, so every use tints explicitly with `text_color`.

use anyhow::Result;
use gpui_kit::{AssetSource, SharedString, Styled, Svg, px, svg};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EIcon {
    Undo,
    Redo,
    Copy,
    Save,
    Pin,
    Upload,
    Search,
    Ai,
    Select,
    Crop,
    Arrow,
    ArrowStraight,
    ArrowCurved,
    ArrowBendable,
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
    Inspector,
    Keyboard,
    Minus,
    Plus,
    Fit,
    Solid,
    Dashed,
    Dotted,
    Close,
    Link,
    Picker,
    Swap,
    Check,
    WinMin,
    WinMax,
    WinRestore,
    WinClose,
    Gear,
}

const ICONS: &[(EIcon, &str, &str)] = &[
    (EIcon::Undo, "undo", r#"<path d="M5.5 3.5L2.5 6.5l3 3M2.5 6.5H10a3.5 3.5 0 010 7H7"/>"#),
    (EIcon::Redo, "redo", r#"<path d="M10.5 3.5l3 3-3 3M13.5 6.5H6a3.5 3.5 0 000 7h3"/>"#),
    (EIcon::Copy, "copy", r#"<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 5.5v-2a1 1 0 00-1-1h-6a1 1 0 00-1 1v6a1 1 0 001 1h2"/>"#),
    (EIcon::Save, "save", r#"<path d="M8 2.5v8M4.5 7L8 10.5 11.5 7M2.5 13.5h11"/>"#),
    (EIcon::Pin, "pin", r#"<path d="M9.8 1.8l4.4 4.4-1.8.6-2.6 2.6.4 3.1-1 1-3.1-3.1L2.5 14M4.2 8.3l-1-1 1-1 3.1.4 2.6-2.6z"/>"#),
    (EIcon::Upload, "upload", r#"<path d="M8 10.5v-8M4.5 6L8 2.5 11.5 6M2.5 10.5v2a1 1 0 001 1h9a1 1 0 001-1v-2"/>"#),
    (EIcon::Search, "search", r#"<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5L14 14"/>"#),
    (EIcon::Ai, "ai", r#"<path d="M2.5 3.5h11v7h-6l-3 2.5v-2.5h-2z"/><path d="M6 7h4M8.5 5.5L10 7 8.5 8.5"/>"#),
    (EIcon::Select, "select", r#"<path d="M3.5 2.5l9 4.2-3.9 1.2-1.6 3.8z"/>"#),
    (EIcon::Crop, "crop", r#"<path d="M4.5 1.5v10h10M1.5 4.5h10v10"/>"#),
    (EIcon::Arrow, "arrow", r#"<path d="M3 13L13 3M6.5 3H13v6.5"/>"#),
    (EIcon::ArrowStraight, "arrow-straight", r#"<path d="M3 13L13 3M7.5 3H13v5.5"/>"#),
    (EIcon::ArrowCurved, "arrow-curved", r#"<path d="M3 13c0-5.5 3.5-9.5 9.5-9.5M9.5 1.5l3 2-2 3"/>"#),
    (EIcon::ArrowBendable, "arrow-bendable", r#"<path d="M3 13l4-5.5 6-4.5M9.6 2.6l3.4.4-.8 3.3"/><circle cx="7" cy="7.5" r="1.3"/>"#),
    (EIcon::Line, "line", r#"<path d="M3 13L13 3"/>"#),
    (EIcon::Rect, "rect", r#"<rect x="2.5" y="3.5" width="11" height="9" rx="1"/>"#),
    (EIcon::Oval, "oval", r#"<ellipse cx="8" cy="8" rx="5.5" ry="4.5"/>"#),
    (EIcon::Pen, "pen", r#"<path d="M2.5 11c2-4 3.5-6 5-5s-1.5 4 .5 4.5S11 7 13.5 5"/>"#),
    (EIcon::Highlighter, "highlighter", r#"<path d="M10 2.5l3.5 3.5L8 11.5H5.5V9zM3 14h10"/>"#),
    (EIcon::Text, "text", r#"<path d="M3 4.5V3h10v1.5M8 3v10M6 13h4"/>"#),
    (EIcon::Step, "step", r#"<circle cx="8" cy="8" r="5.5"/><path d="M7 6.2l1.5-1v6"/>"#),
    (EIcon::Spotlight, "spotlight", r#"<circle cx="8" cy="8" r="3.5"/><path d="M1.5 4V1.5H4M12 1.5h2.5V4M14.5 12v2.5H12M4 14.5H1.5V12"/>"#),
    (EIcon::Blur, "blur", r#"<path d="M2.5 2.5h3.5v3.5H2.5zM9.5 2.5H13v3.5H9.5zM6 6h3.5v3.5H6zM2.5 9.5h3.5V13H2.5zM9.5 9.5H13V13H9.5z"/>"#),
    (EIcon::Erase, "erase", r#"<path d="M6 13.5h7.5M3.2 9.8l6-6.3a1.4 1.4 0 012 0l2.3 2.3a1.4 1.4 0 010 2L8 13.5H5.8z"/>"#),
    (EIcon::Magnifier, "magnifier", r#"<circle cx="7" cy="7" r="4.5"/><path d="M10.3 10.3L14 14M5 7h4M7 5v4"/>"#),
    (EIcon::Measure, "measure", r#"<path d="M1.8 10.6l8.8-8.8 3.6 3.6-8.8 8.8zM5 7.4l1.3 1.3M7 5.4l1.3 1.3M9 3.4l1.3 1.3"/>"#),
    (EIcon::Guides, "guides", r#"<path d="M5.5 1.5v13M1.5 10.5h13" stroke-dasharray="2 2.2"/>"#),
    (EIcon::Overlay, "overlay", r#"<rect x="2.5" y="2.5" width="8" height="8" rx="1"/><rect x="5.5" y="5.5" width="8" height="8" rx="1"/>"#),
    (EIcon::Backdrop, "backdrop", r#"<rect x="1.5" y="2.5" width="13" height="11" rx="2"/><rect x="4.5" y="5.5" width="7" height="5" rx=".5"/>"#),
    (EIcon::Combine, "combine", r#"<path d="M2.5 2.5h5v5h-5zM8.5 8.5h5v5h-5zM11 2v5M8.5 4.5h5"/>"#),
    (EIcon::Inspector, "inspector", r#"<rect x="1.5" y="2.5" width="13" height="11" rx="1.5"/><path d="M10 2.5v11"/>"#),
    (EIcon::Keyboard, "keyboard", r#"<rect x="1.5" y="4" width="13" height="8.5" rx="1.5"/><path d="M4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M5 10h6"/>"#),
    (EIcon::Minus, "minus", r#"<path d="M4 8h8"/>"#),
    (EIcon::Plus, "plus", r#"<path d="M4 8h8M8 4v8"/>"#),
    (EIcon::Fit, "fit", r#"<path d="M1.5 5V1.5H5M11 1.5h3.5V5M14.5 11v3.5H11M5 14.5H1.5V11"/>"#),
    (EIcon::Solid, "solid", r#"<path d="M2 8h12"/>"#),
    (EIcon::Dashed, "dashed", r#"<path d="M2 8h12" stroke-dasharray="3 2.5"/>"#),
    (EIcon::Dotted, "dotted", r#"<path d="M2 8h12" stroke-dasharray=".1 3"/>"#),
    (EIcon::Close, "close", r#"<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>"#),
    (EIcon::Link, "link", r#"<path d="M6.5 9.5l3-3M7 4.5l1-1a2.8 2.8 0 014 4l-1 1M9 11.5l-1 1a2.8 2.8 0 01-4-4l1-1"/>"#),
    (EIcon::Picker, "picker", r#"<path d="M10.5 2.8l2.7 2.7-1.5 1.5-2.7-2.7zM9.8 5.6L4 11.4 3.5 13l1.6-.5 5.8-5.8"/>"#),
    (EIcon::Swap, "swap", r#"<path d="M3.5 5.5h9L10.5 3.5M12.5 10.5h-9l2 2"/>"#),
    (EIcon::Check, "check", r#"<path d="M3 8.5l3 3 7-7"/>"#),
    (EIcon::WinMin, "win-min", r#"<path d="M3 8h10" stroke-width="1" stroke-linecap="butt"/>"#),
    (EIcon::WinMax, "win-max", r#"<rect x="3.5" y="3.5" width="9" height="9" stroke-width="1"/>"#),
    (EIcon::WinRestore, "win-restore", r#"<rect x="3.5" y="5.5" width="7" height="7" stroke-width="1"/><path d="M5.5 5.5v-2h7v7h-2" stroke-width="1"/>"#),
    (EIcon::WinClose, "win-close", r#"<path d="M3 3l10 10M13 3L3 13" stroke-width="1" stroke-linecap="butt"/>"#),
    (EIcon::Gear, "gear", r#"<circle cx="8" cy="8" r="2.5"/><path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"/>"#),
];

impl EIcon {
    fn name(self) -> &'static str {
        ICONS.iter().find(|(i, _, _)| *i == self).map(|(_, n, _)| *n).expect("every icon has a table row")
    }

    pub fn path(self) -> SharedString {
        format!("editor/{}.svg", self.name()).into()
    }
}

/// A 16 px icon; tint with `.text_color(..)`.
pub fn icon(i: EIcon) -> Svg {
    svg().path(i.path()).size(px(16.)).flex_none()
}

fn documents() -> &'static HashMap<String, Vec<u8>> {
    static DOCS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
    DOCS.get_or_init(|| {
        ICONS
            .iter()
            .map(|(_, name, body)| {
                let doc = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
                );
                (format!("editor/{name}.svg"), doc.into_bytes())
            })
            .collect()
    })
}

/// Serves the editor's icons and falls back to the rest of Sukurin's assets.
/// The app registers this instead of `sukurin_ui::icons::Assets`.
pub struct EditorAssets;

impl AssetSource for EditorAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match documents().get(path) {
            Some(doc) => Ok(Some(Cow::Borrowed(doc.as_slice()))),
            None => sukurin_ui::icons::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all: Vec<SharedString> =
            documents().keys().filter(|k| k.starts_with(path)).map(|k| SharedString::from(k.clone())).collect();
        all.extend(sukurin_ui::icons::Assets.list(path)?);
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_served() {
        for (i, _, _) in ICONS {
            let doc = EditorAssets.load(&i.path()).unwrap().expect("served");
            assert!(std::str::from_utf8(&doc).unwrap().starts_with("<svg"));
        }
    }
}
