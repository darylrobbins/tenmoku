// SPDX-License-Identifier: Apache-2.0

//! The chat as a kiln app: [`chat`] sets it up by kiln's conventions (its
//! files, its window a workspace, its layout saved and restored, kiln's
//! own keys and menus) with what is the chat's: its keymap and menus (one
//! TOML file each, for every platform, naming the commands by their `chat.*`
//! IDs), its table in the user's theme file, and its
//! window's commands, below. Its conversations, panel and status item
//! register themselves.

use kiln::gpui::{prelude::*, *};
use kiln::shell::{Direction::*, DockPosition::Left, DockView as _, PaneView as _, Workspace};
use kiln::ui::ThemeMode::{Dark, Light};
use kiln::{AppBuilder, Config, Cx, WorkspaceExt as _, model, prelude::*, ui::*};

use crate::{conversation::*, threads::*};

pub(crate) const APP_ID: &str = "dev.kiln.Chat";

/// The chat's root key context, where the window's commands apply.
pub(crate) const KEY_CONTEXT: &str = "chat";

pub(crate) fn chat() -> AppBuilder {
    kiln::app(APP_ID, "Kiln Chat")
        .key_context(KEY_CONTEXT)
        .config(config)
        // `[chat]` in the user's theme file: see `bubble_color`.
        .theme_extensions(&[THEME_TABLE])
        .workspace(setup)
}

/// The chat's keymap and menus, over kiln's own keys and menus.
pub(crate) fn config(config: Config) -> Config {
    config
        .keymap("keymap.toml", include_str!("keymap.toml"))
        .menus("menus.toml", include_str!("menus.toml"))
        // The menus show the shortcuts bound in the window's contexts.
        .menu_shortcut_context([KEY_CONTEXT, "conversation"])
}

/// New Conversation
#[kiln::command(id = "chat.new_conversation")]
fn new_conversation(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
    let title = format!("Conversation {}", cx.model::<Threads>().0.len() + 1);
    let ix = Threads::add(&title, &[], cx.cx);
    workspace.open::<Conversation>(ConversationState::new(ix), cx.window, cx.cx);
}

/// Close Conversation
///
/// An empty pane closes itself, unless it's the last; else its active
/// conversation closes, once it agrees.
#[kiln::command(id = "chat.close_conversation")]
fn close_conversation(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
    let pane = workspace.active_pane().clone();
    if pane.read(cx).pane().items().is_empty() {
        workspace.close_pane(&pane, cx.window, cx.cx);
    } else {
        pane.update(cx.cx, |pane, pcx| pane.close_active_item(cx.window, pcx));
    }
}

/// Split Right
#[kiln::command(id = "chat.split_right")]
fn split_right(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
    workspace.split(Right, cx.window, cx.cx);
}

/// Split Down
#[kiln::command(id = "chat.split_down")]
fn split_down(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
    workspace.split(Down, cx.window, cx.cx);
}

/// Toggle Conversations
#[kiln::command(id = "chat.toggle_conversations")]
fn toggle_conversations(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
    workspace.toggle_dock(Left, cx.window, cx.cx);
}

/// Toggle Theme
#[kiln::command(id = "chat.toggle_theme")]
fn toggle_theme(_: &mut Workspace, cx: &mut Cx<Workspace>) {
    let dark = cx.theme().appearance == Appearance::Dark;
    cx.cx.set_theme_mode(if dark { Light } else { Dark });
}

/// Sets up each window, as `kiln::app`'s workspace hook: keeps the
/// conversations in its saved layout, and opens the first unless a
/// restored layout's are coming.
pub(crate) fn setup(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    // Every window's entry holds the whole list: restoring adds what's past it.
    let restore = |saved: Threads, _: &mut Window, cx: &mut App| {
        model::update(cx, |all: &mut Threads, _| {
            all.0.extend(saved.0.into_iter().skip(all.0.len()))
        });
    };
    workspace.persist("threads", Threads::save, restore);
    // Layouts saved before `kiln::app` kept the panel's dock as `dock`.
    let dock = workspace.dock(Left).cloned();
    workspace.on_restore(move |saved, _, cx| {
        for (dock, saved) in dock.iter().zip(saved.get("dock")) {
            dock.update(cx, |dock, cx| dock.dock_mut().restore(saved, cx));
        }
    });
    // Once the restore, if any, has started.
    cx.defer_in(window, |workspace, window, cx| {
        if model::read::<Threads>(cx).0.is_empty() {
            Threads::add("General", &["Welcome to Kiln Chat."], cx);
            Threads::add("Random", &[], cx);
        }
        if !workspace.restoring() {
            workspace.open::<Conversation>(ConversationState::new(0), window, cx);
        }
    });
}
