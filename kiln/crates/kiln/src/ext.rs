// SPDX-License-Identifier: LicenseRef-Proprietary

//! Extension traits that put kiln's calls on GPUI's own `App` and `Window`:
//! `use kiln::prelude::*;`, then `cx.set_theme_mode(..)` or
//! `window.run_command(..)`.

use std::collections::HashMap;

use gpui::{App, Global, SharedString, Subscription, Window, WindowId};
use kiln_command::{CommandId, DispatchError, Invocation};
use kiln_core::{Keymap, Os, macos_label, pc_label};
use kiln_platform::{command_actions::dispatch_invocation, keys};
use kiln_ui::{ThemeMode, observe_system_appearance, set_theme_mode};

/// kiln's calls on GPUI's [`App`] (and so on every `Context`).
pub trait AppExt {
    /// Switches the theme to `mode` over the dark and light pair in use.
    fn set_theme_mode(&mut self, mode: ThemeMode);

    /// The label of the shortcut the keymap binds first to `command`, as
    /// this platform prints it: `⌘]` on macOS, `Ctrl+]` elsewhere. `None`
    /// if nothing binds it.
    fn binding_label(&self, command: &CommandId) -> Option<SharedString>;
}

impl AppExt for App {
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
    /// `kiln::init` can't do this itself.
    fn follow_system_appearance(&mut self, cx: &mut App);
}

impl WindowExt for Window {
    fn run_command(&mut self, invocation: Invocation, cx: &mut App) -> Result<(), DispatchError> {
        dispatch_invocation(invocation, self, cx)
    }

    fn follow_system_appearance(&mut self, cx: &mut App) {
        let id = self.window_handle().window_id();
        if !cx.has_global::<Following>() {
            let closed = cx.on_window_closed(|cx, closed| {
                cx.global_mut::<Following>().windows.remove(&closed);
            });
            cx.set_global(Following {
                windows: HashMap::new(),
                _closed: closed,
            });
        }
        if cx.global::<Following>().windows.contains_key(&id) {
            return;
        }
        let subscription = observe_system_appearance(self);
        cx.global_mut::<Following>()
            .windows
            .insert(id, subscription);
    }
}

/// The windows whose appearance the theme follows.
struct Following {
    windows: HashMap<WindowId, Subscription>,
    _closed: Subscription,
}

impl Global for Following {}
