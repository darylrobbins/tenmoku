// SPDX-License-Identifier: LicenseRef-Proprietary

//! The kiln gallery: every kiln-ui widget on a page of its own, a theme
//! switcher, and kiln's commands, keymap, menus and prompts working
//! together. Run it with `cargo run -p kiln --example gallery`; set
//! `KILN_GALLERY_THEME` to `dark` or `light` to start in that theme.
//!
//! It's written as an app would be: plain GPUI, with kiln's calls where
//! kiln has something to add.

mod app;
mod commands;
mod pages;
mod setup;

use gpui::{App, AppContext as _, Focusable as _};
use kiln::platform::{Capabilities, WindowSpec, window_options};

use crate::app::Gallery;

fn main() {
    gpui_platform::application()
        .with_assets(kiln::ui::Assets)
        .run(|cx: &mut App| {
            setup::init(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let capabilities = Capabilities::current();
            let spec = WindowSpec::main(setup::APP_NAME).app_id("dev.kiln.Gallery");
            cx.open_window(window_options(&spec, &capabilities, cx), |window, cx| {
                let gallery = cx.new(|cx| Gallery::new(window, cx));
                let focus = gallery.focus_handle(cx);
                window.focus(&focus, cx);
                gallery
            })
            .expect("the gallery's window opens");
            cx.activate(true);
        });
}
