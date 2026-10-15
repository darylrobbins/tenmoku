// SPDX-License-Identifier: LicenseRef-Proprietary

//! The chat as a kiln app: [`app`] sets it up by kiln's conventions (its
//! files, its window a workspace, its layout saved and restored, kiln's
//! own keys and menus) with what is the chat's: its commands, keymap and
//! menus (one TOML file each, for every platform), its conversations, the
//! panel listing them, the theme to start in and its table in the user's
//! theme file.

use gpui::AppContext as _;
use kiln::{AppBuilder, Config, shell::DockPosition, ui::ThemeMode};

use crate::conversation::{self, Conversation};
use crate::{app, threads::*};

/// The app's name, for the menus and the window.
pub(crate) const APP_NAME: &str = "Kiln Chat";

pub(crate) const APP_ID: &str = "dev.kiln.Chat";

pub(crate) fn app() -> AppBuilder {
    kiln::app(APP_ID, APP_NAME)
        .key_context(app::KEY_CONTEXT)
        .config(config)
        // `KILN_CHAT_THEME` (`dark`, `light` or `system`), or the system's.
        .theme_mode(match std::env::var("KILN_CHAT_THEME").as_deref() {
            Ok("dark") => ThemeMode::Dark,
            Ok("light") => ThemeMode::Light,
            _ => ThemeMode::System,
        })
        // `[chat]` in the user's theme file: see `conversation::bubble_color`.
        .theme_extensions(&[conversation::THEME_TABLE])
        .item::<Conversation>()
        .panel(DockPosition::Left, |_, cx| cx.new(ConversationList::new))
        .status(Threads::count)
        .workspace(app::setup)
}

/// The chat's commands, keymap and menus, over kiln's own keys and menus.
pub(crate) fn config(config: Config) -> Config {
    config
        .commands(app::init)
        .keymap("keymap.toml", include_str!("keymap.toml"))
        .menus("menus.toml", include_str!("menus.toml"))
        // The menus show the shortcuts bound in the window's contexts.
        .menu_shortcut_context([app::KEY_CONTEXT, conversation::KEY_CONTEXT])
}
