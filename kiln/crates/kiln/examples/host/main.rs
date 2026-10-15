// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln Chat: a small chat-like app built from kiln alone, to show
//! kiln-shell end to end. Its window is a `Shell` with panes of
//! conversations that split and take tabs, a dock panel listing the
//! conversations, a status bar, the command palette and the menus. A
//! conversation with unsent text asks before it closes, and the window's
//! layout and conversations are saved when it changes and restored when
//! the app opens again. Run it with `cargo run -p kiln --example host`;
//! set `KILN_CHAT_THEME` to `dark` or `light` to start in that theme.
//!
//! It's written as an app would be: plain GPUI, with kiln's calls where
//! kiln has something to add.

mod app;
mod commands;
mod conversation;
mod panel;
mod setup;
mod threads;

use gpui::{App, AppContext as _};
use kiln::{WindowSpec, platform::Capabilities, shell::layout};

use crate::{app::ChatWindow, conversation::Conversation, threads::Threads};

fn main() {
    kiln::application().run(|cx: &mut App| {
        kiln::init(cx, setup::with_user_files(setup::config()));
        // Saved conversation tabs come back through `PaneItem::restore`.
        layout::register_item::<Conversation>(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        // The window as the chat was last left, if it was saved.
        let launched =
            setup::layouts().and_then(|store| layout::launch(&store, true).into_iter().next());
        let (layout, saved) = match launched {
            Some((layout, saved)) => (Some(layout), saved),
            None => (None, None),
        };
        Threads::load(saved.as_ref().map(|saved| &saved["threads"]), cx);
        let spec = WindowSpec::main(setup::APP_NAME);
        cx.open_window(kiln::window_options(&spec, cx), |window, cx| {
            cx.new(|cx| {
                let mut chat = ChatWindow::new(Capabilities::current(), window, cx);
                chat.start(saved.as_ref(), window, cx);
                if let Some(layout) = layout {
                    chat.set_layout(layout, window, cx);
                }
                chat
            })
        })
        .expect("the chat's window opens");
        cx.activate(true);
    });
}
