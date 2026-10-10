//! Keystrokes, chords and the keymap: parsing keystrokes such as
//! `cmd-shift-left`, layering keymap files (built-in defaults, then the
//! user's `keymap.toml`), matching keystrokes as they're pressed against the
//! bindings whose context holds, and reporting bindings that hide or delay
//! each other; and the menu bar's table ([`menu`]). Headless: the host
//! feeds it keystrokes and context stacks and runs its chord timer. Must
//! never depend on GPUI.

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

use std::path::{Path, PathBuf};

use fe_config::Diagnostic;
use kiln_core::Os;

/// `<config_dir>/keymap.toml`: fastedit's user keymap.
pub fn user_keymap_path() -> Option<PathBuf> {
    fe_config::paths::app_paths().keymap_file()
}

/// The built-in menus, [`MENUS`], as on `os`.
pub fn default_menus_for(
    catalog: Option<&dyn CommandCatalog>,
    os: Os,
) -> (Option<MenuBar>, Vec<Diagnostic>) {
    MenuBar::parse_for(Path::new(menu::DEFAULT_MENUS_PATH), MENUS, catalog, os)
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
/// on any host. `tests/one_keymap.rs` checks the two agree.
pub const DEFAULT_MACOS: &str = include_str!("../keymaps/default-macos.toml");

/// [`DEFAULT`] as resolved on Linux and Windows, for tests that need those
/// bindings on any host. `tests/one_keymap.rs` checks the two agree.
pub const DEFAULT_LINUX: &str = include_str!("../keymaps/default-linux.toml");

/// fastedit's built-in menu bar, for every platform: native on macOS, drawn
/// in the window elsewhere. See [`default_menus`].
pub const MENUS: &str = include_str!("../menus/fastedit.toml");

/// [`MENUS`] as resolved on macOS. `tests/one_keymap.rs` checks they agree.
pub const DEFAULT_MACOS_MENUS: &str = include_str!("../menus/default-macos.toml");

/// [`MENUS`] as resolved on Linux. `tests/one_keymap.rs` checks they agree.
pub const DEFAULT_LINUX_MENUS: &str = include_str!("../menus/default-linux.toml");
