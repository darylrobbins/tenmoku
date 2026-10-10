// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's platform layer: what differs between macOS, Linux and (later)
//! Windows, behind one API. The only GPUI crate in kiln that may branch on
//! the operating system.
//!
//! [`commands`] is GPUI's side of `kiln-command`: the one action every key
//! and menu item dispatches, and the app's own commands.
//! [`command_actions`] makes commands real GPUI actions, declared with
//! [`commands!`].

pub mod command_actions;
pub mod commands;

pub use gpui;
