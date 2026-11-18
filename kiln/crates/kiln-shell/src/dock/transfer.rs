// SPDX-License-Identifier: Apache-2.0

//! Moving a panel between docks: [`DockState::take_panel`] takes it out
//! of one, as a [`DockedPanel`], and [`DockState::insert_panel`] puts it in
//! another, the same entity, focus handle and state.
//!
//! Docks built with the same [`DockState::transfer_group`] take each
//! other's drags: a tabbed dock's tab, or a side dock's rail button, which
//! drag as [`DraggedDockTab`]s. A side dock without tabs takes another
//! dock's panel on its column or its rail's top group ([`DockDrop::Into`]),
//! and its own panel's rail button on its rail's foot group
//! ([`DockDrop::Foot`]), which a host maps to its bottom dock; a tabbed dock
//! takes another dock's panel in its groups' split, join and strip zones.
//! Each draws the drop indicator while a drag hovers; the drop goes to
//! [`DockView::panel_dropped`], for the host to move the panel, as two
//! docks change. Docks without a shared group ignore each other's drags.

use gpui::{
    AnyElement, App, Axis, Bounds, Context, DragMoveEvent, EntityId, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, SharedString,
    Styled as _, Window, anchored, deferred, div, prelude::FluentBuilder as _,
};
use kiln_ui::ui_theme;

use super::{DockDrop, DockState, DockView, DraggedDockTab, Group, MoveTo, PanelHandle, PanelId};
use crate::pane::{DropZone, insertion, record};

/// A panel taken out of a dock ([`DockState::take_panel`]), owned until
/// it's put in another ([`DockState::insert_panel`]): its entity, without
/// its type.
pub struct DockedPanel {
    handle: Box<dyn PanelHandle>,
}

impl DockedPanel {
    /// Its id, as it was registered.
    pub fn id(&self) -> PanelId {
        self.handle.id()
    }
}

impl std::fmt::Debug for DockedPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DockedPanel").field(&self.id()).finish()
    }
}

impl DockDrop {
    /// Where [`DockState::move_panel`] puts a panel dropped on a tabbed
    /// dock's zone: none for a side dock's drops.
    pub fn move_to(self) -> Option<MoveTo> {
        Some(match self {
            DockDrop::SplitBefore(group) => MoveTo::Before(group),
            DockDrop::SplitAfter(group) => MoveTo::After(group),
            DockDrop::Join(group) => MoveTo::Group {
                group,
                index: usize::MAX,
            },
            DockDrop::Insert { group, index } => MoveTo::Group { group, index },
            DockDrop::Into | DockDrop::Foot => return None,
        })
    }
}

impl DockState {
    /// Builds it in the transfer group `group`: it takes the drags of the
    /// other docks built in it, and its rail's buttons drag, as its tabs
    /// do. Without one, it takes only its own tabs' drags.
    pub fn transfer_group(mut self, group: &'static str) -> Self {
        self.transfer_group = Some(group);
        self
    }

    /// The transfer group it was built in, if any.
    pub fn transfer_group_name(&self) -> Option<&'static str> {
        self.transfer_group
    }

    /// Takes `panel` out of the dock, for another to take in: it's no
    /// longer registered, on the rail or in a group, and a group it
    /// empties goes, as a tab dragged out does. Its group shows the tab
    /// beside it in its place; in a tabbed dock, with none, as closing its
    /// last tab does, the nearest drawn group is focused, or with none the
    /// dock hides. None if no such panel is registered.
    pub fn take_panel(&mut self, panel: PanelId) -> Option<DockedPanel> {
        let ix = self.panels.iter().position(|handle| handle.id() == panel)?;
        let shown = self.is_shown(panel);
        if let Some(from) = self.group_of(panel) {
            self.open_shown();
            let open = self.open_panels_in(from);
            let others: Vec<usize> = (self.drawn_groups().into_iter())
                .filter(|&other| other != from)
                .collect();
            let group = &mut self.groups[from];
            group.panels.retain(|id| *id != panel);
            // A tabbed dock's group left with no tab hides, as closing its
            // last does.
            let mut emptied = false;
            if group.active == Some(panel) {
                let ix = open.iter().position(|id| *id == panel);
                let next =
                    ix.and_then(|ix| open.get(ix + 1).or(ix.checked_sub(1).map(|ix| &open[ix])));
                emptied = next.is_none() && self.tabs;
                group.active = next.copied().or(group.panels.first().copied());
            }
            let focused = self.focused == from;
            let removed = group.panels.is_empty() && self.groups.len() > 1;
            if removed {
                self.groups.remove(from);
                self.focused -= usize::from(self.focused > from);
                self.focused = self.focused.min(self.groups.len() - 1);
            }
            if emptied && shown {
                // The nearest drawn group is focused, as numbered now.
                match Self::nearest(&others, from) {
                    Some(nearest) if focused => {
                        self.move_focus(nearest - usize::from(removed && nearest > from));
                    }
                    Some(_) => {}
                    None => self.set_visible(false),
                }
            }
        }
        self.opened.remove(&panel);
        self.joined.remove(&panel);
        if self.followed == Some(panel) {
            self.followed = None;
        }
        let handle = self.panels.remove(ix);
        Some(DockedPanel { handle })
    }

    /// Puts `panel`, taken from another dock, in this one, `at` in the
    /// rail's order (the end if past it): in the first group, before the
    /// next panel there, its active panel if it had none, neither shown
    /// nor focused. A tabbed host then moves it to a group
    /// ([`DockState::move_panel`], [`DockDrop::move_to`]) and shows it.
    /// A panel registered with its id is replaced, as
    /// [`DockState::add_panel`] does.
    pub fn insert_panel(&mut self, panel: DockedPanel, at: usize) -> PanelId {
        let id = panel.id();
        if let Some(ix) = self.panels.iter().position(|handle| handle.id() == id) {
            self.panels[ix] = panel.handle;
            return id;
        }
        let at = at.min(self.panels.len());
        if self.groups.is_empty() {
            self.groups.push(Group::new(Vec::new(), None, 1.));
        }
        let first = &self.groups[0];
        let next = (self.panels[at..].iter())
            .map(|handle| handle.id())
            .find_map(|next| first.panels.iter().position(|id| *id == next));
        self.panels.insert(at, panel.handle);
        let first = &mut self.groups[0];
        first.panels.insert(next.unwrap_or(first.panels.len()), id);
        first.active.get_or_insert(id);
        id
    }

    /// Takes the rail action `id` off the rail, the counterpart of
    /// [`DockState::add_action`]: false if it has none of that id.
    pub fn remove_action(&mut self, id: &str) -> bool {
        let had = self.actions.len();
        self.actions.retain(|(action, _)| action.id != id);
        self.actions.len() != had
    }

    /// The ids of the rail's actions, in their order.
    pub fn action_ids(&self) -> Vec<&'static str> {
        self.actions.iter().map(|(action, _)| action.id).collect()
    }

    /// `panel`'s focus handle, if it's registered.
    pub fn panel_focus(&self, panel: PanelId, cx: &App) -> Option<FocusHandle> {
        Some(self.handle(panel)?.focus_handle(cx))
    }

    /// `panel`'s title, if it's registered.
    pub fn panel_title(&self, panel: PanelId, cx: &App) -> Option<SharedString> {
        Some(self.handle(panel)?.title(cx))
    }

    /// Whether `dragged` is another dock's, of this one's transfer group.
    fn accepts(&self, dragged: &DraggedDockTab, dock: EntityId) -> bool {
        dragged.dock != dock
            && self.transfer_group.is_some()
            && dragged.group == self.transfer_group
    }

    /// Where another dock's panel would drop on this tabbed one at `at`,
    /// by the last frame's layout: a drawn group's strip at the slot, its
    /// content's centre to join it, or its outer quarter to split a group
    /// off, where that leaves each half its least. None if it isn't of
    /// this dock's transfer group, or has a panel of its id here.
    pub(super) fn foreign_tab_target(
        &self,
        dragged: &DraggedDockTab,
        at: Point<Pixels>,
        dock: EntityId,
    ) -> Option<DockDrop> {
        if !self.accepts(dragged, dock) || self.handle(dragged.panel).is_some() {
            return None;
        }
        let drawn = self.drawn.groups.borrow();
        let hovered = drawn
            .iter()
            .find(|drawn| drawn.cell.bounds().contains(&at))?;
        let group = hovered.group;
        if hovered.strip.bounds().contains(&at) {
            let index = insertion(&hovered.tab_layout(), at.x);
            return Some(DockDrop::Insert { group, index });
        }
        let content = hovered.content().filter(|content| content.contains(&at))?;
        let axis = self.group_axis();
        let drop = match DropZone::along(content, at, axis) {
            DropZone::Centre => return Some(DockDrop::Join(group)),
            DropZone::Left | DropZone::Up => DockDrop::SplitBefore(group),
            DropZone::Right | DropZone::Down => DockDrop::SplitAfter(group),
        };
        let length = match axis {
            Axis::Horizontal => hovered.cell.bounds().size.width,
            Axis::Vertical => hovered.cell.bounds().size.height,
        };
        (length / 2. >= self.min_group()).then_some(drop)
    }

    /// Where the rail's two groups were laid out: the top one, down to the
    /// foot's first item (the whole rail without a foot), and the foot.
    fn rail_zones(&self) -> Option<(Bounds<Pixels>, Option<Bounds<Pixels>>)> {
        let rail = self
            .drawn
            .rail_bounds()
            .filter(|_| !self.drawn.rail_in_title.get())?;
        let gap = self
            .drawn
            .buttons
            .borrow()
            .iter()
            .position(|id| id.0.is_empty());
        let foot_top = gap
            .and_then(|gap| self.drawn.rail.bounds_for_item(gap + 1))
            .map(|first| first.top());
        let Some(foot_top) = foot_top.filter(|top| *top > rail.top()) else {
            return Some((rail, None));
        };
        let mut top = rail;
        top.size.height = foot_top - rail.top();
        let mut foot = rail;
        foot.origin.y = foot_top;
        foot.size.height = rail.bottom() - foot_top;
        Some((top, Some(foot)))
    }

    /// Where `dragged` would drop on this side dock at `at`, and the
    /// indicator's bounds: another dock's panel into it, on its column or
    /// its rail's top group; its own panel's rail button, from the top
    /// group, onto its rail's foot group.
    fn side_target(
        &self,
        dragged: &DraggedDockTab,
        at: Point<Pixels>,
        dock: EntityId,
        cx: &App,
    ) -> Option<(DockDrop, Bounds<Pixels>)> {
        let group = self.transfer_group.filter(|_| !self.tabs)?;
        if dragged.group != Some(group) {
            return None;
        }
        let zones = self.rail_zones();
        if dragged.dock != dock {
            if self.handle(dragged.panel).is_some() {
                return None;
            }
            let column = self.drawn.column_bounds();
            let into = column
                .filter(|column| column.contains(&at))
                .or(zones.map(|(top, _)| top).filter(|top| top.contains(&at)))?;
            return Some((DockDrop::Into, into));
        }
        let handle = self.handle(dragged.panel)?;
        let foot = zones?.1.filter(|foot| foot.contains(&at))?;
        (!handle.on_rail_foot(cx)).then_some((DockDrop::Foot, foot))
    }

    /// Where `dragged` would drop at `at`, by the last frame's layout, for
    /// [`DockView::panel_dropped`]: another dock's panel, or this side
    /// dock's rail button on its foot.
    fn transfer_target(
        &self,
        dragged: &DraggedDockTab,
        at: Point<Pixels>,
        dock: EntityId,
        cx: &App,
    ) -> Option<DockDrop> {
        match self.tabs {
            true => self.foreign_tab_target(dragged, at, dock),
            false => Some(self.side_target(dragged, at, dock, cx)?.0),
        }
    }

    /// A drag moved over a side dock of a transfer group: whether where it
    /// would drop changed, for the dock to redraw its indicator.
    fn side_drag_moved(
        &self,
        event: &DragMoveEvent<DraggedDockTab>,
        dock: EntityId,
        cx: &App,
    ) -> bool {
        let target = self.side_target(event.drag(cx), event.event.position, dock, cx);
        let (drop, hint) = target.unzip();
        let moved = self.side_hint.replace(hint) != hint;
        self.hover.replace(drop) != drop || moved
    }

    /// The listener a side dock of a transfer group takes on its outermost
    /// element, which hears every move of a drag: its indicator follows.
    pub(super) fn on_side_drag_move<V: DockView>(
        &self,
        cx: &mut Context<V>,
    ) -> impl Fn(&DragMoveEvent<DraggedDockTab>, &mut Window, &mut App) + 'static {
        cx.listener(
            |view: &mut V, event: &DragMoveEvent<DraggedDockTab>, _, cx| {
                if view.dock().side_drag_moved(event, cx.entity_id(), cx) {
                    cx.notify();
                }
            },
        )
    }

    /// Over a side dock's column, or its rail, while anything is dragged:
    /// a drop there goes to [`DockView::panel_dropped`] if it's taken.
    pub(super) fn side_drop_layer<V: DockView>(
        &self,
        cx: &mut Context<V>,
    ) -> impl IntoElement + use<V> {
        let dropped = cx.listener(|view: &mut V, dragged: &DraggedDockTab, window, cx| {
            drop_foreign(view, *dragged, window, cx)
        });
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .when(cx.has_active_drag(), |layer| layer.on_drop(dropped))
    }

    /// A side dock's drop indicator, where a hovering drag would drop: its
    /// column, or its rail's group, tinted, over the window. None once no
    /// drag is active.
    pub(super) fn side_drop_indicator(&self, cx: &App) -> Option<AnyElement> {
        if !cx.has_active_drag() {
            self.hover.set(None);
            self.side_hint.set(None);
        }
        let hover = self.hover.get();
        self.drawn.drop_hint.set(hover);
        let hint = self.side_hint.get().filter(|_| hover.is_some())?;
        let tint = div()
            .relative()
            .w(hint.size.width)
            .h(hint.size.height)
            .bg(ui_theme(cx).drop_target.opacity(0.2))
            .child(record(&self.drawn.hint));
        let indicator = anchored().position(hint.origin).child(tint);
        Some(deferred(indicator).with_priority(1).into_any_element())
    }

    /// The drag a side dock of a transfer group's rail button `panel`
    /// carries, and its ghost's title.
    pub(super) fn rail_drag<V: DockView>(
        &self,
        panel: PanelId,
        cx: &mut Context<V>,
    ) -> Option<(DraggedDockTab, SharedString)> {
        self.transfer_group?;
        Some(self.tab_drag(panel, cx))
    }
}

/// `dragged` dropped on `view`'s dock, from another dock, or a side rail's
/// button on its foot: [`DockView::panel_dropped`] with where, if the dock
/// takes it there.
pub(super) fn drop_foreign<V: DockView>(
    view: &mut V,
    dragged: DraggedDockTab,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let at = window.mouse_position();
    let target = view
        .dock()
        .transfer_target(&dragged, at, cx.entity_id(), cx);
    view.dock().hover.set(None);
    view.dock().side_hint.set(None);
    if let Some(target) = target {
        view.panel_dropped(dragged.panel, dragged.dock, target, window, cx);
    }
    cx.notify();
}
