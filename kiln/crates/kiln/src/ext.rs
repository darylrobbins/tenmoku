// SPDX-License-Identifier: Apache-2.0

//! Extension traits that put kiln's calls on GPUI's own `App` and `Window`:
//! `use kiln::prelude::*;`, then `cx.settings::<S>()`,
//! `cx.set_theme_mode(..)` or `window.run_command(..)`.

use std::sync::Arc;

use gpui::{App, SharedString, Window};
use kiln_command::{CommandId, DispatchError, Invocation};
use kiln_core::{Keymap, Os, macos_label, pc_label};
use kiln_platform::{command_actions::dispatch_invocation, config, keys};
use kiln_ui::{ThemeMode, set_theme_mode};

/// kiln's calls on GPUI's [`App`] (and so on every `Context`).
pub trait AppExt {
    /// The app's settings of type `S`, as [`crate::Config::settings`]
    /// loaded them and kiln keeps them as their files change:
    /// `S::default()` if the config has none. Follow changes with
    /// `cx.observe_global::<GlobalSettings<S>>(..)`; windows redraw on
    /// each, so reading them while rendering is enough.
    fn settings<S: Default + 'static>(&self) -> Arc<S>;

    /// Switches the theme to `mode` over the dark and light pair in use.
    fn set_theme_mode(&mut self, mode: ThemeMode);

    /// The label of the shortcut the keymap binds first to `command`, as
    /// this platform prints it: `⌘]` on macOS, `Ctrl+]` elsewhere. `None`
    /// if nothing binds it.
    fn binding_label(&self, command: &CommandId) -> Option<SharedString>;
}

impl AppExt for App {
    fn settings<S: Default + 'static>(&self) -> Arc<S> {
        config::settings::<S>(self)
    }

    fn set_theme_mode(&mut self, mode: ThemeMode) {
        set_theme_mode(self, mode);
    }

    fn binding_label(&self, command: &CommandId) -> Option<SharedString> {
        binding_label_for(&keys::current_keymap(self), command, Os::current())
    }
}

/// The label of the shortcut `keymap` binds first to `command`, as `os`
/// prints it: [`AppExt::binding_label`] for any keymap and platform.
pub fn binding_label_for(keymap: &Keymap, command: &CommandId, os: Os) -> Option<SharedString> {
    let binding = keymap.bindings_for(command).into_iter().next()?;
    let label = match os {
        Os::MacOs => macos_label(&binding.keys),
        _ => pc_label(&binding.keys),
    };
    Some(label.into())
}

/// kiln's calls on GPUI's [`Window`].
pub trait WindowExt {
    /// Dispatches `invocation` from the focused element, as a key or menu
    /// item bound to it would (see [`dispatch_invocation`]).
    fn run_command(&mut self, invocation: Invocation, cx: &mut App) -> Result<(), DispatchError>;

    /// Keeps the theme following the system's dark or light appearance, as
    /// this window sees it, while the mode is [`ThemeMode::System`]; kiln
    /// holds the subscription until the window closes. Calling it again
    /// for the same window does nothing more. GPUI tells only windows of
    /// appearance changes, and has no hook for a window opening, so
    /// `kiln::init` can't do this itself; [`crate::Root`] does, as it's made.
    fn follow_system_appearance(&mut self, cx: &mut App);
}

impl WindowExt for Window {
    fn run_command(&mut self, invocation: Invocation, cx: &mut App) -> Result<(), DispatchError> {
        dispatch_invocation(invocation, self, cx)
    }

    fn follow_system_appearance(&mut self, cx: &mut App) {
        kiln_ui::follow_system_appearance(self, cx);
    }
}
