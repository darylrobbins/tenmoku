// SPDX-License-Identifier: Apache-2.0

//! Focus on an element that's no longer drawn, such as one in a page that
//! was hidden. GPUI keeps it focused, but its keys and actions then reach
//! nothing: GPUI dispatches through the frame last drawn, from the focused
//! element's node or, without one, from the frame's root node, which is
//! the window's root view's element itself, not the element its `render`
//! returns, so even the root view's own listeners miss them.
//!
//! Kiln handles it twice over. [`watch`] has a window move focus, once a
//! frame is drawn without the focused element, to its nearest ancestor
//! that's still drawn and focusable (GPUI's
//! [`Window::focus_lost_restore_target`]), so the next key reaches the
//! view around it as it would have before. Kiln's key input and command
//! dispatch watch each window they see; a window's builder can call
//! [`watch`] itself to cover input that came before either. And
//! [`is_drawn`] lets dispatch notice focus left on an undrawn element
//! before that frame, or with no ancestor to move to; [`is_drawn_for`]
//! answers the same, most often without its probe.

use std::collections::HashMap;

use gpui::{
    Action, App, AppContext as _, Entity, FocusHandle, Global, IntoElement, Render, Subscription,
    Window, WindowId,
};

/// Has `window` move focus off an element that stops being drawn, to its
/// nearest drawn focusable ancestor; with none, focus stays where it was.
/// Watching a window twice changes nothing; the watch ends when the window
/// closes. Moves of focus by the app, before the frame is drawn, win: the
/// watch acts only when a drawn frame has no focused element in it.
pub fn watch(window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if cx
        .try_global::<Watched>()
        .is_some_and(|watched| watched.windows.contains_key(&id))
    {
        return;
    }
    if !cx.has_global::<Watched>() {
        let closed = cx.on_window_closed(|cx, closed| {
            cx.global_mut::<Watched>().windows.remove(&closed);
        });
        cx.set_global(Watched {
            windows: HashMap::new(),
            _closed: closed,
        });
    }
    let keeper = cx.new(|_| Keeper);
    let lost = keeper.update(cx, |_, cx| {
        cx.on_focus_lost(window, |_, window, cx| {
            if let Some(target) = window.focus_lost_restore_target(cx) {
                window.focus(&target, cx);
            }
        })
    });
    cx.global_mut::<Watched>()
        .windows
        .insert(id, (keeper, lost));
}

/// Whether `focus`'s element was drawn in `window`'s last frame. GPUI
/// 0.3.7 doesn't say (its dispatch tree is private), but
/// [`FocusHandle::dispatch_action`] does nothing at all for a handle that
/// wasn't drawn, while for one that was it ends at the global listeners,
/// if no element stopped it: so a probe action that no element handles
/// reaches its global listener only then. Propagation is left on, as
/// before any dispatch.
pub fn is_drawn(focus: &FocusHandle, window: &mut Window, cx: &mut App) -> bool {
    if !cx.default_global::<Probe>().installed {
        cx.global_mut::<Probe>().installed = true;
        cx.on_action(|_: &FocusProbe, cx| cx.global_mut::<Probe>().reached = true);
    }
    cx.global_mut::<Probe>().reached = false;
    focus.dispatch_action(&FocusProbe, window, cx);
    std::mem::take(&mut cx.global_mut::<Probe>().reached)
}

/// Whether `focus`'s element was drawn in `window`'s last frame, as
/// [`is_drawn`] says, for dispatching `action` to it: cheaper when an
/// element on its path handles `action`, as one usually does.
///
/// GPUI looks up a focus handle that wasn't drawn as the frame's root, so
/// a handle that was never drawn gives the root's path alone. If `action`
/// has a listener on `focus`'s path and not on the root's, the two paths
/// differ, so `focus` was drawn: that's two walks of the dispatch tree,
/// with no dispatch. Otherwise, or before a window's first frame, which
/// has no root yet, [`is_drawn`]'s probe decides.
pub fn is_drawn_for(
    focus: &FocusHandle,
    action: &dyn Action,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    // A window's first frame is drawn as soon as its root view is set.
    if window.root::<Keeper>().is_some() {
        if cx
            .try_global::<Probe>()
            .is_none_or(|probe| probe.undrawn.is_none())
        {
            let undrawn = cx.focus_handle();
            cx.default_global::<Probe>().undrawn = Some(undrawn);
        }
        let undrawn = cx.global::<Probe>().undrawn.as_ref();
        if undrawn.is_some_and(|undrawn| {
            window.is_action_available_in(action, focus)
                && !window.is_action_available_in(action, undrawn)
        }) {
            return true;
        }
    }
    is_drawn(focus, window, cx)
}

/// The entity a window's focus-lost listener belongs to: GPUI registers
/// one only for a view. A view only so [`is_drawn_for`] can ask whether a
/// window has a root: no window's root is one.
struct Keeper;

impl Render for Keeper {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// The windows [`watch`]ed, each with its listener.
struct Watched {
    windows: HashMap<WindowId, (Entity<Keeper>, Subscription)>,
    _closed: Subscription,
}

impl Global for Watched {}

/// Sent to a focused element to learn whether it was drawn: a no-op
/// action, which only [`is_drawn`]'s global listener hears.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = kiln, no_json, no_register)]
struct FocusProbe;

/// Whether [`FocusProbe`] reached its global listener, and whether that
/// listener is installed; and a focus handle no element tracks, for
/// [`is_drawn_for`].
#[derive(Default)]
struct Probe {
    installed: bool,
    reached: bool,
    undrawn: Option<FocusHandle>,
}

impl Global for Probe {}
