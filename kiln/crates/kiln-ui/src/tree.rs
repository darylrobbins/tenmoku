// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`Tree`]: a [`TreeModel`]'s nodes as [`TreeRow`]s, expanded and
//! collapsed by the tree, which keeps the selection as [`crate::List`]
//! does.
//!
//! Keys are handled with `on_key_down` on the focused tree, as a list's
//! are: `up`, `down`, `home`, `end`, `pageup` and `pagedown` move the
//! selection, `enter` confirms it, `right` expands the selected node (or,
//! expanded, moves to its first child) and `left` collapses it (or,
//! collapsed, moves to its parent).

use std::{collections::HashSet, hash::Hash, ops::Range};

use gpui::{
    AnyElement, App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, uniform_list,
};

use crate::{
    CHEVRON_WIDTH, Icon, TREE_INDENT, TreeRow, UiTheme,
    nav::{NavKey, nav_key, page_rows},
    tree_row::PADDING,
};

/// What a [`Tree`] shows: nodes named by an `Id`, each with its children.
pub trait TreeModel: 'static {
    /// A node's identity, stable while the model changes, so the selection
    /// and the expanded nodes follow it.
    type Id: Clone + Eq + Hash + 'static;

    /// The top-level nodes, in order.
    fn roots(&self) -> Vec<Self::Id>;

    /// `id`'s children, in order; asked only of expanded nodes.
    fn children(&self, id: &Self::Id) -> Vec<Self::Id>;

    /// Whether `id` can be expanded: it has a chevron. By default, whether
    /// it has children; a model that loads them lazily says so sooner.
    fn is_parent(&self, id: &Self::Id) -> bool {
        !self.children(id).is_empty()
    }

    fn label(&self, id: &Self::Id) -> SharedString;

    /// The icon before `id`'s label, which may differ while it's expanded.
    fn icon(&self, _id: &Self::Id, _expanded: bool) -> Option<Icon> {
        None
    }
}

/// What a [`Tree`] tells its subscribers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEvent<Id> {
    /// A node was selected by a click or a key.
    Selected(Id),
    /// A node was confirmed: double-clicked, or selected when `enter` was
    /// pressed.
    Confirmed(Id),
    /// A node was expanded by a click on its chevron or a key.
    Expanded(Id),
    /// A node was collapsed by a click on its chevron or a key.
    Collapsed(Id),
}

/// One visible row: an expanded node's descendants follow it.
struct Row<Id> {
    id: Id,
    depth: usize,
    parent: Option<usize>,
    is_parent: bool,
}

/// A tree of a model's nodes, those of expanded nodes shown below them,
/// over GPUI's `uniform_list`. Clicking a row selects it, clicking its
/// chevron expands or collapses it, and a double click confirms it.
/// Subscribe with `cx.subscribe_in(&tree, window, ..)` for its
/// [`TreeEvent`]s.
pub struct Tree<M: TreeModel> {
    model: M,
    expanded: HashSet<M::Id>,
    rows: Vec<Row<M::Id>>,
    selected: Option<usize>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl<M: TreeModel> EventEmitter<TreeEvent<M::Id>> for Tree<M> {}

impl<M: TreeModel> Tree<M> {
    pub fn new(model: M, cx: &mut Context<Self>) -> Self {
        let mut tree = Self {
            model,
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        };
        tree.flatten();
        tree
    }

    pub fn model(&self) -> &M {
        &self.model
    }

    /// The model, to change its nodes; call [`Tree::model_changed`] after.
    pub fn model_mut(&mut self) -> &mut M {
        &mut self.model
    }

    /// For the model's owner once its nodes change: rebuilds the rows,
    /// keeping the selected node selected while it's still shown.
    pub fn model_changed(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_id().cloned();
        let at = self.selected;
        self.flatten();
        self.selected = match selected {
            Some(id) => self
                .index_of(&id)
                .or_else(|| at.map(|ix| ix.min(self.rows.len().saturating_sub(1))))
                .filter(|_| !self.rows.is_empty()),
            None => None,
        };
        cx.notify();
    }

    /// The visible nodes, in order, with their depths: 0 for a root.
    pub fn visible(&self) -> impl Iterator<Item = (&M::Id, usize)> {
        self.rows.iter().map(|row| (&row.id, row.depth))
    }

    pub fn is_expanded(&self, id: &M::Id) -> bool {
        self.expanded.contains(id)
    }

    /// Expands or collapses `id` without emitting an event: for the tree's
    /// owner, where a click or a key emits [`TreeEvent::Expanded`] or
    /// [`TreeEvent::Collapsed`]. Its descendants keep their own state.
    /// Collapsing the selected node's ancestor selects it.
    pub fn set_expanded(&mut self, id: &M::Id, expanded: bool, cx: &mut Context<Self>) {
        let hides_selection = !expanded && self.selected.is_some_and(|ix| self.is_under(ix, id));
        let changed = if expanded {
            self.expanded.insert(id.clone())
        } else {
            self.expanded.remove(id)
        };
        if changed {
            self.model_changed(cx);
            if hides_selection {
                self.selected = self.index_of(id);
            }
        }
    }

    /// Whether row `ix` is a descendant of `id`'s.
    fn is_under(&self, ix: usize, id: &M::Id) -> bool {
        let mut parent = self.rows[ix].parent;
        while let Some(at) = parent {
            if self.rows[at].id == *id {
                return true;
            }
            parent = self.rows[at].parent;
        }
        false
    }

    pub fn selected_id(&self) -> Option<&M::Id> {
        Some(&self.rows.get(self.selected?)?.id)
    }

    /// Selects `id` if it's shown (`None` clears the selection) and
    /// scrolls it into view, without emitting an event.
    pub fn set_selected(&mut self, id: Option<&M::Id>, cx: &mut Context<Self>) {
        self.selected = id.and_then(|id| self.index_of(id));
        self.scroll_to_selected();
        cx.notify();
    }

    /// Selects row `ix` as a click or key would, emitting
    /// [`TreeEvent::Selected`] if the selection changed.
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.rows.len() {
            return;
        }
        let changed = self.selected != Some(ix);
        self.selected = Some(ix);
        self.scroll_to_selected();
        cx.notify();
        if changed {
            cx.emit(TreeEvent::Selected(self.rows[ix].id.clone()));
        }
    }

    fn confirm(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.rows.len() {
            self.select(ix, cx);
            cx.emit(TreeEvent::Confirmed(self.rows[ix].id.clone()));
        }
    }

    /// Expands or collapses row `ix`'s node as a click or key would.
    fn toggle(&mut self, ix: usize, expand: bool, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(ix).filter(|row| row.is_parent) else {
            return;
        };
        let id = row.id.clone();
        if self.is_expanded(&id) == expand {
            return;
        }
        let selected = self.selected_id().cloned();
        self.set_expanded(&id, expand, cx);
        cx.emit(if expand {
            TreeEvent::Expanded(id)
        } else {
            TreeEvent::Collapsed(id)
        });
        if let Some(now) = self
            .selected_id()
            .filter(|now| Some(*now) != selected.as_ref())
        {
            cx.emit(TreeEvent::Selected(now.clone()));
        }
    }

    fn scroll_to_selected(&self) {
        if let Some(ix) = self.selected {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    fn index_of(&self, id: &M::Id) -> Option<usize> {
        self.rows.iter().position(|row| row.id == *id)
    }

    fn flatten(&mut self) {
        let mut rows = Vec::new();
        let mut stack: Vec<(M::Id, usize, Option<usize>)> = self
            .model
            .roots()
            .into_iter()
            .rev()
            .map(|id| (id, 0, None))
            .collect();
        while let Some((id, depth, parent)) = stack.pop() {
            let is_parent = self.model.is_parent(&id);
            if is_parent && self.expanded.contains(&id) {
                let me = Some(rows.len());
                let children = self.model.children(&id);
                stack.extend(children.into_iter().rev().map(|id| (id, depth + 1, me)));
            }
            rows.push(Row {
                id,
                depth,
                parent,
                is_parent,
            });
        }
        self.rows = rows;
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.rows.len();
        let key = nav_key(event, self.selected, count, page_rows(&self.scroll, count));
        match (key, self.selected) {
            (Some(NavKey::To(ix)), _) => self.select(ix, cx),
            (Some(NavKey::Confirm), Some(ix)) => self.confirm(ix, cx),
            (Some(NavKey::Right), Some(ix)) if self.rows[ix].is_parent => {
                if self.is_expanded(&self.rows[ix].id) {
                    let child = self.rows.get(ix + 1).filter(|row| row.parent == Some(ix));
                    if child.is_some() {
                        self.select(ix + 1, cx);
                    }
                } else {
                    self.toggle(ix, true, cx);
                }
            }
            (Some(NavKey::Left), Some(ix)) => {
                let row = &self.rows[ix];
                if row.is_parent && self.is_expanded(&row.id) {
                    self.toggle(ix, false, cx);
                } else if let Some(parent) = row.parent {
                    self.select(parent, cx);
                }
            }
            (Some(NavKey::Confirm | NavKey::Left), None) | (Some(NavKey::Right), _) | (None, _) => {
                return;
            }
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
        // TreeRow reads cx.theme(); the UiTheme it takes is ignored.
        let theme = UiTheme::default();
        let focused = self.focus.is_focused(window);
        range
            .filter_map(|ix| {
                let row = self.rows.get(ix)?;
                let expanded = self.expanded.contains(&row.id);
                let tree_row = TreeRow::new(ix, self.model.label(&row.id), &theme)
                    .depth(row.depth)
                    .when(row.is_parent, |tree_row| tree_row.directory(expanded))
                    .when_some(self.model.icon(&row.id, expanded), TreeRow::icon)
                    .selected(self.selected == Some(ix))
                    .focused(focused)
                    .on_click(
                        cx.listener(move |tree, event, _, cx| tree.on_row_click(ix, event, cx)),
                    );
                // The chevron's own target, over the row so a click on it
                // toggles the node rather than selecting the row.
                let chevron = row.is_parent.then(|| {
                    div()
                        .id(("chevron", ix))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(PADDING + TREE_INDENT * row.depth as f32)
                        .w(CHEVRON_WIDTH)
                        .occlude()
                        .on_click(cx.listener(move |tree, _, _, cx| tree.toggle(ix, !expanded, cx)))
                });
                Some(
                    div()
                        .relative()
                        .w_full()
                        .child(tree_row)
                        .children(chevron)
                        .into_any_element(),
                )
            })
            .collect()
    }
}

impl<M: TreeModel> Focusable for Tree<M> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl<M: TreeModel> Render for Tree<M> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx)
            })
            .size_full()
            .child(
                uniform_list(
                    "tree-rows",
                    self.rows.len(),
                    cx.processor(|tree, range, window, cx| tree.render_rows(range, window, cx)),
                )
                .track_scroll(&self.scroll)
                .size_full(),
            )
    }
}
