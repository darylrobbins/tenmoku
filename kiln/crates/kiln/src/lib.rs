// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln: an app framework on GPUI that hides the differences between
//! platforms and provides high-level widgets, in GPUI's own idioms. This
//! crate sets an app up ([`app()`], or [`application`] and [`init`] by
//! hand) and re-exports the others; see `docs/framework.md` in kiln.

mod app_builder;
mod app_commands;
mod config;
mod ext;
mod setup;

pub use gpui;
pub use kiln_command as command;
pub use kiln_core as core;
pub use kiln_platform as platform;
pub use kiln_shell as shell;
pub use kiln_ui as ui;

/// The function [`app()`] beside the module [`app`](mod@app) of app commands:
/// `kiln::app(..)` sets an app up, and `kiln::app::Quit` is a command.
pub use app_builder::{AppBuilder, Started, app};
pub use ext::binding_label_for;
/// serde, for an app's settings type without a serde dependency of its
/// own: `#[derive(Deserialize, Serialize)]` with
/// `#[serde(crate = "kiln::serde", default)]`.
pub use kiln_core::serde;
pub use kiln_core::{SettingsType, settings::SettingsEvent};
pub use kiln_platform::WindowSpec;
pub use kiln_platform::config::{ConfigDiagnostics, GlobalSettings};
/// A window's root view, around the app's: the chrome the platform wants
/// and the per-window setup. See [`kiln_shell::Root`].
pub use kiln_shell::Root;
pub use setup::{AppInfo, Config, application, catalog, init, window_options};

/// The extension traits that put kiln's calls on GPUI's own types:
/// `use kiln::prelude::*;` then `cx.theme()`, `cx.settings::<S>()`,
/// `cx.set_theme_mode(..)`, `cx.binding_label(..)` or
/// `window.run_command(..)`; and [`Command`], for a typed command's ID:
/// `NewNote::id()`.
pub mod prelude {
    pub use crate::ext::{AppExt, WindowExt};
    pub use kiln_platform::command_actions::Command;
    pub use kiln_ui::ActiveTheme;
}

// Commands: `kiln::commands!` declares typed commands (GPUI actions with a
// title and context), and these dispatch and record them.
pub use kiln_platform::command_actions::{
    Command, CommandInfo, current_invocation, dispatch_invocation, register_command,
};
pub use kiln_platform::commands;

/// The app commands, as typed actions: `app.quit` is `app::Quit`. `init`
/// registers them and runs them app-wide; bind them as `app.quit`.
pub mod app {
    kiln_platform::commands!(
        app,
        [
            /// Quit
            Quit,
            /// Hide
            Hide,
            /// Hide Others
            HideOthers,
            /// Show All
            ShowAll,
        ]
    );
}

/// The window commands, as typed actions on the active window:
/// `window.close` is `window::Close`.
pub mod window {
    kiln_platform::commands!(
        window,
        [
            /// Minimize
            Minimize,
            /// Zoom
            Zoom,
            /// Toggle Full Screen
            ToggleFullscreen,
            /// Close Window
            Close,
        ]
    );
}
