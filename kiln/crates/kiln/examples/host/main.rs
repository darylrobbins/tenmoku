// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln Chat: a small chat-like app built from kiln alone, to show
//! `kiln::app` and its macros end to end. Its window is a kiln workspace
//! with panes of conversations that split and take tabs, a dock panel
//! listing the conversations, a status bar, the command palette and the
//! menus. A conversation with unsent text asks before it closes, and the
//! window's layout and conversations are saved when it changes and restored
//! when the app opens again. Run it with `cargo run -p kiln --example host`.

mod app;
mod conversation;
mod threads;

fn main() {
    app::chat().run();
}
