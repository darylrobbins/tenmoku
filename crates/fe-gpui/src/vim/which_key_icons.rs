//! The which-key popup's row icons, as LazyVim's which-key draws them: a
//! group's from its `:fegroup` or `[[vim.which_key_group]]`
//! ([`fe_vim::WhichKeyEntry::icon`]), an item's from its command
//! ([`command_icon`]). Icons are the bundled Lucide set's ([`Icon`]), drawn
//! as a mask tinted with one of the theme's semantic colours, by a fixed
//! table per icon ([`icon_colour`]): no colour of their own.

use fe_ui::Icon;
use fe_vim::{WhichKey, WhichKeyEntry, WhichKeyLabel};
use gpui::Hsla;

use crate::EditorTheme;

/// The theme colour an icon is tinted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconColour {
    /// [`EditorTheme::information`].
    Information,
    /// [`EditorTheme::warning`].
    Warning,
    /// [`EditorTheme::error`].
    Error,
    /// [`EditorTheme::hint`].
    Hint,
    /// Success: [`EditorTheme::change_added`], the theme's green for what
    /// went in.
    Success,
}

impl IconColour {
    /// The colour in `theme`.
    pub(crate) fn of(self, theme: &EditorTheme) -> Hsla {
        match self {
            IconColour::Information => theme.information,
            IconColour::Warning => theme.warning,
            IconColour::Error => theme.error,
            IconColour::Hint => theme.hint,
            IconColour::Success => theme.change_added,
        }
    }
}

/// The icon of each command that has one, by ID, before
/// [`NAMESPACE_ICONS`].
pub(crate) const COMMAND_ICONS: &[(&str, &str)] = &[
    // Finding and searching.
    ("quick_open.toggle", "search"),
    ("search.show", "search"),
    ("search.show_replace", "search"),
    ("command_palette.toggle", "list"),
    ("outline.show", "list-tree"),
    ("nav.document_symbols", "list-tree"),
    ("nav.workspace_symbols", "braces"),
    // Files and tabs.
    ("dock.show_project", "folder"),
    ("dock.toggle", "panels-top-left"),
    ("workbench.open_recent", "rotate-ccw"),
    ("workbench.reopen_closed_editor", "rotate-ccw"),
    ("workbench.new_file", "file"),
    ("workbench.open_file", "file"),
    ("workbench.open_folder", "folder-open"),
    ("workbench.next_tab", "files"),
    ("workbench.previous_tab", "files"),
    ("workbench.close_tab", "x"),
    ("workbench.close_other_tabs", "x"),
    ("workbench.pin_tab", "pin"),
    ("workbench.unpin_tab", "pin"),
    ("workbench.harpoon_add", "pin"),
    ("workbench.harpoon_toggle_list", "list"),
    ("editor.save", "save"),
    ("workbench.save_all", "save"),
    ("workbench.save_as", "save"),
    ("workbench.open_settings", "settings"),
    ("workbench.open_settings_ui", "settings"),
    ("workbench.open_keyboard_shortcuts", "settings"),
    ("workbench.toggle_zen_mode", "eye"),
    ("editor.toggle_soft_wrap", "text-wrap"),
    // Windows.
    ("pane.split_down", "rows-2"),
    ("pane.split_right", "columns-2"),
    ("pane.close", "x"),
    ("pane.equalize", "app-window"),
    ("pane.focus_next", "app-window"),
    ("pane.focus_previous", "app-window"),
    ("pane.focus_left", "app-window"),
    ("pane.focus_right", "app-window"),
    ("pane.focus_up", "app-window"),
    ("pane.focus_down", "app-window"),
    // Git, past its namespace's.
    ("git.show_changes", "file-diff"),
    ("git.stage_hunk", "file-diff"),
    ("git.unstage_hunk", "file-diff"),
    ("git.revert_hunk", "file-diff"),
    ("git.log", "git-graph"),
    ("git.log_all", "git-graph"),
    ("git.toggle_blame_annotations", "git-graph"),
    // Problems.
    ("diagnostics.next", "triangle-alert"),
    ("diagnostics.previous", "triangle-alert"),
    ("problems.show", "triangle-alert"),
    ("app.quit", "x"),
];

/// The icon of every command of a namespace (`git` of `git.push`) that
/// [`COMMAND_ICONS`] doesn't list.
pub(crate) const NAMESPACE_ICONS: &[(&str, &str)] = &[
    ("git", "git-branch"),
    ("lsp", "code"),
    ("terminal", "terminal"),
    ("tasks", "play"),
    ("chat", "bot"),
    ("ai", "bot"),
];

/// The colour of each icon the popup draws, by name; one not listed, a
/// group's named in a vimrc, takes [`IconColour::Information`], a group
/// title's colour.
pub(crate) const ICON_COLOURS: &[(&str, IconColour)] = &[
    ("search", IconColour::Success),
    ("list", IconColour::Hint),
    ("list-tree", IconColour::Hint),
    ("braces", IconColour::Hint),
    ("folder", IconColour::Information),
    ("folder-open", IconColour::Information),
    ("panels-top-left", IconColour::Information),
    ("app-window", IconColour::Information),
    ("columns-2", IconColour::Information),
    ("rows-2", IconColour::Information),
    ("file", IconColour::Information),
    ("files", IconColour::Information),
    ("rotate-ccw", IconColour::Information),
    ("save", IconColour::Information),
    ("undo", IconColour::Information),
    ("redo", IconColour::Information),
    ("pin", IconColour::Hint),
    ("settings", IconColour::Hint),
    ("eye", IconColour::Hint),
    ("text-wrap", IconColour::Hint),
    ("toggle-right", IconColour::Hint),
    ("git-branch", IconColour::Warning),
    ("git-graph", IconColour::Warning),
    ("file-diff", IconColour::Warning),
    ("code", IconColour::Success),
    ("terminal", IconColour::Success),
    ("play", IconColour::Success),
    ("flask-conical", IconColour::Success),
    ("bot", IconColour::Information),
    ("triangle-alert", IconColour::Warning),
    ("bug", IconColour::Error),
    ("x", IconColour::Error),
    ("log-out", IconColour::Error),
];

/// The icon the popup draws beside a map running the command `id`, by
/// name: its own ([`COMMAND_ICONS`]), else its namespace's
/// ([`NAMESPACE_ICONS`]); none for the rest.
pub(crate) fn command_icon(id: &str) -> Option<&'static str> {
    let listed = |table: &[(&str, &'static str)], key: &str| {
        (table.iter())
            .find(|(listed, _)| *listed == key)
            .map(|&(_, icon)| icon)
    };
    listed(COMMAND_ICONS, id).or_else(|| {
        let (namespace, _) = id.split_once('.')?;
        listed(NAMESPACE_ICONS, namespace)
    })
}

/// The bundled icon named `name`, if there is one.
pub(crate) fn bundled(name: &str) -> Option<Icon> {
    Icon::ALL.iter().copied().find(|icon| icon.name() == name)
}

/// The colour the icon named `name` is tinted with ([`ICON_COLOURS`]).
pub(crate) fn icon_colour(name: &str) -> IconColour {
    (ICON_COLOURS.iter())
        .find(|(listed, _)| *listed == name)
        .map_or(IconColour::Information, |&(_, colour)| colour)
}

/// Gives each item of `which` that runs a command and has no icon its
/// command's ([`command_icon`]). A group's icon is its record's.
pub(crate) fn fill(which: &mut WhichKey) {
    for entry in &mut which.entries {
        if let (None, WhichKeyLabel::Command(id)) = (&entry.icon, &entry.label) {
            entry.icon = command_icon(id).map(str::to_owned);
        }
    }
}

/// The icon `entry` draws, with its colour in `theme`: none for one
/// without an icon, or whose icon isn't bundled.
pub(crate) fn entry_icon(entry: &WhichKeyEntry, theme: &EditorTheme) -> Option<(Icon, Hsla)> {
    let name = entry.icon.as_deref()?;
    Some((bundled(name)?, icon_colour(name).of(theme)))
}

/// Whether any of `which`'s entries draws an icon: only then does the
/// popup make room for one in each row.
pub(crate) fn any_icon(which: &WhichKey) -> bool {
    (which.entries.iter()).any(|entry| entry.icon.as_deref().and_then(bundled).is_some())
}
