//! Lens items for assistive technology (T1964): the editor element paints
//! a lens row's items ([`crate::lens`]), so a screen reader would see none.
//! This layer, a child of the editor after its text area, as the line
//! numbers' targets are ([`super::breakpoints`]), puts one node over each
//! item the last frame laid out in view, in the rect its pointer hitbox
//! covers:
//!
//! - an item with an invocation is a [`fe_ui::Role::Button`] named for its
//!   title without a leading decorative glyph ([`lens::accessible_name`]),
//!   whose Click focuses the editor and runs the invocation from the view,
//!   as a pointer click on it does ([`EditorView::run_lens`]);
//! - a plain-text item, such as the test lens's history, is a
//!   [`fe_ui::Role::Label`] with its whole title, so it is read where it
//!   shows, after the buttons before it.
//!
//! The nodes are named in every frame, not only while a screen reader is
//! attached, so tests read them. They take no pointer events (they have no
//! hitbox): a click still reaches [`EditorView::press_lens`] and runs once,
//! with its underline and pointing hand. They take no Tab stop either: the
//! keyboard's way to a lens is the palette's "Code Lens: Run…".

use fe_command::{Invocation, Source};
use gpui::{
    AccessibleAction, AnyElement, App, AvailableSpace, ElementId, InteractiveElement as _,
    IntoElement, ParentElement as _, Styled as _, WeakEntity, Window, div, size,
};

use crate::{EditorView, commands::RunCommand, lens};

impl EditorView {
    /// The id of the accessibility node over the item `index` (from 0) of
    /// the lens row above buffer row `row`.
    #[doc(hidden)]
    pub fn lens_item_id(row: u32, index: usize) -> ElementId {
        ElementId::NamedInteger("lens-item".into(), (u64::from(row) << 32) | index as u64)
    }

    /// The lens items the last frame laid out in view, each a node over
    /// the part of it shown.
    pub(crate) fn layout_lens_targets(
        &mut self,
        this: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let targets = &self.lens_targets;
        let first = targets.first()?;
        let mut area = first.bounds;
        for target in targets {
            area = area.union(&target.bounds);
        }
        let mut children = Vec::with_capacity(targets.len());
        for target in targets {
            let bounds = target.bounds;
            let node = div()
                .id(Self::lens_item_id(target.row, target.index))
                .absolute()
                .left(bounds.left() - area.left())
                .top(bounds.top() - area.top())
                .w(bounds.size.width)
                .h(bounds.size.height);
            let node = match &target.item.invocation {
                Some(invocation) => {
                    let (this, invocation) = (this.clone(), invocation.clone());
                    let node = fe_ui::on_action(
                        node,
                        window,
                        cx,
                        AccessibleAction::Click,
                        move |_, window, cx| {
                            (this.update(cx, |view, cx| {
                                view.activate_lens_item(&invocation, window, cx)
                            }))
                            .ok();
                        },
                    );
                    let name = lens::accessible_name(&target.item.title);
                    fe_ui::node(node, window, cx, fe_ui::Role::Button, name)
                }
                None => {
                    let title = target.item.title.clone();
                    fe_ui::node(node, window, cx, fe_ui::Role::Label, title)
                }
            };
            children.push(node);
        }
        let mut element = div()
            .w(area.size.width)
            .h(area.size.height)
            .children(children)
            .into_any_element();
        let space = size(
            AvailableSpace::Definite(area.size.width),
            AvailableSpace::Definite(area.size.height),
        );
        element.layout_as_root(space, window, cx);
        element.prepaint_at(area.origin, window, cx);
        Some(element)
    }

    /// A screen reader's Click on a lens item: what a pointer click on it
    /// does ([`EditorView::press_lens`], after the press focuses the
    /// editor and ends a composition), so it runs as a click's does. The
    /// carets stay where they are.
    fn activate_lens_item(
        &mut self,
        invocation: &Invocation,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.composition = None;
        self.run_lens(invocation, window, cx);
    }

    /// Runs a lens item's `invocation` from the view once the listener
    /// calling this is done, as a bound click's command: from the pointer
    /// and from a screen reader's Click alike, both as [`Source::Mouse`],
    /// so a command treats the two the same way. A provider's invocation
    /// names no context, so it's given the view's, as a bound click's is
    /// ([`EditorView::context_stack`]): run in the editor's frame alone, a
    /// host's command, such as a merge conflict's `git.accept_incoming`,
    /// isn't found.
    pub(super) fn run_lens(&mut self, invocation: &Invocation, window: &mut Window, cx: &mut App) {
        let mut invocation = invocation.clone().source(Source::Mouse);
        if invocation.context.is_empty()
            && let Some(stack) = self.context_stack(window, cx)
        {
            invocation = invocation.context(stack);
        }
        let (action, focus) = (RunCommand(invocation), self.focus.clone());
        window.defer(cx, move |window, cx| {
            focus.dispatch_action(&action, window, cx)
        });
    }
}
