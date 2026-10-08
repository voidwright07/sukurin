//! Look and feel shared by every Shattr window: tokens, icons, small widgets.

pub mod icons;
pub mod theme;
pub mod widgets;

pub use icons::{Icon, icon};
pub use theme::{DARK, LIGHT, MONO_FONT, Theme, UI_FONT};
pub use widgets::{icon_button, kbd, pill, render_image};
