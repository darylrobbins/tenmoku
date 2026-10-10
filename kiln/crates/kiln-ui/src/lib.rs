// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's widgets: GPUI elements styled by a [`UiTheme`], which a host
//! makes active with [`set_ui_theme`]. Views that own an app's state, such
//! as a project tree or a tab pane, build their rows and tabs from these.
//!
//! Depends on GPUI alone. Written for kiln: nothing here is copied from
//! gpui-kit or elsewhere.

mod activity_rail;
mod assets;
mod breadcrumbs;
mod controls;
mod file_icons;
mod highlighted_label;
mod icon_button;
mod menu_bar;
mod palette;
mod prompt;
mod status_bar;
mod tab_bar;
mod text_input;
mod theme;
mod toggle_button;
mod tooltip;
mod tree_row;

pub use activity_rail::{ActivityRail, RAIL_WIDTH, RailButton};
pub use assets::Assets;
pub use breadcrumbs::{Breadcrumbs, Crumb, CrumbKind, TOOLBAR_HEIGHT};
pub use controls::{Button, ButtonSize, ButtonVariant, Checkbox, Switch};
pub use file_icons::{Icon, file_icon, folder_icon};
pub use highlighted_label::{HighlightedLabel, char_ranges};
pub use icon_button::IconButton;
pub use menu_bar::{MENU_BAR_HEIGHT, MenuEntry, MenuPanel, MenuStrip};
pub use palette::{PALETTE_WIDTH, PaletteFrame};
pub use prompt::{PROMPT_WIDTH, Prompt, prompt_renderer};
pub use status_bar::{StatusBarFrame, StatusEntry};
pub use tab_bar::{TAB_HEIGHT, Tab, TabBar};
pub use text_input::{TextInput, TextInputEvent};
pub use theme::{ActiveUiTheme, ROW_HEIGHT, UiTheme, set_ui_theme, ui_theme};
pub use toggle_button::ToggleButton;
pub use tooltip::Tooltip;
pub use tree_row::{CHEVRON_WIDTH, ICON_SIZE, TREE_INDENT, TreeRow};

#[cfg(test)]
mod tests;

type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>;
