//! Look and feel shared by every Sukurin window: tokens, icons, small widgets.

pub mod icons;
pub mod theme;
pub mod widgets;

pub use icons::{Icon, icon};
pub use theme::{DARK, LIGHT, MONO_FONT, Theme, UI_FONT};
pub use widgets::{icon_button, kbd, mark, pill, render_image};

/// Window app id. Linux desktops match it to `sukurin.desktop` for the dock icon and name.
pub const APP_ID: &str = "sukurin";
