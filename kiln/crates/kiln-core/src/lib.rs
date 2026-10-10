// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's headless core: config layers and settings, app paths, platform
//! conventions, keymaps and menu tables resolved per platform, theme data
//! and a text-edit model. Must never depend on GPUI, so headless tools load
//! the same config and keymaps as the app.
//!
//! [`layers`] loads TOML layers checked against a JSON schema, with
//! [`Diagnostic`]s that point at the offending token; [`ConfigWatcher`] says
//! which files changed; [`write_setting`] changes one setting in a file,
//! keeping its comments; [`AppPaths`] says where an app keeps its files; and
//! [`trust`] records which folders the user trusts.

pub mod diagnostic;
pub mod layers;
pub mod paths;
pub mod platform;
pub mod trust;
pub mod watch;
pub mod write;

pub use diagnostic::{Diagnostic, Severity};
pub use layers::{Layer, load_layer, merge, parse_layer};
pub use paths::{AppId, AppPaths, PathStyle};
pub use platform::Os;
pub use watch::ConfigWatcher;
pub use write::write_setting;

#[cfg(test)]
mod tests;
