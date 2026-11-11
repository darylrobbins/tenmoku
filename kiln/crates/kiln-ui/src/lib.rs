// SPDX-License-Identifier: Apache-2.0

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

mod a11y;
mod activity_rail;
mod assets;
mod badge;
mod breadcrumbs;
mod controls;
mod file_icons;
mod file_icons_coloured;
mod file_icons_tinted;
mod highlighted_label;
mod icon_button;
pub mod l10n;
mod layout;
mod list;
mod menu_bar;
mod nav;
mod palette;
mod prompt;
mod roles;
mod scroll_area;
mod segmented;
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

pub use a11y::{Announcer, Politeness, Role, States, announce, node, node_with};
#[cfg(any(test, feature = "test-support"))]
pub use a11y::{Node, clear_nodes, nodes, set_listening_for_tests};
pub use activity_rail::{ActivityRail, RAIL_WIDTH, RailButton};
pub use assets::Assets;
pub use badge::{BADGE_OVERHANG, BADGE_RING, Badge, BadgeTone};
pub use breadcrumbs::{Breadcrumbs, Crumb, CrumbKind, TOOLBAR_HEIGHT};
pub use controls::{Button, ButtonSize, ButtonVariant, Checkbox, Switch};
pub use controls::{ButtonLook, DISABLED_OPACITY, TOGGLE_ON_ALPHA, focus_ring};
pub use file_icons::{
    Icon, IconRef, IconSet, LucideIcons, file_icon, file_icon_path, file_icon_ref, folder_icon,
    folder_icon_path, folder_icon_ref, folder_icon_ref_named, icon_set, reset_icon_set,
    set_icon_set,
};
pub use file_icons_coloured::ColouredIcons;
pub use file_icons_tinted::TintedIcons;
pub use highlighted_label::{HighlightedLabel, char_ranges};
pub use icon_button::IconButton;
pub use layout::{h_flex, v_flex};
pub use list::{List, ListDelegate, ListEvent, ListRow, RowState};
pub use menu_bar::{MENU_BAR_HEIGHT, MenuEntry, MenuPanel, MenuStrip};
pub use palette::{PALETTE_WIDTH, PaletteFrame};
pub use prompt::{PROMPT_WIDTH, Prompt, prompt_renderer};
pub use roles::{
    BOTTOM_PANEL_DEFAULT, CODE, CODE_SMALL, FOCUS_RING, FamilySlot, FocusRing, FontFamilies,
    PAGE_TITLE, SECTION_HEADING, SHADOW_FLOAT, Shadow, TextRole, UI_BODY, UI_HEADING, UI_LABEL,
    UI_SMALL, height, radius, space, width,
};
pub use scroll_area::{SCROLLBAR_WIDTH, ScrollArea};
pub use segmented::SegmentedControl;
pub use split_pane::{SplitPane, SplitPaneEvent};
pub use status_bar::{
    StatusBarFrame, StatusEntry, StatusEntryColors, StatusFill, StatusTone, status_entry_colors,
};
pub use tab_bar::{TAB_HEIGHT, Tab, TabBar};
pub use tabs::{Tabs, TabsEvent};
pub use text_field::{TextField, TextFieldEvent};
pub use text_input::{SettleEdit, Settled, TextInput, TextInputEvent, Typed};
pub use theme::{
    ActiveTheme, ActiveUiTheme, Appearance, Color, GlobalTheme, ROW_HEIGHT, Theme, ThemeData,
    ThemeMode, Tokens, UiTheme, follow_system_appearance, hsla, observe_system_appearance,
    set_theme, set_theme_mode, set_themes, set_ui_theme, system_appearance_changed, ui_theme,
};
pub use toggle_button::ToggleButton;
pub use tooltip::Tooltip;
pub use tree::{Tree, TreeEvent, TreeModel};
pub use tree_row::{CHEVRON_WIDTH, ICON_SIZE, TREE_INDENT, TreeRow};
// A tree row's badge column, and the dot a folder's mark is.
pub use tree_row::{BADGE_COLUMN, DOT_SIZE};

#[cfg(test)]
mod tests;

type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>;
