// SPDX-License-Identifier: Apache-2.0

//! A view container: a dock panel whose views an app registers by id,
//! each under a collapsible header in the panel's [`ViewStack`], as an
//! explorer panel holds a project tree over an outline. The views go top
//! to bottom by their [`ViewSpec::order`]; a view added with an id that's
//! there replaces it.
//!
//! The container saves its views' collapse and weight with the dock's
//! state ([`DockItem::save_state`]), so a saved layout holds them, under
//! the panel's id, as [`ViewStack::save`] gives them: `{"views": {..}}`.
//! A saved view the container doesn't have is ignored.
//!
//! Its lead view ([`ViewContainer::lead`]), or its only one, has no header:
//! the dock's names it, and carries its actions
//! ([`ViewContainer::set_actions`], [`DockItem::actions`]). A view's
//! [`ViewContainer::set_detail`] shows in its header while it's collapsed.
//!
//! ```ignore
//! let explorer = cx.new(|cx| {
//!     let mut explorer = ViewContainer::new(EXPLORER, "Explorer", Icon::Files, cx);
//!     explorer.add_view(ViewSpec {
//!         id: OUTLINE,
//!         title: "Outline".into(),
//!         order: 20,
//!         default_collapsed: true,
//!         view: outline.into(),
//!     }, cx);
//!     explorer
//! });
//! workspace.add_panel(DockPosition::Left, explorer, window, cx);
//! ```

use std::collections::HashMap;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, div,
};
use kiln_command::Value;
use kiln_ui::Icon;

use crate::{DockItem, PaneAction, PanelId, StackedView, ViewId, ViewStack};

/// A view to add to a [`ViewContainer`].
pub struct ViewSpec {
    /// Distinct among the container's views, and what its saved state is
    /// kept by.
    pub id: ViewId,
    /// Its header's text.
    pub title: SharedString,
    /// Where it goes: views go top to bottom by order, and views of the
    /// same order as they were added.
    pub order: i32,
    /// Whether it starts collapsed, until the user or a restored layout
    /// says otherwise.
    pub default_collapsed: bool,
    /// What it shows.
    pub view: AnyView,
}

/// A dock panel of views, registered by id, stacked under collapsible
/// headers.
pub struct ViewContainer {
    id: PanelId,
    title: SharedString,
    icon: Icon,
    show_command: Option<&'static str>,
    stack: Entity<ViewStack>,
    /// Each view's order, by id.
    orders: HashMap<ViewId, i32>,
    /// The view the dock's header names, drawn without one of its own.
    lead: Option<ViewId>,
    /// Each view's actions, by id: the lead's, or a lone view's, go in the
    /// dock's header.
    actions: HashMap<ViewId, Vec<PaneAction>>,
    /// What a view's header shows after its title while it's collapsed.
    details: HashMap<ViewId, AnyView>,
    focus: FocusHandle,
    /// The stack's changes are the container's, for the dock to hear.
    _stack_changed: Subscription,
}

impl ViewContainer {
    /// An empty container, panel `id`, with `title` over it and `icon` on
    /// the rail.
    pub fn new(
        id: PanelId,
        title: impl Into<SharedString>,
        icon: Icon,
        cx: &mut Context<Self>,
    ) -> Self {
        let stack = cx.new(|_| ViewStack::new());
        let stack_changed = cx.observe(&stack, |_, _, cx| cx.notify());
        Self {
            id,
            title: title.into(),
            icon,
            show_command: None,
            stack,
            orders: HashMap::new(),
            lead: None,
            actions: HashMap::new(),
            details: HashMap::new(),
            focus: cx.focus_handle(),
            _stack_changed: stack_changed,
        }
    }

    /// Builds it shown by `command`, whose key the rail's tooltip shows.
    pub fn show_command(mut self, command: &'static str) -> Self {
        self.show_command = Some(command);
        self
    }

    /// Builds it with `view` as its lead, once added: the view the dock's
    /// header names, drawn without a header, never collapsed, and whose
    /// actions are the dock header's. Give it the lowest order.
    pub fn lead(mut self, view: ViewId) -> Self {
        self.lead = Some(view);
        self
    }

    /// `view`'s actions, from now on: in the dock's header if it's the
    /// lead, or the container's only view.
    pub fn set_actions(&mut self, view: ViewId, actions: Vec<PaneAction>, cx: &mut Context<Self>) {
        self.actions.insert(view, actions);
        cx.notify();
    }

    /// Shows `detail` after `view`'s title while it's collapsed, now and
    /// once added.
    pub fn set_detail(&mut self, view: ViewId, detail: AnyView, cx: &mut Context<Self>) {
        self.details.insert(view, detail.clone());
        self.stack
            .update(cx, |stack, cx| stack.set_detail(view, Some(detail), cx));
    }

    /// Adds `view` below the views of its order or less, above those of
    /// more. A view with its id that's there is replaced, moving to the new
    /// one's order but keeping its collapse and weight.
    pub fn add_view(&mut self, view: ViewSpec, cx: &mut Context<Self>) {
        let ViewSpec {
            id,
            title,
            order,
            default_collapsed,
            view,
        } = view;
        self.orders.insert(id, order);
        let (lead, detail) = (self.lead == Some(id), self.details.get(&id).cloned());
        let orders = &self.orders;
        self.stack.update(cx, |stack, cx| {
            let ix = (stack.view_ids().into_iter())
                .filter(|other| *other != id)
                .take_while(|other| orders.get(other).is_none_or(|other| *other <= order))
                .count();
            let mut view = StackedView::new(id, title, view)
                .collapsed(default_collapsed)
                .lead(lead);
            if let Some(detail) = detail {
                view = view.detail(detail);
            }
            stack.insert(ix, view, cx);
        });
    }

    /// Titles it `title` from now on: its rail button's tooltip and the
    /// dock's header.
    pub fn set_title(&mut self, title: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.title = title.into();
        cx.notify();
    }

    /// The views' ids, top to bottom.
    pub fn view_ids(&self, cx: &App) -> Vec<ViewId> {
        self.stack.read(cx).view_ids()
    }

    /// The stack the views are in, to collapse, expand or weigh them.
    pub fn stack(&self) -> &Entity<ViewStack> {
        &self.stack
    }
}

impl Render for ViewContainer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .child(self.stack.clone())
    }
}

impl Focusable for ViewContainer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for ViewContainer {
    fn id(&self) -> PanelId {
        self.id
    }

    fn title(&self, _: &App) -> SharedString {
        self.title.clone()
    }

    fn icon(&self, _: &App) -> Icon {
        self.icon
    }

    fn show_command(&self) -> Option<&'static str> {
        self.show_command
    }

    /// The actions of the view its header names: the lead, or a lone view.
    fn actions(&self, cx: &App) -> Vec<PaneAction> {
        let stack = self.stack.read(cx);
        (stack.view_ids().into_iter())
            .find(|view| stack.is_headless(*view) == Some(true))
            .and_then(|view| self.actions.get(&view).cloned())
            .unwrap_or_default()
    }

    /// Its views' collapse and weight, as [`ViewStack::save`] gives them.
    fn save_state(&self, cx: &App) -> Option<Value> {
        Some(self.stack.read(cx).save())
    }

    /// See [`ViewStack::restore`]: a saved view it doesn't have is
    /// ignored.
    fn restore_state(&mut self, saved: &Value, cx: &mut Context<Self>) {
        self.stack.update(cx, |stack, cx| stack.restore(saved, cx));
    }
}
