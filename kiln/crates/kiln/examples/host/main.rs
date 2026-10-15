// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln Chat: a small chat-like app built from kiln alone, to show
//! `kiln::app` end to end. Its window is a kiln workspace with panes of
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
mod conversation;
mod setup;
mod threads;

fn main() {
    setup::app().run();
}
