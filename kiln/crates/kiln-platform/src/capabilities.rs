// SPDX-License-Identifier: LicenseRef-Proprietary

//! What a platform does natively, as data: which of an app's chrome GPUI
//! draws with the system's own widgets there, and which the app draws.

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
}
