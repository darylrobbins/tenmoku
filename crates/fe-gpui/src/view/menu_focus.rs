//! A popup menu's own focus, while it's open (T1897): the editor's
//! right-click menu takes it, and a view's other menus can.
//!
//! - **Its own handle.** Opening focuses a new [`FocusHandle`] that the
//!   menu's element tracks, with a key context of its own carrying
//!   kiln's [`HOLDS_KEYS`] ([`key_context`]): no binding runs for a key
//!   without the platform modifier, so every such key reaches the menu's
//!   `on_key_down`, and none the owner's. The owner's handle isn't focused,
//!   so the owner registers no input handler, and the menu registers none:
//!   an input method can neither commit into the owner nor take the arrows.
//! - **Giving it back.** Closing hands focus back to the owner if the menu
//!   still has it ([`MenuFocus::give_back`]): Escape, choosing an item, a
//!   press outside, a stale menu. A close without a window does it at the
//!   owner's next render ([`MenuFocus::give_back_later`]). Focus that left
//!   for elsewhere (another view, another window) stays where it went.
//! - **Losing it.** Focus leaving the menu for anything else, or the
//!   menu's element no longer being drawn, runs the opener's callback,
//!   which closes the menu. GPUI tells focus changes as a frame is drawn in
//!   an active window; a window going inactive is one.

use gpui::{App, Context, FocusHandle, KeyContext, Subscription, Window};

/// The key context of a menu named `name` while it has focus: its name,
/// and kiln's [`HOLDS_KEYS`].
///
/// [`HOLDS_KEYS`]: kiln_platform::keys::HOLDS_KEYS
pub(crate) fn key_context(name: &'static str) -> KeyContext {
    let mut context = KeyContext::default();
    context.add(name);
    context.add(kiln_platform::keys::HOLDS_KEYS);
    context
}

/// An open menu's focus: its own handle, and the owner's to give back.
pub(crate) struct MenuFocus {
    handle: FocusHandle,
    owner: FocusHandle,
    /// Runs the opener's callback once focus leaves the handle.
    _left: Subscription,
}

impl MenuFocus {
    /// Focuses a new handle for a menu `owner` opened, calling `left` on
    /// the view once focus leaves it: see the [module](self).
    pub(crate) fn take<V: 'static>(
        owner: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<V>,
        mut left: impl FnMut(&mut V, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        let handle = cx.focus_handle();
        let left = cx.on_focus_out(&handle, window, move |view, _, window, cx| {
            left(view, window, cx)
        });
        window.focus(&handle, cx);
        Self {
            handle,
            owner: owner.clone(),
            _left: left,
        }
    }

    /// The handle the menu's element tracks.
    pub(crate) fn handle(&self) -> &FocusHandle {
        &self.handle
    }

    /// Hands focus back to the owner, if the menu still has it.
    pub(crate) fn give_back(self, window: &mut Window, cx: &mut App) {
        if self.handle.is_focused(window) {
            window.focus(&self.owner, cx);
        }
    }

    /// For a close without a window: what gives focus back at the owner's
    /// next render.
    pub(crate) fn give_back_later(self) -> FocusReturn {
        FocusReturn {
            handle: self.handle,
            owner: self.owner,
        }
    }
}

/// Focus a closed menu gives back at its owner's next render, if it still
/// has it.
pub(crate) struct FocusReturn {
    handle: FocusHandle,
    owner: FocusHandle,
}

impl FocusReturn {
    pub(crate) fn finish(self, window: &mut Window, cx: &mut App) {
        if self.handle.is_focused(window) {
            window.focus(&self.owner, cx);
        }
    }
}
