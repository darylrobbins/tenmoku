// SPDX-License-Identifier: Apache-2.0

//! A watchdog for a window whose frame requests stop arriving:
//! [`watch_frame_requests`]. It's macOS's; elsewhere it's compiled out.

use gpui::{App, Task, Window};

/// Watches that `window` keeps getting frame requests, and restarts its
/// display link when they stop: a workaround for a macOS window that
/// stops drawing until it's deactivated and activated again
/// (docs/decisions/h9n3r1vpgs-freeze.md, "Card 2").
///
/// While the window is visible or key, a probe posted with
/// [`Window::on_next_frame`] that goes unacknowledged for 500 ms, across
/// two polls 250 ms apart, is a stall. The watchdog then has AppKit redraw
/// the window's view (`setNeedsDisplay:`, so GPUI's `displayLayer:`),
/// which subscribes it to its display link again, at most twice a second
/// and every 2 s after 8 tries. It logs one warning when a stall begins
/// and one when frame requests resume.
///
/// kiln-shell's `Root` calls it for its window. A host whose windows aren't `Root`s
/// calls it once per window as the window opens, and keeps the task, or
/// detaches it: the watch ends when the task is dropped or the window
/// closes.
///
/// `None` on other platforms, where the watchdog is compiled out, and for
/// a window with no AppKit view, as on GPUI's test platform.
pub fn watch_frame_requests(window: &mut Window, cx: &mut App) -> Option<Task<()>> {
    #[cfg(target_os = "macos")]
    return macos::watch(window, cx);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, cx);
        None
    }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(test, target_os = "macos"))]
mod tests;
