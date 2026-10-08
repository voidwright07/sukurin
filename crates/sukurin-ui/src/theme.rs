//! Design tokens from the Shattr canvas. Volt (`accent`) marks only the
//! selection and the active tool or mode.

use gpui_kit::Rgba;

#[derive(Clone, Copy)]
pub struct Theme {
    pub canvas: Rgba,
    pub chrome: Rgba,
    pub control: Rgba,
    pub hover: Rgba,
    pub line: Rgba,
    pub line2: Rgba,
    pub text: Rgba,
    pub text2: Rgba,
    pub text3: Rgba,
    pub accent: Rgba,
    pub accent_ink: Rgba,
    pub accent_soft: Rgba,
    pub warn: Rgba,
    pub scrim: Rgba,
    pub hud: Rgba,
}

pub const DARK: Theme = Theme {
    canvas: rgb_c(0x0D0E10),
    chrome: rgb_c(0x151619),
    control: rgb_c(0x1D1F23),
    hover: rgb_c(0x26292E),
    line: rgb_c(0x25282D),
    line2: rgb_c(0x34383F),
    text: rgb_c(0xE8E9EB),
    text2: rgb_c(0xA9ADB4),
    text3: rgb_c(0x8A8F97),
    accent: rgb_c(0xC6F135),
    accent_ink: rgb_c(0x10140A),
    accent_soft: rgba_c(0xC6F13529),
    warn: rgb_c(0xF0A63A),
    scrim: rgba_c(0x0607098F),
    hud: rgba_c(0x0D0E10EB),
};

pub const LIGHT: Theme = Theme {
    canvas: rgb_c(0xE4E5E8),
    chrome: rgb_c(0xFBFBFC),
    control: rgb_c(0xF0F1F3),
    hover: rgb_c(0xE6E7EA),
    line: rgb_c(0xE3E4E8),
    line2: rgb_c(0xCDD0D5),
    text: rgb_c(0x141518),
    text2: rgb_c(0x4C5159),
    text3: rgb_c(0x62676F),
    accent: rgb_c(0x4A6A00),
    accent_ink: rgb_c(0xFFFFFF),
    accent_soft: rgba_c(0x4A6A001F),
    warn: rgb_c(0x9A5800),
    scrim: rgba_c(0x0607098F),
    hud: rgba_c(0xFBFBFCF0),
};

#[cfg(windows)]
pub const UI_FONT: &str = "Segoe UI";
#[cfg(windows)]
pub const MONO_FONT: &str = "Cascadia Mono";
#[cfg(target_os = "macos")]
pub const UI_FONT: &str = ".SystemUIFont";
#[cfg(target_os = "macos")]
pub const MONO_FONT: &str = "Menlo";
#[cfg(all(not(windows), not(target_os = "macos")))]
pub const UI_FONT: &str = "Cantarell";
#[cfg(all(not(windows), not(target_os = "macos")))]
pub const MONO_FONT: &str = "DejaVu Sans Mono";

pub const ANNOTATION_INK: [u32; 8] = [
    0xF0443A, 0xFF8A1F, 0xFFD23F, 0x22B573, 0x2F7BF5, 0xE23D9B, 0xFFFFFF, 0x111111,
];

const fn rgb_c(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

const fn rgba_c(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 24) & 0xFF) as f32 / 255.0,
        g: ((hex >> 16) & 0xFF) as f32 / 255.0,
        b: ((hex >> 8) & 0xFF) as f32 / 255.0,
        a: (hex & 0xFF) as f32 / 255.0,
    }
}
