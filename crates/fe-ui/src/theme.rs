use gpui::{Hsla, Pixels, px, rgb, rgba};

/// Every list row's height: the project tree's, the outline's, the quick-open
/// picker's and the search results', so uniform lists lay them out alike.
pub const ROW_HEIGHT: Pixels = px(24.);

/// Colours for the workbench chrome: panels, rows and tabs. Hard-coded
/// until themes land (M5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTheme {
    /// Behind the editor and the empty pane, as the editor's own background.
    pub background: Hsla,
    pub panel_background: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    /// Ignored entries and placeholders.
    pub muted_text: Hsla,
    /// Entries that couldn't be read.
    pub error_text: Hsla,
    pub row_hover: Hsla,
    pub row_selected: Hsla,
    /// The selected row while its list doesn't have focus.
    pub row_selected_inactive: Hsla,
    /// The outline of the selected row while its list has focus.
    pub row_focus_border: Hsla,
    /// A file's icon in a tree row.
    pub icon: Hsla,
    /// A folder's icon in a tree row.
    pub folder_icon: Hsla,
    /// A tree row's disclosure chevron.
    pub chevron: Hsla,
    /// The vertical lines marking a tree row's ancestor levels.
    pub indent_guide: Hsla,
    pub tab_active: Hsla,
    pub tab_active_text: Hsla,
    pub tab_inactive: Hsla,
    /// The dot on a tab with unsaved changes.
    pub dirty_dot: Hsla,
    /// Where a dragged tab would land: the insertion marker in a tab bar,
    /// and, at low alpha, the part of a pane a drop would fill.
    pub drop_target: Hsla,
    /// The bar beside the activity rail's highlighted button.
    pub accent: Hsla,
}

impl UiTheme {
    /// The colours the window had before the workbench: the editor's
    /// background (`EditorTheme::default_dark`) and the placeholder panel's.
    pub fn default_dark() -> Self {
        Self {
            background: rgb(0x16181d).into(),
            panel_background: rgb(0x121418).into(),
            border: rgb(0x23262d).into(),
            text: rgb(0x8b8f98).into(),
            muted_text: rgb(0x4b5160).into(),
            error_text: rgb(0xe06c75).into(),
            row_hover: rgba(0xffffff0a).into(),
            row_selected: rgba(0x4f7cc833).into(),
            row_selected_inactive: rgba(0xffffff0f).into(),
            row_focus_border: rgba(0x4f7cc8aa).into(),
            icon: rgb(0x8b8f98).into(),
            folder_icon: rgb(0x7f9fd8).into(),
            chevron: rgb(0x6c717c).into(),
            indent_guide: rgba(0xffffff14).into(),
            tab_active: rgb(0x16181d).into(),
            tab_active_text: rgb(0xc8ccd4).into(),
            tab_inactive: rgb(0x121418).into(),
            dirty_dot: rgb(0xd19a66).into(),
            drop_target: rgb(0x4f7cc8).into(),
            accent: rgb(0x4f7cc8).into(),
        }
    }
}

impl Default for UiTheme {
    fn default() -> Self {
        Self::default_dark()
    }
}
