// SPDX-License-Identifier: LicenseRef-Proprietary

//! The kiln gallery: every kiln-ui widget on a page of its own, a theme
//! switcher, and kiln's commands, keymap, menus and prompts working
//! together. Run it with `cargo run -p kiln --example gallery`; set
//! `KILN_GALLERY_THEME` to `dark` or `light` to start in that theme. The
//! user's `settings.toml` (its `title`), `keymap.toml` and `theme.toml`
//! apply as they're saved: see `setup::with_user_files`.
//!
//! It's written as an app would be: plain GPUI, with kiln's calls where
//! kiln has something to add.

mod app;
mod commands;
mod pages;
mod settings;
mod setup;

use gpui::{App, AppContext as _, Focusable as _};
use kiln::{WindowSpec, prelude::*};

use crate::app::Gallery;

fn main() {
    kiln::application().run(|cx: &mut App| {
        kiln::init(cx, setup::with_user_files(setup::config()));
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let spec = WindowSpec::main(setup::APP_NAME);
        cx.open_window(kiln::window_options(&spec, cx), |window, cx| {
            window.follow_system_appearance(cx);
            let gallery = cx.new(|cx| Gallery::new(window, cx));
            let focus = gallery.focus_handle(cx);
            window.focus(&focus, cx);
            gallery
        })
        .expect("the gallery's window opens");
        cx.activate(true);
    });
}
