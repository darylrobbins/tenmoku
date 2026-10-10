//! The native menu bar, through kiln-platform's [`kiln_platform::menus`]:
//! [`install`] sets GPUI's menus from a [`fe_keymap::MenuBar`] table, and
//! choosing an item dispatches the same [`RunCommand`] a key would, stamped
//! with the focused element's context and [`fe_command::Source::Menu`].
//!
//! Known limit: every item is enabled; see kiln's module.
//!
//! [`RunCommand`]: crate::commands::RunCommand

pub use kiln_platform::menus::{KEY_CONTEXT, MenuCommand, install, installed, run};

#[cfg(test)]
use kiln_platform::menus::{choose, native_key_equivalent as native};

#[cfg(test)]
#[path = "menu_native_tests.rs"]
mod native_tests;
