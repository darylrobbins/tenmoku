// SPDX-License-Identifier: Apache-2.0

//! Kiln's platform layer: what differs between macOS, Linux and (later)
//! Windows, behind one API. The only GPUI crate in kiln that may branch on
//! the operating system.
//!
//! [`commands`] is GPUI's side of `kiln-command`: the one action every key
//! and menu item dispatches, and the app's own commands.
//! [`command_actions`] makes commands real GPUI actions, declared with
//! [`commands!`]. [`keys`] is key input: kiln's keymap matched against
//! every keystroke, with an app's [`keys::KeyOwner`] taking part, and
//! [`menus`] the native menu bar, whose items dispatch as keys do.
//! [`action_dispatch`] dispatches an action the app holds, such as a
//! button's, as its command, and [`focus`] keeps keys working when the
//! focused element stops being drawn. [`config`] watches the app's config
//! files (settings, keymap, theme) and applies each change to every
//! window. [`locale`] is the language of the app's own text: the system's
//! preferred languages, and the [`locale::ActiveLocale`] the string
//! catalogue follows. [`Capabilities`] says what a
//! platform does natively, and [`window_options`], [`fonts`] and
//! [`dialogs`] act on it. With the `test-support` feature, `screenshots`
//! is the golden-screenshot harness for an app's examples.

pub mod action_dispatch;
mod capabilities;
pub mod command_actions;
pub mod commands;
pub mod config;
pub mod dialogs;
pub mod focus;
pub mod fonts;
pub mod frame_watch;
pub mod keys;
pub mod locale;
pub mod menus;
#[cfg(feature = "test-support")]
pub mod screenshots;
pub mod window_options;

pub use capabilities::Capabilities;
pub use kiln_core::Os;
pub use window_options::{WindowSpec, window_options};

pub use gpui;
