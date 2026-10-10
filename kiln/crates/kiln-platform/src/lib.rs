// SPDX-License-Identifier: LicenseRef-Proprietary

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
//! [`Capabilities`] says what a platform does natively, and
//! [`window_options`], [`fonts`] and [`dialogs`] act on it.

mod capabilities;
pub mod command_actions;
pub mod commands;
pub mod dialogs;
pub mod fonts;
pub mod keys;
pub mod menus;
pub mod window_options;

pub use capabilities::Capabilities;
pub use kiln_core::Os;
pub use window_options::{WindowSpec, window_options};

pub use gpui;
