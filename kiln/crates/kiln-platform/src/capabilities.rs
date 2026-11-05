// SPDX-License-Identifier: Apache-2.0

//! What a platform does natively, as data: which of an app's chrome GPUI
//! draws with the system's own widgets there, and which the app draws.

use gpui::{App, Global};
use kiln_core::Os;

/// What GPUI does natively on a platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub os: Os,
    /// A native menu bar (macOS); elsewhere the app draws its menus in the
    /// window.
    pub native_menus: bool,
    /// Native alerts for `window.prompt` (macOS); elsewhere the app draws
    /// them in the window.
    pub native_prompts: bool,
    /// The unified title bar, with the traffic lights in the app's own
    /// title row (macOS); elsewhere the window manager draws the title bar,
    /// or leaves it to the app (GNOME on Wayland).
    pub unified_title_bar: bool,
}

impl Capabilities {
    pub const fn for_os(os: Os) -> Self {
        let mac = matches!(os, Os::MacOs);
        Self {
            os,
            native_menus: mac,
            native_prompts: mac,
            unified_title_bar: mac,
        }
    }

    /// This platform's.
    pub const fn current() -> Self {
        Self::for_os(Os::current())
    }

    /// The app's: those [`Capabilities::install`] gave it, else this
    /// platform's. Its windows' options and chrome follow them, from the
    /// first window on.
    pub fn of(cx: &App) -> Self {
        cx.try_global::<AppCapabilities>()
            .map_or_else(Self::current, |installed| installed.0)
    }

    /// Has the app draw its windows as on a platform with these
    /// capabilities, rather than this one's ([`Capabilities::of`]): for
    /// tests, and for showing one platform's chrome on another. Windows
    /// opened before keep theirs.
    pub fn install(self, cx: &mut App) {
        cx.set_global(AppCapabilities(self));
    }
}

/// The capabilities [`Capabilities::install`] gave the app.
struct AppCapabilities(Capabilities);

impl Global for AppCapabilities {}
