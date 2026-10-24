//! Keystrokes, chords and the keymap: parsing keystrokes such as
//! `cmd-shift-left`, layering keymap files (built-in defaults, then the
//! user's `keymap.toml`), matching keystrokes as they're pressed against the
//! bindings whose context holds, and reporting bindings that hide or delay
//! each other; the menu bar's table ([`menu`]); and the Keyboard Shortcuts
//! editor's rows ([`ShortcutsModel`]). Headless: the host feeds it
//! keystrokes and context stacks and runs its chord timer. Must never
//! depend on GPUI.

// The keymap and menu machinery is kiln's (docs/decisions/app-framework.md);
// fastedit keeps its names for it, and its own keymaps and menus here.
pub use kiln_core::menu;
pub use kiln_core::{
    Alternative, Binding, CatalogError, CommandCatalog, CommandItem, DEFAULT_KEYMAP_PATH, Entry,
    ItemKind, KEYMAP_FILE, Key, KeyOutcome, KeySequence, Keymap, KeymapEvent, KeymapIssue,
    KeymapLayer, KeymapStore, Keystroke, KeystrokeError, Location, Matcher, Menu, MenuBar,
    MenuItem, Modifiers, OsAction, Place, SystemItem, SystemMenu, load_keymap, load_keymap_layer,
    macos_label, pc_label, shortcut_label, shortcuts,
};

pub use fe_command::{CommandId, ContextPredicate};

mod keymap_edit;
mod shortcuts_model;

pub use keymap_edit::{
    DefaultKeymap, KEYMAP_HEADER, KeymapEdit, KeymapEditError, KeymapWrite, edit_keymap,
    edit_keymap_on, write_keymap, write_keymap_checked, write_keymap_hooked, write_keymap_on,
};

pub use shortcuts_model::{
    CommandInfo, ShortcutBinding, ShortcutIssue, ShortcutRow, ShortcutSource, ShortcutsModel,
};

use std::path::{Path, PathBuf};

use fe_config::Diagnostic;
use kiln_core::Os;

/// `<config_dir>/keymap.toml`: fastedit's user keymap.
pub fn user_keymap_path() -> Option<PathBuf> {
    fe_config::paths::app_paths().keymap_file()
}

/// The frames a menu item's shortcut is resolved in: the workbench with
/// an editor focused.
pub const MENU_SHORTCUT_CONTEXT: [&str; 2] = ["workbench", "editor"];

/// The built-in menus, [`MENUS`], as on `os`.
pub fn default_menus_for(
    catalog: Option<&dyn CommandCatalog>,
    os: Os,
) -> (Option<MenuBar>, Vec<Diagnostic>) {
    let path = Path::new(menu::DEFAULT_MENUS_PATH);
    MenuBar::parse_in(path, MENUS, catalog, os, &MENU_SHORTCUT_CONTEXT)
}

/// The built-in menus as on macOS, with its app menu.
pub fn default_menus_macos(
    catalog: Option<&dyn CommandCatalog>,
) -> (Option<MenuBar>, Vec<Diagnostic>) {
    default_menus_for(catalog, Os::MacOs)
}

/// The built-in menus as on Linux: macOS's, but for its app menu, whose
/// Settings… and Quit are in File.
pub fn default_menus_linux(
    catalog: Option<&dyn CommandCatalog>,
) -> (Option<MenuBar>, Vec<Diagnostic>) {
    default_menus_for(catalog, Os::Linux)
}

/// This platform's built-in menus.
pub fn default_menus(catalog: Option<&dyn CommandCatalog>) -> (Option<MenuBar>, Vec<Diagnostic>) {
    default_menus_for(catalog, Os::current())
}

/// fastedit's built-in keymap, for every platform: the first layer under
/// the user's, resolved for the platform it's loaded on (see
/// [`KeymapLayer::parse_for`]).
pub const DEFAULT: &str = include_str!("../keymaps/fastedit.toml");

/// [`DEFAULT`] as resolved on macOS, for tests that need macOS's bindings
/// on any host. Generated from [`DEFAULT`] by this crate's build script.
pub const DEFAULT_MACOS: &str = include_str!(concat!(env!("OUT_DIR"), "/keymap-macos.toml"));

/// [`DEFAULT`] as resolved on Linux and Windows, for tests that need those
/// bindings on any host. Generated from [`DEFAULT`] by this crate's build
/// script.
pub const DEFAULT_LINUX: &str = include_str!(concat!(env!("OUT_DIR"), "/keymap-linux.toml"));

/// fastedit's built-in menu bar, for every platform: native on macOS, drawn
/// in the window elsewhere. See [`default_menus`].
pub const MENUS: &str = include_str!("../menus/fastedit.toml");

/// [`MENUS`] as resolved on macOS. Generated from [`MENUS`] by this crate's
/// build script.
pub const DEFAULT_MACOS_MENUS: &str = include_str!(concat!(env!("OUT_DIR"), "/menus-macos.toml"));

/// [`MENUS`] as resolved on Linux. Generated from [`MENUS`] by this crate's
/// build script.
pub const DEFAULT_LINUX_MENUS: &str = include_str!(concat!(env!("OUT_DIR"), "/menus-linux.toml"));
