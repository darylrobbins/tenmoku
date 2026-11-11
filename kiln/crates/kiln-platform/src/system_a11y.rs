// SPDX-License-Identifier: Apache-2.0

//! The system's accessibility display options, as a global an app follows.
//!
//! [`SystemA11y`] is what the system says now. [`install`] reads it, sets
//! the global and, where the platform can say when it changes, keeps it
//! current; [`system_a11y`] reads the global and [`observe_system_a11y`]
//! watches it. Nothing is applied here: the app decides what each flag
//! changes.
//!
//! On macOS the reader is `NSWorkspace`'s `accessibilityDisplayShould…`
//! options, read at [`install`] and again on every
//! `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`. Elsewhere
//! every flag is off and never changes.

use gpui::{App, Global, Subscription};

#[cfg(target_os = "macos")]
mod macos;

/// The system's accessibility display options. All off by default, and
/// wherever the platform has no reader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemA11y {
    /// Reduce Motion.
    pub reduce_motion: bool,
    /// Increase Contrast.
    pub increase_contrast: bool,
    /// Reduce Transparency. Read and exposed; the app has nothing
    /// translucent to change.
    pub reduce_transparency: bool,
    /// Differentiate Without Colour.
    pub differentiate_without_color: bool,
}

impl Global for SystemA11y {}

/// The system's options as the platform reads them now, without touching
/// the global.
pub fn read() -> SystemA11y {
    #[cfg(target_os = "macos")]
    {
        macos::read()
    }
    #[cfg(not(target_os = "macos"))]
    {
        SystemA11y::default()
    }
}

/// The options the app last saw: the defaults before [`install`].
pub fn system_a11y(cx: &App) -> SystemA11y {
    cx.try_global::<SystemA11y>().copied().unwrap_or_default()
}

/// Calls `f` each time the options change, not when a reading is the same.
pub fn observe_system_a11y(cx: &mut App, mut f: impl FnMut(&mut App) + 'static) -> Subscription {
    cx.observe_global::<SystemA11y>(move |cx| f(cx))
}

/// Sets the options, notifying observers only if they differ.
pub fn set_system_a11y(cx: &mut App, value: SystemA11y) {
    if cx.try_global::<SystemA11y>() != Some(&value) {
        cx.set_global(value);
    }
}

/// Reads the options now, sets the global, and on macOS keeps it current
/// for as long as the app runs. Call it once, on the main thread, at
/// startup.
pub fn install(cx: &mut App) {
    set_system_a11y(cx, read());
    #[cfg(target_os = "macos")]
    macos::watch(cx);
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn observers_hear_changes_only(cx: &mut TestAppContext) {
        let heard = Rc::new(Cell::new(0));
        cx.update(|cx| {
            assert_eq!(system_a11y(cx), SystemA11y::default());
            let heard = heard.clone();
            observe_system_a11y(cx, move |_| heard.set(heard.get() + 1)).detach();
            let on = SystemA11y {
                reduce_motion: true,
                ..Default::default()
            };
            set_system_a11y(cx, on);
            assert_eq!(system_a11y(cx), on);
        });
        // The same reading again says nothing; a different one does.
        cx.update(|cx| set_system_a11y(cx, system_a11y(cx)));
        cx.update(|cx| set_system_a11y(cx, SystemA11y::default()));
        assert_eq!(heard.get(), 2);
    }
}
