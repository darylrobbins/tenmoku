//! fastedit's UI primitives: stateless GPUI elements styled by a
//! [`UiTheme`]. Views that own state (the project panel, the tab pane) live
//! in `fe-workbench` and build their rows and tabs from these.
//!
//! Depends on GPUI alone, never on a workspace crate. Written for fastedit:
//! nothing here is copied from gpui-kit or elsewhere.

mod highlighted_label;
mod palette;
mod tab_bar;
mod text_input;
mod theme;
mod toggle_button;
mod tree_row;

pub use highlighted_label::{HighlightedLabel, char_ranges};
pub use palette::{PALETTE_WIDTH, PaletteFrame};
pub use tab_bar::{TAB_HEIGHT, Tab, TabBar};
pub use text_input::{TextInput, TextInputEvent};
pub use theme::UiTheme;
pub use toggle_button::ToggleButton;
pub use tree_row::{ROW_HEIGHT, TreeRow};

type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>;
