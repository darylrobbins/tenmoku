// SPDX-License-Identifier: Apache-2.0

//! [`List`]: a virtualised list of rows a [`ListDelegate`] supplies, with a
//! selection the list keeps and moves.
//!
//! [`ListRow`] is a row's usual look, for delegates to build on. The list
//! draws a scrollbar over its right edge while its rows overflow.
//!
//! Keys are handled with `on_key_down` on the focused list, not GPUI
//! actions, so a list works with no keymap: `up`, `down`, `home`, `end`,
//! `pageup` and `pagedown` move the selection and `enter` confirms it. A
//! key with a modifier, and any other key, goes on to the app's bindings.
//!
//! To a screen reader the list is a `ListBox` named by
//! [`ListDelegate::accessible_name`], and each row a `ListBoxOption` named
//! by [`ListDelegate::row_name`], with its place in the list and whether
//! it's selected ([`RowState::option`]). Focus is on the list, so the
//! selected row is its active descendant: the screen reader's focus
//! follows the selection while the list has focus.

use std::{cell::Cell, ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels,
    Refineable as _, Render, RenderOnce, ScrollStrategy, SharedString, StatefulInteractiveElement,
    StyleRefinement, Styled, UniformListScrollHandle, Window, div, prelude::FluentBuilder as _,
    uniform_list,
};

use crate::{
    ActiveTheme as _, ROW_HEIGHT, Role, States, Tokens,
    nav::{NavKey, nav_key, page_rows},
    node, node_with,
    scroll_area::scrollbar,
    tree_row::{selection_colors, selection_edge},
};

/// How a row is to be drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowState {
    pub selected: bool,
    /// The pointer is over it.
    pub hovered: bool,
    /// The list has focus, so a selected row shows it with an accent edge.
    pub focused: bool,
}

impl RowState {
    /// The debug selector, in a test build or one with `test-support`, of
    /// a marker over the row [`RowState::option`] made the active
    /// descendant, for tests: the row keeps any selector of its own.
    pub const ACTIVE_DESCENDANT: &str = "kiln-ui-active-descendant";

    /// `element` as a screen reader sees a row of a list: a
    /// `ListBoxOption` named `name`, row `ix` (from 0) of `count`, and
    /// selected or not. A selected row in a focused list (`focused`) is
    /// also the active descendant, where the screen reader's focus is,
    /// which only works while the focused element is an ancestor of the
    /// row: [`List`]'s own rows, or a list such as a tab switcher that
    /// holds focus itself. A picker whose query has focus leaves `focused`
    /// off, as the query is the rows' sibling. The element needs an id,
    /// and is made `relative`.
    pub fn option<E>(
        self,
        element: E,
        name: impl Into<SharedString>,
        (ix, count): (usize, usize),
        window: &Window,
        cx: &mut App,
    ) -> E
    where
        E: StatefulInteractiveElement + gpui::Element + ParentElement + Styled,
    {
        let states = States {
            selected: Some(self.selected),
            ..States::default()
        };
        let active = self.selected && self.focused;
        node_with(element, window, cx, Role::ListBoxOption, name, states)
            .aria_position_in_set(ix + 1)
            .aria_size_of_set(count)
            .relative()
            .when(active, |row| {
                row.aria_active_descendant()
                    .children(active_descendant_marker())
            })
    }

    /// The row's (background, edge) in the theme's list colours: hovered,
    /// selected in a focused list (tinted, with an accent edge down its
    /// left), or selected in one without focus (tinted).
    pub(crate) fn colors(self, c: &Tokens<Hsla>) -> (Option<Hsla>, Option<Hsla>) {
        match (self.selected, self.hovered) {
            (true, _) => {
                let (background, edge) = selection_colors(self.focused, c);
                (Some(background), edge)
            }
            (false, true) => (Some(c.list.row_hover), None),
            (false, false) => (None, None),
        }
    }
}

/// The marker over an active descendant that tests find by
/// [`RowState::ACTIVE_DESCENDANT`]; none outside tests.
fn active_descendant_marker() -> Option<Div> {
    #[cfg(any(test, feature = "test-support"))]
    let marker = Some(
        div()
            .absolute()
            .inset_0()
            .debug_selector(|| RowState::ACTIVE_DESCENDANT.into()),
    );
    #[cfg(not(any(test, feature = "test-support")))]
    let marker = None;
    marker
}

/// A list row's usual look for its [`RowState`], in the theme's list
/// colours: [`ROW_HEIGHT`] tall and full width, its children in a row,
/// centred vertically. Styled like a `div`, the caller's styles over its
/// own.
///
/// ```ignore
/// fn render_row(&mut self, ix: usize, state: RowState, ..) -> AnyElement {
///     ListRow::new(state).px_3().child(self.names[ix].clone()).into_any_element()
/// }
/// ```
#[derive(IntoElement)]
pub struct ListRow {
    base: Div,
    state: RowState,
}

impl ListRow {
    pub fn new(state: RowState) -> Self {
        Self { base: div(), state }
    }
}

impl Styled for ListRow {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for ListRow {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}

impl RenderOnce for ListRow {
    fn render(mut self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (background, edge) = self.state.colors(&cx.theme().colors);
        let caller = std::mem::take(self.base.style());
        let mut row = self
            .base
            .flex()
            .h(ROW_HEIGHT)
            .w_full()
            .items_center()
            .when_some(background, |row, background| row.bg(background))
            .when_some(edge, |row, edge| {
                row.relative()
                    .child(selection_edge(edge).debug_selector(|| "list-row-edge".into()))
            });
        row.style().refine(&caller);
        row
    }
}

/// What a [`List`] shows: how many rows, and each row's element.
pub trait ListDelegate: Sized + 'static {
    fn count(&self) -> usize;

    /// Row `ix`'s element. Rows are measured from the first, so they should
    /// share one height (such as [`crate::ROW_HEIGHT`]). The list adds the
    /// row's click handling around it. [`ListRow`] gives it the usual look.
    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        window: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement;

    /// The list's name to a screen reader, such as "Problems". Unnamed by
    /// default.
    fn accessible_name(&self, _cx: &App) -> SharedString {
        SharedString::default()
    }

    /// Row `ix`'s name to a screen reader: what its row says, such as a
    /// file's name. Unnamed by default.
    fn row_name(&self, _ix: usize, _cx: &App) -> SharedString {
        SharedString::default()
    }
}

/// What a [`List`] tells its subscribers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListEvent {
    /// Row `ix` was selected by a click or a key.
    Selected(usize),
    /// Row `ix` was confirmed: double-clicked, or selected when `enter`
    /// was pressed.
    Confirmed(usize),
}

/// A list of a delegate's rows over GPUI's `uniform_list`, so only the rows
/// in view are built. It owns the selection: a click selects a row, a
/// double click confirms it, and keys move it (see the module docs),
/// scrolling the selected row into view. Subscribe with
/// `cx.subscribe_in(&list, window, ..)` for its [`ListEvent`]s.
pub struct List<D: ListDelegate> {
    delegate: D,
    selected: Option<usize>,
    hovered: Option<usize>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// The scrollbar's thumb while it's dragged.
    grab: Rc<Cell<Option<Pixels>>>,
}

impl<D: ListDelegate> EventEmitter<ListEvent> for List<D> {}

impl<D: ListDelegate> List<D> {
    pub fn new(delegate: D, cx: &mut Context<Self>) -> Self {
        Self {
            delegate,
            selected: None,
            hovered: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            grab: Rc::default(),
        }
    }

    pub fn delegate(&self) -> &D {
        &self.delegate
    }

    /// The delegate, to change its rows; call [`List::rows_changed`] after.
    pub fn delegate_mut(&mut self) -> &mut D {
        &mut self.delegate
    }

    /// For the delegate's owner once the rows change: keeps the selection
    /// in range and redraws.
    pub fn rows_changed(&mut self, cx: &mut Context<Self>) {
        let count = self.delegate.count();
        self.selected = self
            .selected
            .map(|ix| ix.min(count.saturating_sub(1)))
            .filter(|_| count > 0);
        self.hovered = self.hovered.filter(|&ix| ix < count);
        cx.notify();
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Selects row `ix` (clamped to the rows; `None` clears the selection)
    /// and scrolls it into view, without emitting an event: for the list's
    /// owner, where a click or a key emits [`ListEvent::Selected`].
    pub fn set_selected(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        let count = self.delegate.count();
        self.selected = ix.filter(|_| count > 0).map(|ix| ix.min(count - 1));
        if let Some(ix) = self.selected {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// Selects row `ix` as a click or key would, emitting
    /// [`ListEvent::Selected`] if the selection changed.
    pub fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        let before = self.selected;
        self.set_selected(Some(ix), cx);
        if let Some(ix) = self.selected.filter(|_| self.selected != before) {
            cx.emit(ListEvent::Selected(ix));
        }
    }

    /// Selects and confirms row `ix`, if it exists.
    pub fn confirm(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.delegate.count() {
            self.select(ix, cx);
            cx.emit(ListEvent::Confirmed(ix));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.delegate.count();
        match nav_key(event, self.selected, count, page_rows(&self.scroll, count)) {
            Some(NavKey::To(ix)) => self.select(ix, cx),
            Some(NavKey::Confirm) => {
                if let Some(ix) = self.selected {
                    self.confirm(ix, cx);
                }
            }
            Some(NavKey::Left | NavKey::Right) | None => return,
        }
        cx.stop_propagation();
    }

    fn on_row_click(&mut self, ix: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        if event.click_count() >= 2 {
            self.confirm(ix, cx);
        } else {
            self.select(ix, cx);
        }
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let focused = self.focus.is_focused(window);
        let count = self.delegate.count();
        range
            .map(|ix| {
                let state = RowState {
                    selected: self.selected == Some(ix),
                    hovered: self.hovered == Some(ix),
                    focused,
                };
                let row = self.delegate.render_row(ix, state, window, cx);
                let name = self.delegate.row_name(ix, cx);
                state
                    .option(div().id(ix), name, (ix, count), window, cx)
                    .w_full()
                    .on_hover(cx.listener(move |list, hovered: &bool, _, cx| {
                        let now = if *hovered {
                            Some(ix)
                        } else {
                            list.hovered.filter(|&was| was != ix)
                        };
                        if now != list.hovered {
                            list.hovered = now;
                            cx.notify();
                        }
                    }))
                    .on_click(
                        cx.listener(move |list, event, _, cx| list.on_row_click(ix, event, cx)),
                    )
                    .child(row)
                    .into_any_element()
            })
            .collect()
    }
}

impl<D: ListDelegate> Focusable for List<D> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl<D: ListDelegate> Render for List<D> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        let handle = self.scroll.0.borrow().base_handle.clone();
        // The focused node, so the selected row can be its active
        // descendant.
        let name = self.delegate.accessible_name(cx);
        node(div().id("kiln-list"), window, cx, Role::ListBox, name)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx)
            })
            .relative()
            .size_full()
            .child(
                uniform_list(
                    "list-rows",
                    self.delegate.count(),
                    cx.processor(|list, range, window, cx| list.render_rows(range, window, cx)),
                )
                .track_scroll(&self.scroll)
                .size_full(),
            )
            .child(scrollbar(handle, self.grab.clone(), cx))
    }
}
