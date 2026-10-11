// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's headless core: config layers and settings, app paths, platform
//! conventions, keymaps and menu tables resolved per platform, theme data
//! and a text-edit model. Must never depend on GPUI, so headless tools load
//! the same config and keymaps as the app.
//!
//! [`layers`] loads TOML layers checked against a JSON schema, with
//! [`Diagnostic`]s that point at the offending token; a [`SettingsStore`]
//! layers an app's own settings type from them, keeping the last good; [`ConfigWatcher`] says
//! which files changed; [`write_setting`] changes one setting in a file,
//! keeping its comments; [`AppPaths`] says where an app keeps its files; and
//! [`trust`] records which folders the user trusts.
//!
//! Keys: keystrokes such as `cmd-shift-left` and chords such as
//! `cmd-k cmd-0`; [`Keymap`]s layered from files (an app's defaults, then the
//! user's `keymap.toml`), reloaded by a [`KeymapStore`]; a [`Matcher`] that
//! matches keystrokes as they're pressed against the bindings whose context
//! holds; and reports of bindings that hide or delay each other. [`menu`]
//! is the menu bar as data, its shortcuts read from the keymap.
//!
//! [`TextEdit`] is the model behind a single-line text field: a selection
//! on grapheme boundaries, word motions, IME composition and undo.
//!
//! [`theme`] reads theme files of semantic colour tokens, laid over a
//! bundled dark or light base.

pub mod diagnostic;
mod keymap;
mod keystroke;
mod label;
pub mod layers;
mod load;
mod matcher;
pub mod menu;
pub mod paths;
pub mod platform;
mod report;
pub mod settings;
pub mod text_edit;
pub mod theme;
pub mod trust;
pub mod watch;
pub mod write;

pub use diagnostic::{Diagnostic, Severity};
pub use keymap::{Binding, CatalogError, CommandCatalog, Entry, Keymap, KeymapLayer, Location};
pub use keystroke::{Key, KeySequence, Keystroke, KeystrokeError, Modifiers};
pub use label::{macos_label, pc_label, shortcut_label};
pub use layers::{Layer, load_layer, merge, parse_layer};
pub use load::{
    DEFAULT_KEYMAP_PATH, KEYMAP_FILE, KeymapEvent, KeymapStore, load_keymap, load_keymap_layer,
};
pub use matcher::{KeyOutcome, Matcher};
pub use menu::{
    Alternative, CommandItem, ItemKind, Menu, MenuBar, MenuItem, OsAction, Place, SystemItem,
    SystemMenu, shortcuts,
};
pub use paths::{AppId, AppPaths, PathStyle};
pub use platform::Os;
pub use report::KeymapIssue;
/// serde, for an app's settings type ([`SettingsType`]).
pub use serde;
pub use settings::{ProjectFilter, SettingsEvent, SettingsOptions, SettingsStore, SettingsType};
pub use text_edit::{Motion, Selection, TextEdit};
pub use watch::ConfigWatcher;
pub use write::write_setting;

#[cfg(test)]
mod tests;
