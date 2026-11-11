// SPDX-License-Identifier: Apache-2.0

//! The accessibility kit: [`node`] gives an element a role and a label (and
//! its [`States`]), and the [`Announcer`] is the window's live region.
//!
//! Under `cfg(any(test, feature = "test-support"))` every [`node`] call is
//! recorded per window, as `(element id, Role, label, states)`, and
//! [`nodes`] reads the list back. A test window never has a screen reader
//! attached, so GPUI builds no tree there; the recorder is what chrome tests
//! assert instead. A dependent crate's tests don't see this crate's
//! `cfg(test)`, hence the `test-support` feature.

use gpui::{
    AnyElement, AnyWindowHandle, App, Context, InteractiveElement as _, IntoElement, Render,
    SharedString, StatefulInteractiveElement, Styled as _, WeakEntity, Window, accesskit, div,
};

#[cfg(any(test, feature = "test-support"))]
use gpui::ElementId;
pub use gpui::accesskit::Role;

/// How urgently a screen reader speaks an [`announce`]ment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Politeness {
    /// After what it is saying now.
    #[default]
    Polite,
    /// At once, for errors.
    Assertive,
}

impl Politeness {
    fn live(self) -> accesskit::Live {
        match self {
            Self::Polite => accesskit::Live::Polite,
            Self::Assertive => accesskit::Live::Assertive,
        }
    }
}

/// What an element says about itself besides its role and label.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct States {
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub toggled: Option<bool>,
    /// A live region's message.
    pub value: Option<SharedString>,
    /// A live region's politeness.
    pub live: Option<Politeness>,
}

/// One recorded node.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: ElementId,
    pub role: Role,
    pub label: SharedString,
    pub states: States,
}

/// `element` with `role` and `label`. The element needs an id for GPUI to
/// make a node of it.
pub fn node<E: StatefulInteractiveElement + gpui::Element>(
    element: E,
    window: &Window,
    cx: &mut App,
    role: Role,
    label: impl Into<SharedString>,
) -> E {
    node_with(element, window, cx, role, label, States::default())
}

/// [`node`], with `states` too.
pub fn node_with<E: StatefulInteractiveElement + gpui::Element>(
    element: E,
    window: &Window,
    cx: &mut App,
    role: Role,
    label: impl Into<SharedString>,
    states: States,
) -> E {
    let label = label.into();
    #[cfg(any(test, feature = "test-support"))]
    if let Some(id) = gpui::Element::id(&element) {
        recorder::record(
            window,
            Node {
                id,
                role,
                label: label.clone(),
                states: states.clone(),
            },
            cx,
        );
    }
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = (window, &cx);
    let mut element = element.role(role).aria_label(label);
    if let Some(selected) = states.selected {
        element = element.aria_selected(selected);
    }
    if let Some(expanded) = states.expanded {
        element = element.aria_expanded(expanded);
    }
    if let Some(toggled) = states.toggled {
        element = element.aria_toggled(toggled.into());
    }
    if let Some(value) = states.value {
        element = element.aria_value(value);
    }
    match states.live {
        Some(politeness) => element.a11y_synthetic_children(move |builder| {
            builder.parent_node().set_live(politeness.live());
        }),
        None => element,
    }
}

/// The nodes [`node`] recorded for `window`, in draw order, one per element
/// id: a later draw replaces an earlier node.
#[cfg(any(test, feature = "test-support"))]
pub fn nodes(window: &Window, cx: &App) -> Vec<Node> {
    recorder::of(window.window_handle().window_id(), cx)
}

/// Forgets what was recorded, for a test that wants a fresh list.
#[cfg(any(test, feature = "test-support"))]
pub fn clear_nodes(cx: &mut App) {
    recorder::clear(cx);
}

#[cfg(any(test, feature = "test-support"))]
mod recorder {
    use std::{cell::Cell, collections::HashMap, rc::Rc};

    use gpui::{App, EntityId, Global, Window, WindowId};

    use super::Node;

    /// One window's nodes: those of the draw in progress or the last one.
    #[derive(Default)]
    struct Draw {
        /// Each node with the view that drew it: the same local id under
        /// two views is two nodes.
        nodes: Vec<(EntityId, Node)>,
        /// Set once the draw ends, so the next draw's first node starts a
        /// new list rather than adding to this one.
        finished: Rc<Cell<bool>>,
        watching: bool,
    }

    #[derive(Default)]
    struct Recorded(HashMap<WindowId, Draw>);

    impl Global for Recorded {}

    pub(super) fn record(window: &Window, node: Node, cx: &mut App) {
        let id = window.window_handle().window_id();
        let view = window.current_view();
        let draw = cx.default_global::<Recorded>().0.entry(id).or_default();
        if draw.finished.replace(false) {
            draw.nodes.clear();
        }
        match draw
            .nodes
            .iter_mut()
            .find(|(v, n)| *v == view && n.id == node.id)
        {
            Some(slot) => slot.1 = node,
            None => draw.nodes.push((view, node)),
        }
        if !draw.watching {
            draw.watching = true;
            let finished = draw.finished.clone();
            // After this frame, whatever draws next is a new list. The
            // callback re-arms only when a node records again.
            window.defer(cx, move |_, cx| {
                finished.set(true);
                if let Some(draw) = cx.default_global::<Recorded>().0.get_mut(&id) {
                    draw.watching = false;
                }
            });
        }
    }

    pub(super) fn of(window: WindowId, cx: &App) -> Vec<Node> {
        cx.try_global::<Recorded>()
            .and_then(|r| r.0.get(&window))
            .map(|d| d.nodes.iter().map(|(_, n)| n.clone()).collect())
            .unwrap_or_default()
    }

    pub(super) fn clear(cx: &mut App) {
        cx.default_global::<Recorded>().0.clear();
    }
}

/// Every window's announcer, for [`announce`] to find.
#[derive(Default)]
struct Announcers(Vec<(AnyWindowHandle, WeakEntity<Announcer>)>);

impl gpui::Global for Announcers {}

/// Whether a screen reader is listening to windows, as a test sets it:
/// a test window never has one attached.
#[cfg(any(test, feature = "test-support"))]
struct ListeningForTests(bool);

#[cfg(any(test, feature = "test-support"))]
impl gpui::Global for ListeningForTests {}

/// For tests: says whether a screen reader is listening to every window
/// (it is, until a test says otherwise).
#[cfg(any(test, feature = "test-support"))]
pub fn set_listening_for_tests(cx: &mut App, listening: bool) {
    cx.set_global(ListeningForTests(listening));
}

/// Whether `window` has a screen reader listening right now, read from the
/// window itself, never remembered from an earlier draw.
fn listening(window: &Window, cx: &App) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    {
        let _ = window;
        cx.try_global::<ListeningForTests>().is_none_or(|l| l.0)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        let _ = cx;
        window.is_a11y_active()
    }
}

/// The window's live region: a zero-size `Status` node whose live child
/// carries the latest [`announce`]ment. Put one last in the window's root
/// with [`Announcer::mount`].
pub struct Announcer {
    message: SharedString,
    politeness: Politeness,
    /// A new element id per message, so the same text twice is a node the
    /// platform sees added, and speaks.
    sequence: u64,
}

impl Announcer {
    /// This window's announcer, as an element to put in its root.
    pub fn mount(window: &mut Window, cx: &mut App) -> AnyElement {
        window
            .use_keyed_state("kiln-a11y-announcer", cx, |window, cx| {
                let handle = window.window_handle();
                let announcer = cx.weak_entity();
                cx.default_global::<Announcers>()
                    .0
                    .push((handle, announcer));
                Self {
                    message: SharedString::default(),
                    politeness: Politeness::Polite,
                    sequence: 0,
                }
            })
            .into_any_element()
    }

    fn set(&mut self, text: SharedString, politeness: Politeness, cx: &mut Context<Self>) {
        self.message = text;
        self.politeness = politeness;
        self.sequence += 1;
        cx.notify();
    }
}

impl Render for Announcer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A message that outlived the listener is dropped, not kept for the
        // next activation.
        if !listening(window, cx) {
            self.message = SharedString::default();
        }
        let message = Some(self.message.clone()).filter(|m| !m.is_empty());
        let states = States {
            value: message.clone(),
            live: message.as_ref().map(|_| self.politeness),
            ..States::default()
        };
        node_with(
            div()
                .id(("kiln-a11y-announcer", self.sequence as usize))
                .absolute()
                .size_0(),
            window,
            cx,
            Role::Status,
            "",
            states,
        )
    }
}

/// Says `text` to a screen reader, from any view: in the active window, or
/// in every window if none has an announcer. One message per frame is
/// spoken, the last one made. A window with no screen reader listening
/// drops it.
pub fn announce(cx: &mut App, text: impl Into<SharedString>, politeness: Politeness) {
    let text = text.into();
    let active = cx.active_window().map(|w| w.window_id());
    let mut all = cx.default_global::<Announcers>().0.clone();
    all.retain(|(_, weak)| weak.upgrade().is_some());
    cx.default_global::<Announcers>().0 = all.clone();
    let any_active = all.iter().any(|(h, _)| Some(h.window_id()) == active);
    for (handle, weak) in all {
        if !any_active || Some(handle.window_id()) == active {
            deliver(cx, handle, weak, text.clone(), politeness, true);
        }
    }
}

/// Hands `text` to one window's announcer if that window is listening now.
/// A window that is mid-update (a view announcing from its own render)
/// can't be asked: the check waits until the update ends, then asks the
/// live window.
fn deliver(
    cx: &mut App,
    handle: AnyWindowHandle,
    weak: WeakEntity<Announcer>,
    text: SharedString,
    politeness: Politeness,
    may_wait: bool,
) {
    match handle.update(cx, |_, window, cx| listening(window, cx)) {
        Ok(true) => {
            if let Some(entity) = weak.upgrade() {
                entity.update(cx, |announcer, cx| announcer.set(text, politeness, cx));
            }
        }
        Ok(false) => {}
        Err(_) if may_wait => {
            cx.defer(move |cx| deliver(cx, handle, weak, text, politeness, false))
        }
        Err(_) => {}
    }
}
