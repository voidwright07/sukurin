//! Shattr's own 16 px stroke icons, served to GPUI as SVG assets under
//! `shattr/`. GPUI renders SVGs as alpha masks tinted by `text_color`.

use anyhow::Result;
use gpui_kit::{AssetSource, SharedString, Styled, Svg, px, svg};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Area,
    Window,
    Screen,
    Scrolling,
    Repeat,
    Timer,
    Text,
    Copy,
    Save,
    Pin,
    Edit,
    Upload,
    Ai,
    Close,
    Check,
    Grip,
    Opacity,
    Library,
}

const ICONS: &[(Icon, &str, &str)] = &[
    (Icon::Area, "area", r#"<path d="M2.5 5.5v-3h3M10.5 2.5h3v3M13.5 10.5v3h-3M5.5 13.5h-3v-3M8 6v4M6 8h4"/>"#),
    (Icon::Window, "window", r#"<rect x="1.5" y="2.5" width="13" height="11" rx="1.5"/><path d="M1.5 5.5h13"/>"#),
    (Icon::Screen, "screen", r#"<path d="M1.5 5V1.5H5M11 1.5h3.5V5M14.5 11v3.5H11M5 14.5H1.5V11"/>"#),
    (Icon::Scrolling, "scrolling", r#"<rect x="3.5" y="1.5" width="9" height="13" rx="1.5"/><path d="M8 5v6M6 9l2 2 2-2"/>"#),
    (Icon::Repeat, "repeat", r#"<path d="M2.5 8a5.5 5.5 0 019.6-3.6M13.5 8a5.5 5.5 0 01-9.6 3.6M12.5 1.8v2.8H9.7M3.5 14.2v-2.8h2.8"/>"#),
    (Icon::Timer, "timer", r#"<circle cx="8" cy="9" r="5"/><path d="M8 6.5V9l1.5 1.5M6.5 1.5h3"/>"#),
    (Icon::Text, "text", r#"<path d="M1.5 5V2.5a1 1 0 011-1H5M11 1.5h2.5a1 1 0 011 1V5M14.5 11v2.5a1 1 0 01-1 1H11M5 14.5H2.5a1 1 0 01-1-1V11M5 5.5h6M8 5.5v5"/>"#),
    (Icon::Copy, "copy", r#"<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 5.5v-2a1 1 0 00-1-1h-6a1 1 0 00-1 1v6a1 1 0 001 1h2"/>"#),
    (Icon::Save, "save", r#"<path d="M8 2.5v8M4.5 7L8 10.5 11.5 7M2.5 13.5h11"/>"#),
    (Icon::Pin, "pin", r#"<path d="M9.8 1.8l4.4 4.4-1.8.6-2.6 2.6.4 3.1-1 1-3.1-3.1L2.5 14M4.2 8.3l-1-1 1-1 3.1.4 2.6-2.6z"/>"#),
    (Icon::Edit, "edit", r#"<path d="M10.5 2.5l3 3-8 8h-3v-3z"/>"#),
    (Icon::Upload, "upload", r#"<path d="M8 10.5v-8M4.5 6L8 2.5 11.5 6M2.5 10.5v2a1 1 0 001 1h9a1 1 0 001-1v-2"/>"#),
    (Icon::Ai, "ai", r#"<path d="M2.5 3.5h11v7h-6l-3 2.5v-2.5h-2z"/><path d="M6 7h4M8.5 5.5L10 7 8.5 8.5"/>"#),
    (Icon::Close, "close", r#"<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>"#),
    (Icon::Check, "check", r#"<path d="M3 8.5l3 3 7-7"/>"#),
    (Icon::Grip, "grip", r#"<path d="M6 4h.01M10 4h.01M6 8h.01M10 8h.01M6 12h.01M10 12h.01" stroke-width="2.2"/>"#),
    (Icon::Opacity, "opacity", r#"<circle cx="8" cy="8" r="5.5"/><path d="M8 2.5a5.5 5.5 0 010 11z" fill="black"/>"#),
    (Icon::Library, "library", r#"<rect x="2.5" y="2.5" width="5" height="5" rx="1"/><rect x="8.5" y="2.5" width="5" height="5" rx="1"/><rect x="2.5" y="8.5" width="5" height="5" rx="1"/><rect x="8.5" y="8.5" width="5" height="5" rx="1"/>"#),
];

impl Icon {
    fn spec(self) -> &'static (Icon, &'static str, &'static str) {
        ICONS.iter().find(|(i, _, _)| *i == self).expect("every icon has a table row")
    }

    pub fn path(self) -> SharedString {
        format!("shattr/{}.svg", self.spec().1).into()
    }
}

/// A 16 px icon element; tint with `.text_color(..)`.
pub fn icon(i: Icon) -> Svg {
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
                (format!("shattr/{name}.svg"), doc.into_bytes())
            })
            .collect()
    })
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(doc) = documents().get(path) {
            return Ok(Some(Cow::Borrowed(doc.as_slice())));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all: Vec<SharedString> = documents()
            .keys()
            .filter(|k| k.starts_with(path))
            .map(|k| SharedString::from(k.clone()))
            .collect();
        all.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(all)
    }
}
