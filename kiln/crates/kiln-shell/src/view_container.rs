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

use crate::{DockItem, PanelId, StackedView, ViewId, ViewStack};

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
            focus: cx.focus_handle(),
            _stack_changed: stack_changed,
        }
    }

    /// Builds it shown by `command`, whose key the rail's tooltip shows.
    pub fn show_command(mut self, command: &'static str) -> Self {
        self.show_command = Some(command);
        self
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
        let orders = &self.orders;
        self.stack.update(cx, |stack, cx| {
            let ix = (stack.view_ids().into_iter())
                .filter(|other| *other != id)
                .take_while(|other| orders.get(other).is_none_or(|other| *other <= order))
                .count();
            let view = StackedView::new(id, title, view).collapsed(default_collapsed);
            stack.insert(ix, view, cx);
        });
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
