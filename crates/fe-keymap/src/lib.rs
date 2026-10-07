//! Keystrokes, chords and the keymap: parsing keystrokes such as
//! `cmd-shift-left`, layering keymap files (built-in defaults, then the
//! user's `keymap.toml`), matching keystrokes as they're pressed against the
//! bindings whose context holds, and reporting bindings that hide or delay
//! each other; and the menu bar's table ([`menu`]). Headless: the host
//! feeds it keystrokes and context stacks and runs its chord timer. Must
//! never depend on GPUI.

mod keymap;
mod keystroke;
mod label;
mod load;
mod matcher;
pub mod menu;
mod report;

pub use keymap::{Binding, CatalogError, CommandCatalog, Entry, Keymap, KeymapLayer, Location};
pub use keystroke::{Key, KeySequence, Keystroke, KeystrokeError, Modifiers};
pub use label::macos_label;
pub use load::{
    DEFAULT_KEYMAP_PATH, KEYMAP_FILE, KeymapEvent, KeymapStore, load_keymap, load_keymap_layer,
    user_keymap_path,
};
pub use matcher::{KeyOutcome, Matcher};
pub use menu::{
    Alternative, CommandItem, ItemKind, Menu, MenuBar, MenuItem, OsAction, Place, SystemItem,
    SystemMenu, shortcuts,
};
pub use report::KeymapIssue;

/// The built-in macOS keymap, the first layer under the user's: see
/// [`KeymapStore::load`].
pub const DEFAULT_MACOS: &str = include_str!("../keymaps/default-macos.toml");

/// The built-in macOS menu bar: see [`MenuBar::default_macos`].
pub const DEFAULT_MACOS_MENUS: &str = include_str!("../menus/default-macos.toml");

#[cfg(test)]
mod tests;
