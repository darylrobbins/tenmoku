use gpui::{Hsla, rgb, rgba};

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
    pub tab_active: Hsla,
    pub tab_active_text: Hsla,
    pub tab_inactive: Hsla,
    /// The dot on a tab with unsaved changes.
    pub dirty_dot: Hsla,
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
            tab_active: rgb(0x16181d).into(),
            tab_active_text: rgb(0xc8ccd4).into(),
            tab_inactive: rgb(0x121418).into(),
            dirty_dot: rgb(0xd19a66).into(),
        }
    }
}

impl Default for UiTheme {
    fn default() -> Self {
        Self::default_dark()
    }
}
