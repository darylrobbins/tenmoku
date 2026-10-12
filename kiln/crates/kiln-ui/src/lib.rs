// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's widgets: GPUI elements styled by the active [`Theme`], which a
//! host makes active with [`set_theme`] and widgets read with `cx.theme()`
//! ([`ActiveTheme`]) as they render. The constructors that still take a
//! flat [`UiTheme`] ignore it; it remains for apps that set their chrome's
//! colours with [`set_ui_theme`], which sets the theme too. Views that
//! own an app's state, such as a project tree or a tab pane, build their
//! rows and tabs from these.
//!
//! Depends on GPUI and kiln-core (theme data, and [`TextField`]'s editing
//! model). Written for kiln, except [`TextField`], which is based on GPUI's
//! Apache-2.0 input example (see its header); none of gpui-kit is used in
//! it.

mod activity_rail;
mod assets;
mod breadcrumbs;
mod controls;
mod file_icons;
mod highlighted_label;
mod icon_button;
mod layout;
mod list;
mod menu_bar;
mod nav;
mod palette;
mod prompt;
mod scroll_area;
mod split_pane;
mod status_bar;
mod tab_bar;
mod tabs;
pub mod text_field;
mod text_input;
mod theme;
mod toggle_button;
mod tooltip;
mod tree;
mod tree_row;

pub use activity_rail::{ActivityRail, RAIL_WIDTH, RailButton};
pub use assets::Assets;
pub use breadcrumbs::{Breadcrumbs, Crumb, CrumbKind, TOOLBAR_HEIGHT};
pub use controls::{Button, ButtonSize, ButtonVariant, Checkbox, Switch};
pub use file_icons::{Icon, file_icon, folder_icon};
pub use highlighted_label::{HighlightedLabel, char_ranges};
pub use icon_button::IconButton;
pub use layout::{h_flex, v_flex};
pub use list::{List, ListDelegate, ListEvent, ListRow, RowState};
pub use menu_bar::{MENU_BAR_HEIGHT, MenuEntry, MenuPanel, MenuStrip};
pub use palette::{PALETTE_WIDTH, PaletteFrame};
pub use prompt::{PROMPT_WIDTH, Prompt, prompt_renderer};
pub use scroll_area::{SCROLLBAR_WIDTH, ScrollArea};
pub use split_pane::{SplitPane, SplitPaneEvent};
pub use status_bar::{StatusBarFrame, StatusEntry};
pub use tab_bar::{TAB_HEIGHT, Tab, TabBar};
pub use tabs::{Tabs, TabsEvent};
pub use text_field::{TextField, TextFieldEvent};
pub use text_input::{TextInput, TextInputEvent};
pub use theme::{
    ActiveTheme, ActiveUiTheme, Appearance, Color, GlobalTheme, ROW_HEIGHT, Theme, ThemeData,
    ThemeMode, Tokens, UiTheme, follow_system_appearance, hsla, observe_system_appearance,
    set_theme, set_theme_mode, set_themes, set_ui_theme, system_appearance_changed, ui_theme,
};
pub use toggle_button::ToggleButton;
pub use tooltip::Tooltip;
pub use tree::{Tree, TreeEvent, TreeModel};
pub use tree_row::{CHEVRON_WIDTH, ICON_SIZE, TREE_INDENT, TreeRow};

#[cfg(test)]
mod tests;

type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>;
