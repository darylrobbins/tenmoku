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

/// `<config_dir>/keymap.toml`: fastedit's user keymap.
pub fn user_keymap_path() -> Option<PathBuf> {
    fe_config::paths::app_paths().keymap_file()
}

/// The built-in macOS menus, [`DEFAULT_MACOS_MENUS`].
pub fn default_menus_macos(
    catalog: Option<&dyn CommandCatalog>,
) -> (Option<MenuBar>, Vec<Diagnostic>) {
    MenuBar::parse(
        Path::new(menu::DEFAULT_MENUS_PATH),
        DEFAULT_MACOS_MENUS,
        catalog,
    )
}

/// The built-in Linux menus, [`DEFAULT_LINUX_MENUS`]: macOS's, but for its
/// app menu, whose Settings… and Quit are in File.
pub fn default_menus_linux(
    catalog: Option<&dyn CommandCatalog>,
) -> (Option<MenuBar>, Vec<Diagnostic>) {
    MenuBar::parse(
        Path::new(menu::DEFAULT_MENUS_PATH),
        DEFAULT_LINUX_MENUS,
        catalog,
    )
}

/// This platform's built-in menus: [`default_menus_macos`] on macOS,
/// [`default_menus_linux`] elsewhere.
pub fn default_menus(catalog: Option<&dyn CommandCatalog>) -> (Option<MenuBar>, Vec<Diagnostic>) {
    if cfg!(target_os = "macos") {
        default_menus_macos(catalog)
    } else {
        default_menus_linux(catalog)
    }
}

/// The built-in macOS keymap, the first layer under the user's: see
/// [`KeymapStore::load`].
pub const DEFAULT_MACOS: &str = include_str!("../keymaps/default-macos.toml");

/// The built-in keymap where there's no cmd key (Linux, Windows): the
/// macOS one's bindings with ctrl for cmd, and PC keys for macOS's own.
pub const DEFAULT_LINUX: &str = include_str!("../keymaps/default-linux.toml");

/// This platform's built-in keymap: [`DEFAULT_MACOS`] on macOS,
/// [`DEFAULT_LINUX`] elsewhere.
pub const DEFAULT: &str = if cfg!(target_os = "macos") {
    DEFAULT_MACOS
} else {
    DEFAULT_LINUX
};

/// The built-in macOS menu bar: see [`default_menus_macos`].
pub const DEFAULT_MACOS_MENUS: &str = include_str!("../menus/default-macos.toml");

/// The built-in Linux menu bar, drawn in the window: see
/// [`default_menus_linux`].
pub const DEFAULT_LINUX_MENUS: &str = include_str!("../menus/default-linux.toml");
