// SPDX-License-Identifier: LicenseRef-Proprietary

//! The app commands as typed actions: `app.quit` is [`crate::app::Quit`],
//! `window.close` is [`crate::window::Close`], and so on, so an app can
//! bind, dispatch or handle them as it does its own. Each runs what the
//! app's command registry runs for its ID ([`kiln_platform::commands`]),
//! which still serves menus chosen with no window open.

use gpui::App;
use kiln_command::Invocation;
use kiln_platform::{
    command_actions::{Command, current_invocation},
    commands::dispatch_app,
};

use crate::{app, window};

/// Registers the typed app commands and their app-wide handlers. A view
/// may handle one first, with `.on_action(..)`.
pub(crate) fn init(cx: &mut App) {
    app::register(cx);
    window::register(cx);
    cx.on_action(run::<app::Quit>);
    cx.on_action(run::<app::Hide>);
    cx.on_action(run::<app::HideOthers>);
    cx.on_action(run::<app::ShowAll>);
    cx.on_action(run::<window::Minimize>);
    cx.on_action(run::<window::Zoom>);
    cx.on_action(run::<window::ToggleFullscreen>);
    cx.on_action(run::<window::Close>);
}

/// Runs `A` through the app's registry, under the invocation being
/// dispatched if it's `A`'s; the registry tells the observers.
fn run<A: Command>(_: &A, cx: &mut App) {
    let id = A::id();
    let invocation = current_invocation(cx)
        .filter(|invocation| invocation.command == id)
        .unwrap_or_else(|| Invocation::new(id));
    dispatch_app(&invocation, cx);
}
