// SPDX-License-Identifier: Apache-2.0

//! The modal layer: one modal view at a time over the window, such as a
//! go-to-file picker or a command palette, named by a [`ModalId`].
//!
//! A modal is a [`ModalView`]: a GPUI `ManagedView` (focusable, emitting
//! `DismissEvent`) that can be dismissed from outside, as Escape would. The
//! layer hides it on its `DismissEvent`, or when focus leaves it (another
//! element is clicked, or the window deactivates), dismissing it then. If
//! a modal still has focus as it's hidden, focus goes to the layer's
//! fallback, else back to where it was before the modal showed.
//!
//! ```ignore
//! layer.update(cx, |layer, cx| layer.show_modal(GO_TO_FILE, picker, window, cx));
//! ```

use std::{collections::HashMap, rc::Rc};

use gpui::{
    AnyView, App, Context, DismissEvent, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable as _, IntoElement, ManagedView, ParentElement as _, Render, Styled as _,
    Subscription, Window, div,
};

use crate::{Picker, PickerDelegate};

/// A modal's identity: `"quick_open"`, `"command_palette"`, …
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModalId(pub &'static str);

/// A view the modal layer can show.
pub trait ModalView: ManagedView {
    /// Dismisses it as Escape would. It emits `DismissEvent`, as the default
    /// does.
    fn dismiss(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl<D: PickerDelegate> ModalView for Picker<D> {
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Picker::dismiss(self, window, cx);
    }
}

/// What the layer tells its host as modals come and go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalEvent {
    Shown(ModalId),
    /// Hidden: dismissed, picked from, or left for another element.
    Dismissed(ModalId),
}

/// Where focus goes when a modal that has it hides.
pub type FocusFallback = Rc<dyn Fn(&mut Window, &mut App) -> Option<FocusHandle>>;

/// Dismisses a modal, as Escape would.
type Dismiss = Rc<dyn Fn(&mut Window, &mut App)>;

struct Active {
    id: ModalId,
    entity: EntityId,
    view: AnyView,
    dismiss: Dismiss,
}

/// The modal showing, if any, drawn over its host.
#[derive(Default)]
pub struct ModalLayer {
    active: Option<Active>,
    /// Per modal shown: its dismissal and its blur.
    subscriptions: HashMap<EntityId, [Subscription; 2]>,
    /// Focused before each modal showed, by its entity.
    previous: HashMap<EntityId, FocusHandle>,
    fallback: Option<FocusFallback>,
}

impl EventEmitter<ModalEvent> for ModalLayer {}

impl ModalLayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where focus goes when a modal that has it hides, rather than back
    /// to where it was before it showed: the host's main view, say. `None`
    /// from it falls back to where focus was.
    pub fn set_focus_fallback(&mut self, fallback: FocusFallback) {
        self.fallback = Some(fallback);
    }

    /// The modal showing, if any.
    pub fn modal(&self) -> Option<ModalId> {
        self.active.as_ref().map(|active| active.id)
    }

    /// The view of the modal showing, if any.
    pub fn modal_view(&self) -> Option<&AnyView> {
        self.active.as_ref().map(|active| &active.view)
    }

    /// Shows `modal` as `id`, dismissing the one showing unless it's the
    /// same entity, and focuses it unless focus is in it already. It hides
    /// on its `DismissEvent` or when focus leaves it.
    pub fn show_modal<M: ModalView>(
        &mut self,
        id: ModalId,
        modal: Entity<M>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity = modal.entity_id();
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.entity != entity)
        {
            self.dismiss_modal(window, cx);
        }
        let focus = modal.focus_handle(cx);
        if !focus.contains_focused(window, cx) {
            if let Some(previous) = window.focused(cx) {
                self.previous.insert(entity, previous);
            }
            window.focus(&focus, cx);
        }
        let dismissed = cx.subscribe_in(
            &modal,
            window,
            move |layer, modal, _: &DismissEvent, window, cx| {
                layer.dismissed(modal.entity_id(), modal.focus_handle(cx), window, cx)
            },
        );
        let weak = modal.downgrade();
        let left = cx.on_blur(&focus, window, move |layer, window, cx| {
            if layer.active.as_ref().map(|active| active.entity) != Some(entity) {
                return;
            }
            if let Some(modal) = weak.upgrade()
                && modal.focus_handle(cx).is_focused(window)
            {
                // The window deactivated: GPUI blurs then too, focus
                // staying on the modal.
                modal.update(cx, |modal, cx| modal.dismiss(window, cx));
            }
            layer.hide(cx);
        });
        self.subscriptions.insert(entity, [dismissed, left]);
        let weak = modal.downgrade();
        self.active = Some(Active {
            id,
            entity,
            view: modal.into(),
            dismiss: Rc::new(move |window, cx| {
                if let Some(modal) = weak.upgrade() {
                    modal.update(cx, |modal, cx| modal.dismiss(window, cx));
                }
            }),
        });
        cx.emit(ModalEvent::Shown(id));
        cx.notify();
    }

    /// Dismisses the modal showing, as Escape would.
    pub fn dismiss_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active.as_ref() else {
            return;
        };
        // Held while it's dismissed: the layer may have been all that
        // kept it.
        let (dismiss, _view) = (active.dismiss.clone(), active.view.clone());
        self.hide(cx);
        dismiss(window, cx);
    }

    /// Hides the modal showing without dismissing it.
    fn hide(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = self.active.take() {
            cx.emit(ModalEvent::Dismissed(active.id));
            cx.notify();
        }
    }

    /// A modal emitted `DismissEvent`: focus leaves it, if it has it, and
    /// it hides if it's the one showing.
    fn dismissed(
        &mut self,
        entity: EntityId,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self.previous.remove(&entity);
        if focus.contains_focused(window, cx) {
            let fallback = self.fallback.clone();
            let to = fallback
                .and_then(|fallback| fallback(window, cx))
                .or(previous);
            if let Some(to) = to {
                window.focus(&to, cx);
            }
        }
        if self.active.as_ref().map(|active| active.entity) == Some(entity) {
            self.hide(cx);
        }
    }
}

impl Render for ModalLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The modal positions itself, over the layer's host.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .children(self.modal_view().cloned())
    }
}
