// SPDX-License-Identifier: LicenseRef-Proprietary

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

use std::{cell::Cell, ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels,
    Refineable as _, Render, RenderOnce, ScrollStrategy, StatefulInteractiveElement as _,
    StyleRefinement, Styled, UniformListScrollHandle, Window, div, prelude::FluentBuilder as _,
    uniform_list,
};

use crate::{
    ActiveTheme as _, ROW_HEIGHT, Tokens,
    nav::{NavKey, nav_key, page_rows},
    scroll_area::scrollbar,
};

/// How a row is to be drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowState {
    pub selected: bool,
    /// The pointer is over it.
    pub hovered: bool,
    /// The list has focus, so a selected row shows it strongly.
    pub focused: bool,
}

impl RowState {
    /// The row's (background, border) in the theme's list colours: hovered,
    /// selected in a focused list (outlined), or selected in one without
    /// focus.
    pub(crate) fn colors(self, c: &Tokens<Hsla>) -> (Option<Hsla>, Option<Hsla>) {
        let list = &c.list;
        match (self.selected, self.focused, self.hovered) {
            (true, true, _) => (Some(list.row_selected), Some(list.row_focus_border)),
            (true, false, _) => (Some(list.row_selected_inactive), None),
            (false, _, true) => (Some(list.row_hover), None),
            (false, _, false) => (None, None),
        }
    }
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
        let (background, border) = self.state.colors(&cx.theme().colors);
        let caller = std::mem::take(self.base.style());
        let mut row = self
            .base
            .flex()
            .h(ROW_HEIGHT)
            .w_full()
            .items_center()
            .when_some(background, |row, background| row.bg(background))
            .when_some(border, |row, border| row.border_1().border_color(border));
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
        range
            .map(|ix| {
                let state = RowState {
                    selected: self.selected == Some(ix),
                    hovered: self.hovered == Some(ix),
                    focused,
                };
                let row = self.delegate.render_row(ix, state, window, cx);
                div()
                    .id(ix)
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        let handle = self.scroll.0.borrow().base_handle.clone();
        div()
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
