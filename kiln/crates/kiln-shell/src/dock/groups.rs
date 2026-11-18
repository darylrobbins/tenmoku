// SPDX-License-Identifier: Apache-2.0

//! A tabbed dock's groups: its panels split into tab strips laid side by
//! side along the dock's long axis, left to right at the bottom and top to
//! bottom at a side, each with its own active panel, sharing the column by
//! weights.
//!
//! Each registered panel is placed in one group, in tab order; one added
//! goes at the end of the first. A group is drawn while one of its placed
//! panels is open and it isn't collapsed ([`DockState::hide_panel`]), and
//! it goes only once it has no placed panel left. With one group drawn the
//! dock draws as a dock without groups does.
//!
//! A tab drags ([`DraggedDockTab`]) within its dock: onto a group's strip
//! at a slot, onto its content's centre to join it, or onto the outer
//! quarter of its content along the dock's axis to split a new group off
//! on that side ([`DockDrop`]). The border between two drawn groups drags
//! the weight between them; a double-click evens them all.

use std::collections::HashSet;

use gpui::{
    AnyElement, App, AppContext as _, Axis, Bounds, Context, CursorStyle, DragMoveEvent, EntityId,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, ScrollHandle, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use kiln_command::Value;
use kiln_l10n::tr;
use kiln_ui::{Role, TAB_HEIGHT, ui_theme};

use super::{
    DockPosition, DockState, DockView, Landmark, MIN_DOCK_HEIGHT, MIN_DOCK_WIDTH, NoGhost, PanelId,
};
use crate::pane::{DropZone, TabGhost, insertion, marker, record};

/// The grab area of the border between two groups, centred on its 1 px,
/// as [`kiln_ui::SplitPane`]'s.
const DIVIDER_HANDLE: f32 = 6.;

/// One group of a tabbed dock.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Group {
    /// Its placed panels, in tab order.
    pub(super) panels: Vec<PanelId>,
    /// The panel it shows while drawn.
    pub(super) active: Option<PanelId>,
    /// Its share of the column beside the other drawn groups'.
    pub(super) weight: f32,
    /// Hidden by [`DockState::hide_panel`], keeping its tabs.
    pub(super) collapsed: bool,
}

impl Group {
    pub(super) fn new(panels: Vec<PanelId>, active: Option<PanelId>, weight: f32) -> Self {
        Self {
            panels,
            active,
            weight,
            collapsed: false,
        }
    }
}

/// Where [`DockState::move_panel`] puts a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveTo {
    /// Into group `group`, at `index` among its open tabs once it's there,
    /// or after the last.
    Group { group: usize, index: usize },
    /// Into a new group just before group `group`.
    Before(usize),
    /// Into a new group just after group `group`.
    After(usize),
}

/// What a dragged dock tab does dropped where it hovers, as the drop
/// indicator shows it ([`DrawnDock::drop_hint`](super::DrawnDock::drop_hint)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockDrop {
    /// Splits a new group off before `group`: left of it at the bottom,
    /// above it at a side.
    SplitBefore(usize),
    /// Splits a new group off after `group`.
    SplitAfter(usize),
    /// Joins `group`, at the end of its strip.
    Join(usize),
    /// Into `group`'s strip at slot `index`, before the tab drawn there
    /// now: in the tab's own group, a reorder.
    Insert { group: usize, index: usize },
    /// Into this dock, a side dock without tabs: another dock's panel
    /// dropped on its column or its rail's top group ([`super::transfer`]).
    Into,
    /// Onto a side rail's foot group: its own panel's rail button dropped
    /// there, which a host maps to its bottom dock.
    Foot,
}

/// A dock's tab, or a side dock's rail button, being dragged: the dock's
/// view, its panel, and the dock's transfer group, if it has one
/// ([`DockState::transfer_group`]), which another dock of the group takes
/// its drop in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DraggedDockTab {
    pub dock: EntityId,
    pub panel: PanelId,
    pub group: Option<&'static str>,
}

/// The border before the `ix`th drawn group, as dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GroupDivider {
    dock: EntityId,
    ix: usize,
}

/// Where a border's drag started: the pointer, the groups drawn and their
/// weights.
pub(super) struct DividerPress {
    ix: usize,
    at: Point<Pixels>,
    groups: Vec<usize>,
    weights: Vec<f32>,
}

/// What the last frame drew of one group.
#[derive(Clone)]
pub(super) struct DrawnGroup {
    pub(super) group: usize,
    pub(super) tabs: Vec<PanelId>,
    /// The group: its strip's row, then its content.
    pub(super) cell: ScrollHandle,
    /// Its tab strip, which tracks each tab.
    pub(super) strip: ScrollHandle,
}

impl DrawnGroup {
    pub(super) fn content(&self) -> Option<Bounds<Pixels>> {
        self.cell.bounds_for_item(1)
    }

    /// Where each tab was laid out, scrolled as it showed.
    pub(super) fn tab_layout(&self) -> Vec<Bounds<Pixels>> {
        let scrolled = self.strip.offset();
        (0..self.tabs.len())
            .map_while(|ix| self.strip.bounds_for_item(ix))
            .map(|tab| Bounds::new(tab.origin + scrolled, tab.size))
            .collect()
    }
}

impl DockState {
    /// The axis the groups are laid along: across at the bottom, down at a
    /// side.
    pub(super) fn group_axis(&self) -> Axis {
        match self.position {
            DockPosition::Bottom => Axis::Horizontal,
            DockPosition::Left | DockPosition::Right => Axis::Vertical,
        }
    }

    /// The least a group gets along the axis.
    pub(super) fn min_group(&self) -> Pixels {
        match self.group_axis() {
            Axis::Horizontal => MIN_DOCK_WIDTH,
            Axis::Vertical => MIN_DOCK_HEIGHT,
        }
    }

    /// Whether `panel` has a tab: shown since and not closed, or the
    /// focused group's active panel while the dock shows, as a dock built
    /// shown shows its first.
    pub(super) fn is_open(&self, panel: PanelId) -> bool {
        self.opened.contains(&panel) || self.visible && self.active() == Some(panel)
    }

    /// How many groups the dock has, drawn or not.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// The group `panel` is placed in, if it's registered.
    pub fn group_of(&self, panel: PanelId) -> Option<usize> {
        (self.groups.iter()).position(|group| group.panels.contains(&panel))
    }

    /// The panels with tabs in `group`, in tab order.
    pub fn open_panels_in(&self, group: usize) -> Vec<PanelId> {
        (self.groups.get(group).into_iter())
            .flat_map(|group| group.panels.iter().copied())
            .filter(|&id| self.is_open(id))
            .collect()
    }

    /// Whether `group` is collapsed: not drawn, by [`DockState::hide_panel`],
    /// but keeping its tabs and active panel.
    pub fn is_collapsed(&self, group: usize) -> bool {
        self.groups.get(group).is_some_and(|group| group.collapsed)
    }

    /// Whether `group` is drawn: the dock shows, it isn't collapsed and it
    /// has a tab.
    fn is_drawn(&self, group: usize) -> bool {
        self.visible && !self.is_collapsed(group) && !self.open_panels_in(group).is_empty()
    }

    /// The groups drawn, in order.
    pub(super) fn drawn_groups(&self) -> Vec<usize> {
        (0..self.groups.len())
            .filter(|&group| self.is_drawn(group))
            .collect()
    }

    /// The panel `group` shows, if it's drawn: its active panel, or its
    /// first tab if that has none.
    pub(super) fn group_shown(&self, group: usize) -> Option<PanelId> {
        if !self.is_drawn(group) {
            return None;
        }
        let open = self.open_panels_in(group);
        let active = self.groups[group].active.filter(|id| open.contains(id));
        active.or(open.first().copied())
    }

    /// Each drawn group's active panel, in order.
    pub fn shown_panels(&self) -> Vec<PanelId> {
        (self.drawn_groups().into_iter())
            .filter_map(|group| self.group_shown(group))
            .collect()
    }

    /// Whether `panel` shows: a drawn group's active panel.
    pub fn is_shown(&self, panel: PanelId) -> bool {
        (self.group_of(panel)).is_some_and(|group| self.group_shown(group) == Some(panel))
    }

    /// Makes `group` the focused one, the one it was's active panel's tab
    /// kept.
    fn focus_group(&mut self, group: usize) {
        self.open_shown();
        self.move_focus(group);
    }

    /// Makes `group` the focused one, its active panel the one it shows,
    /// so the dock's is.
    pub(super) fn move_focus(&mut self, group: usize) {
        if let Some(shown) = self.group_shown(group) {
            self.groups[group].active = Some(shown);
        }
        self.focused = group;
    }

    /// The drawn group nearest `group` among `drawn`: the one before it,
    /// else the one after.
    pub(super) fn nearest(drawn: &[usize], group: usize) -> Option<usize> {
        let before = drawn.iter().rev().find(|&&other| other < group);
        before
            .or(drawn.iter().find(|&&other| other > group))
            .copied()
    }

    /// Places `panel`, added anew, at the end of the first group.
    pub(super) fn place(&mut self, panel: PanelId) {
        if self.group_of(panel).is_some() {
            return;
        }
        if self.groups.is_empty() {
            self.groups.push(Group::new(Vec::new(), None, 1.));
        }
        let first = &mut self.groups[0];
        first.panels.push(panel);
        first.active.get_or_insert(panel);
    }

    /// Hides `panel` for the host's foot buttons and toggles: if it's a
    /// drawn group's active panel and another group is drawn, its group
    /// collapses, keeping its tabs, active panel and weight, and focus
    /// moves to the nearest drawn group; if its group is the only one
    /// drawn, the dock hides ([`DockState::set_visible`]). False, doing
    /// nothing, if it isn't shown.
    pub fn hide_panel(&mut self, panel: PanelId) -> bool {
        let Some(group) = self.group_of(panel).filter(|_| self.is_shown(panel)) else {
            return false;
        };
        let others: Vec<_> = (self.drawn_groups().into_iter())
            .filter(|&other| other != group)
            .collect();
        let Some(nearest) = Self::nearest(&others, group) else {
            self.set_visible(false);
            return true;
        };
        // Its tab stays as it stops being the focused group's.
        self.open_shown();
        self.groups[group].active = Some(panel);
        self.groups[group].collapsed = true;
        if self.focused == group {
            self.move_focus(nearest);
        }
        true
    }

    /// Closes `panel`'s tab, in its group: if it was the group's active
    /// panel, the next open one there shows in its place, else the one
    /// before. With none left there, the group hides, keeping the panel's
    /// place for it to show in again, and focus moves to the nearest
    /// drawn group; with no group left drawn, the dock hides. False if it
    /// had no tab.
    pub fn close_tab(&mut self, panel: PanelId) -> bool {
        let Some(group) = self.group_of(panel) else {
            return false;
        };
        let open = self.open_panels_in(group);
        let Some(ix) = open.iter().position(|id| *id == panel) else {
            return false;
        };
        let others: Vec<_> = (self.drawn_groups().into_iter())
            .filter(|&other| other != group)
            .collect();
        self.opened.remove(&panel);
        if self.groups[group].active != Some(panel) {
            return true;
        }
        let next = open
            .get(ix + 1)
            .or(ix.checked_sub(1).and_then(|ix| open.get(ix)));
        match (next, Self::nearest(&others, group)) {
            (Some(&next), _) => self.groups[group].active = Some(next),
            (None, None) => self.set_visible(false),
            (None, Some(nearest)) => {
                if self.focused == group {
                    self.move_focus(nearest);
                }
            }
        }
        true
    }

    /// Moves `panel` to `to`, active there, its group focused, and opens
    /// it: for a tab's drag and the host's commands. Moving a group's last
    /// placed panel out takes that group away. False, doing nothing, if the
    /// dock has no tabs, `panel` isn't registered, `to` names no group, or
    /// it would put a group's only panel in a new group beside itself.
    pub fn move_panel(&mut self, panel: PanelId, to: MoveTo) -> bool {
        let Some(from) = self.group_of(panel).filter(|_| self.tabs) else {
            return false;
        };
        let (MoveTo::Group { group: target, .. } | MoveTo::Before(target) | MoveTo::After(target)) =
            to;
        let alone = self.groups[from].panels == [panel];
        let beside = matches!(to, MoveTo::Before(_) | MoveTo::After(_));
        if target >= self.groups.len() || alone && beside && target == from {
            return false;
        }
        self.open_shown();
        // Its group shows the tab beside it in its place.
        let open = self.open_panels_in(from);
        let group = &mut self.groups[from];
        group.panels.retain(|id| *id != panel);
        if group.active == Some(panel) {
            let ix = open.iter().position(|id| *id == panel);
            let next = ix.and_then(|ix| open.get(ix + 1).or(ix.checked_sub(1).map(|ix| &open[ix])));
            group.active = next.copied().or(group.panels.first().copied());
        }
        let (from, target) = match to {
            MoveTo::Group { group, index } => {
                let open = self.open_panels_in(group);
                let placed = &mut self.groups[group].panels;
                let at = match open.get(index) {
                    Some(before) => placed.iter().position(|id| id == before),
                    None => (open.last())
                        .and_then(|last| placed.iter().position(|id| id == last))
                        .map(|ix| ix + 1),
                };
                placed.insert(at.unwrap_or(placed.len()), panel);
                (from, group)
            }
            MoveTo::Before(group) | MoveTo::After(group) => {
                let at = group + usize::from(matches!(to, MoveTo::After(_)));
                // The new group takes half of the one it splits off.
                self.groups[group].weight /= 2.;
                let weight = self.groups[group].weight;
                let new = Group::new(vec![panel], Some(panel), weight);
                self.groups.insert(at, new);
                (from + usize::from(at <= from), at)
            }
        };
        let mut focused = target;
        if self.groups[from].panels.is_empty() {
            self.groups.remove(from);
            focused -= usize::from(from < focused);
        }
        let group = &mut self.groups[focused];
        group.active = Some(panel);
        group.collapsed = false;
        self.opened.insert(panel);
        self.focused = focused;
        true
    }

    /// Puts every panel back in one group, the groups' tabs left to right
    /// (top to bottom) in their order, the focused group's active panel
    /// active.
    pub fn join_groups(&mut self) {
        self.open_shown();
        let active = self.active();
        let panels = (self.groups.drain(..))
            .flat_map(|group| group.panels)
            .collect();
        self.groups = vec![Group::new(panels, active, 1.)];
        self.focused = 0;
    }

    /// Focuses the drawn group `by` after the focused one, wrapping, and
    /// the panel it shows: that panel, or None with no group drawn.
    pub fn focus_next_group(
        &mut self,
        by: isize,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<PanelId> {
        let drawn = self.drawn_groups();
        let at = drawn.iter().position(|&group| group == self.focused);
        let count = drawn.len() as isize;
        let next = drawn[(at? as isize + by).rem_euclid(count) as usize];
        let panel = self.group_shown(next)?;
        self.focus_group(next);
        let focus = self.handle(panel)?.focus_handle(cx);
        window.focus(&focus, cx);
        Some(panel)
    }

    /// Makes the drawn group whose shown panel focus has moved to the
    /// focused one, for focus that came to it some other way than through
    /// the dock, as a key or the host gives it: only as focus moves, so a
    /// panel shown elsewhere since keeps its group focused while focus
    /// stays where it was. True if it changed. [`Dock`](super::Dock) calls
    /// it as it draws.
    pub fn follow_focus(&mut self, window: &Window, cx: &App) -> bool {
        let focused = (self.panels.iter())
            .find(|panel| panel.focus_handle(cx).contains_focused(window, cx))
            .map(|panel| panel.id());
        if std::mem::replace(&mut self.followed, focused) == focused {
            return false;
        }
        let group = focused
            .filter(|&panel| self.is_shown(panel))
            .and_then(|panel| self.group_of(panel));
        match group {
            Some(group) if group != self.focused => {
                self.focus_group(group);
                true
            }
            _ => false,
        }
    }

    /// Whether the groups are as a dock that was never split has them: one,
    /// of its panels in the order added, not collapsed.
    fn is_default_layout(&self) -> bool {
        match &self.groups[..] {
            [] => true,
            [group] => !group.collapsed && group.panels == self.panel_ids(),
            _ => false,
        }
    }

    /// The groups, for [`DockState::save`], unless they're as by default:
    /// each one's panels in tab order, its active panel (none if it has no
    /// tab), its weight and whether it's collapsed.
    pub(super) fn save_groups(&self) -> Option<Value> {
        if self.is_default_layout() {
            return None;
        }
        let groups = (self.groups.iter().enumerate()).map(|(ix, group)| {
            let open = self.open_panels_in(ix);
            let active = group.active.filter(|id| open.contains(id));
            let active = active.or(open.first().copied());
            Value::from_iter([
                (
                    "panels",
                    Value::from_iter(group.panels.iter().map(|id| id.0)),
                ),
                ("active", active.map_or(Value::Null, |id| id.0.into())),
                ("weight", Value::from(f64::from(group.weight))),
                ("collapsed", Value::from(group.collapsed)),
            ])
        });
        Some(Value::from_iter(groups))
    }

    /// Puts back the groups [`DockState::save_groups`] saved, for the
    /// registered panels: unknown ids and groups left empty go, the panels
    /// it doesn't place go at the end of the first group, and each group's
    /// active panel opens, the first group not collapsed focused. Saved
    /// without groups, the dock has one, of its panels in the order added,
    /// its active panel as it was. True if it put back saved groups.
    pub(super) fn restore_groups(&mut self, saved: Option<&Value>) -> bool {
        let active = self.active();
        let registered = self.panel_ids();
        let find = |name: &str| registered.iter().copied().find(|id| id.0 == name);
        let mut placed = HashSet::new();
        let mut groups = Vec::new();
        for entry in saved.and_then(Value::as_array).into_iter().flatten() {
            let panels: Vec<PanelId> = (entry.get("panels").and_then(Value::as_array))
                .into_iter()
                .flatten()
                .filter_map(|id| find(id.as_str()?))
                .filter(|&id| placed.insert(id))
                .collect();
            if panels.is_empty() {
                continue;
            }
            let active = (entry.get("active").and_then(Value::as_str))
                .and_then(find)
                .filter(|id| panels.contains(id));
            if let Some(active) = active {
                self.opened.insert(active);
            }
            let weight = (entry.get("weight").and_then(Value::as_f64))
                .map(|weight| weight as f32)
                .filter(|weight| weight.is_finite() && *weight > 0.)
                .unwrap_or(1.);
            let mut group = Group::new(panels.clone(), active.or(panels.first().copied()), weight);
            group.collapsed = (entry.get("collapsed").and_then(Value::as_bool)).unwrap_or(false);
            groups.push(group);
        }
        let restored = !groups.is_empty();
        if !restored {
            groups.push(Group::new(Vec::new(), active, 1.));
        }
        let unplaced = registered.iter().filter(|id| !placed.contains(id));
        groups[0].panels.extend(unplaced);
        if groups[0].active.is_none() {
            groups[0].active = groups[0].panels.first().copied();
        }
        self.groups = groups;
        // Saved groups keep the active panels and collapse they were saved
        // with: the one before the restore is the default's alone.
        self.focused = match restored {
            true => (self.groups.iter().position(|group| !group.collapsed)).unwrap_or(0),
            false => 0,
        };
        if let Some(active) = active.filter(|_| !restored) {
            self.set_active(active);
        }
        restored
    }

    /// Makes `panel`, the saved dock's active panel, its group's active
    /// panel and that group the focused one, unless the group was saved
    /// collapsed, which stays as it was.
    pub(super) fn restore_active(&mut self, panel: PanelId) {
        if let Some(group) = self
            .group_of(panel)
            .filter(|&group| !self.is_collapsed(group))
        {
            self.groups[group].active = Some(panel);
            self.focused = group;
        }
    }

    /// Makes `panel` its group's active panel and that group the focused
    /// one, drawn, without opening it or focusing it.
    pub(super) fn set_active(&mut self, panel: PanelId) {
        if let Some(group) = self.group_of(panel) {
            self.groups[group].active = Some(panel);
            self.groups[group].collapsed = false;
            self.focused = group;
        }
    }

    /// Where `dragged`, a tab of the dock `dock`, would drop at `at`, by the
    /// last frame's layout: none where the drop would change nothing, where
    /// a split would leave a half narrower (shorter) than the least, or if
    /// it's another dock's.
    pub(super) fn drop_target(
        &self,
        dragged: &DraggedDockTab,
        at: Point<Pixels>,
        dock: EntityId,
    ) -> Option<DockDrop> {
        if dragged.dock != dock {
            return self.foreign_tab_target(dragged, at, dock);
        }
        let from = self.group_of(dragged.panel)?;
        let drawn = self.drawn.groups.borrow();
        let hovered = drawn
            .iter()
            .find(|drawn| drawn.cell.bounds().contains(&at))?;
        let group = hovered.group;
        let own = self.open_panels_in(from);
        let alone = own == [dragged.panel];
        if hovered.strip.bounds().contains(&at) {
            let slot = insertion(&hovered.tab_layout(), at.x);
            let ix = (group == from)
                .then(|| hovered.tabs.iter().position(|id| *id == dragged.panel))
                .flatten();
            if ix.is_some_and(|ix| slot == ix || slot == ix + 1) {
                return None;
            }
            return Some(DockDrop::Insert { group, index: slot });
        }
        let content = hovered.content().filter(|content| content.contains(&at))?;
        let axis = self.group_axis();
        let drop = match DropZone::along(content, at, axis) {
            DropZone::Centre => return (group != from).then_some(DockDrop::Join(group)),
            DropZone::Left | DropZone::Up => DockDrop::SplitBefore(group),
            DropZone::Right | DropZone::Down => DockDrop::SplitAfter(group),
        };
        // A group's only tab split off beside where it is changes nothing.
        let next_to = |group: usize, after: bool| {
            let drawn: Vec<_> = drawn.iter().map(|drawn| drawn.group).collect();
            let ix = drawn.iter().position(|&other| other == group);
            let beside = match after {
                true => ix.and_then(|ix| drawn.get(ix + 1)),
                false => ix.and_then(|ix| ix.checked_sub(1)).map(|ix| &drawn[ix]),
            };
            beside == Some(&from)
        };
        let unchanged = match drop {
            DockDrop::SplitBefore(group) => group == from || next_to(group, false),
            _ => group == from || next_to(group, true),
        };
        if alone && unchanged {
            return None;
        }
        let length = match axis {
            Axis::Horizontal => hovered.cell.bounds().size.width,
            Axis::Vertical => hovered.cell.bounds().size.height,
        };
        (length / 2. >= self.min_group()).then_some(drop)
    }

    /// A dock tab dragged over the dock moved: whether where it would drop
    /// changed, for the dock to redraw its indicator.
    fn tab_drag_moved(
        &self,
        event: &DragMoveEvent<DraggedDockTab>,
        dock: EntityId,
        cx: &App,
    ) -> bool {
        let target = self.drop_target(event.drag(cx), event.event.position, dock);
        self.hover.replace(target) != target
    }

    /// `dragged`, a tab of the dock view `dock`, dropped at `at`, by the
    /// last frame's layout: moves it as its drop indicator showed, a split,
    /// a join or a move along a strip, or does nothing where it showed
    /// none. True if it moved. For [`DockView::tab_dropped`].
    pub fn drop_tab(
        &mut self,
        dragged: &DraggedDockTab,
        at: Point<Pixels>,
        dock: EntityId,
    ) -> bool {
        self.hover.set(None);
        let Some(target) = self.drop_target(dragged, at, dock) else {
            return false;
        };
        let panel = dragged.panel;
        let to = match target {
            DockDrop::SplitBefore(group) => MoveTo::Before(group),
            DockDrop::SplitAfter(group) => MoveTo::After(group),
            DockDrop::Join(group) => MoveTo::Group {
                group,
                index: usize::MAX,
            },
            DockDrop::Insert { group, index } => {
                let own = (self.group_of(panel) == Some(group))
                    .then(|| {
                        self.open_panels_in(group)
                            .iter()
                            .position(|id| *id == panel)
                    })
                    .flatten();
                let index = match own {
                    Some(ix) if ix < index => index - 1,
                    _ => index,
                };
                MoveTo::Group { group, index }
            }
            DockDrop::Into | DockDrop::Foot => return false,
        };
        self.move_panel(panel, to)
    }

    /// The listeners a tabbed dock's column takes for its tabs' drags: the
    /// drop indicator follows a drag over it.
    pub(super) fn on_tab_drag_move<V: DockView>(
        &self,
        cx: &mut Context<V>,
    ) -> impl Fn(&DragMoveEvent<DraggedDockTab>, &mut Window, &mut App) + 'static {
        cx.listener(
            |view: &mut V, event: &DragMoveEvent<DraggedDockTab>, _, cx| {
                if view.dock().tab_drag_moved(event, cx.entity_id(), cx) {
                    cx.notify();
                }
            },
        )
    }

    /// Over the column while anything is dragged, for a dock tab's drop to
    /// reach it before the panels' own mouse handlers: the last child but
    /// the indicator.
    pub(super) fn drop_layer<V: DockView>(&self, cx: &mut Context<V>) -> impl IntoElement + use<V> {
        // Another dock's panel goes to the host ([`DockView::panel_dropped`]).
        let dropped =
            cx.listener(|view: &mut V, dragged: &DraggedDockTab, window, cx| {
                match dragged.dock == cx.entity_id() {
                    true => view.tab_dropped(*dragged, window, cx),
                    false => super::transfer::drop_foreign(view, *dragged, window, cx),
                }
            });
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(record(&self.drawn.layer))
            .when(cx.has_active_drag(), |layer| layer.on_drop(dropped))
    }

    /// The drop indicator for where a hovering tab would drop, by the last
    /// frame's layout, as editor panes draw theirs: the half of the group
    /// a split would fill, tinted, the content a join would go to, or the
    /// strip's insertion marker. None once no drag is active. Read before
    /// the frame's groups are drawn.
    pub(super) fn drop_indicator(&self, cx: &App) -> Option<AnyElement> {
        if !cx.has_active_drag() {
            self.hover.set(None);
        }
        let hover = self.hover.get();
        self.drawn.drop_hint.set(hover);
        let hover = hover?;
        let ui = ui_theme(cx);
        let group = match hover {
            DockDrop::SplitBefore(group)
            | DockDrop::SplitAfter(group)
            | DockDrop::Join(group)
            | DockDrop::Insert { group, .. } => group,
            // A side dock's, drawn by it ([`super::transfer`]).
            DockDrop::Into | DockDrop::Foot => return None,
        };
        let drawn = self.drawn.groups.borrow();
        let drawn = drawn.iter().find(|drawn| drawn.group == group)?;
        let (cell, content) = (drawn.cell.bounds(), drawn.content()?);
        let horizontal = self.group_axis() == Axis::Horizontal;
        let (hint, fill) = match hover {
            DockDrop::Insert { index, .. } => {
                let strip = drawn.strip.bounds();
                (marker(&drawn.tab_layout(), index, strip), ui.drop_target)
            }
            DockDrop::Join(_) => (content, ui.drop_target.opacity(0.2)),
            DockDrop::Into | DockDrop::Foot => return None,
            DockDrop::SplitBefore(_) | DockDrop::SplitAfter(_) => {
                let before = matches!(hover, DockDrop::SplitBefore(_));
                let zone = match (horizontal, before) {
                    (true, true) => DropZone::Left,
                    (true, false) => DropZone::Right,
                    (false, true) => DropZone::Up,
                    (false, false) => DropZone::Down,
                };
                (zone.overlay(cell, content), ui.drop_target.opacity(0.2))
            }
        };
        let origin = self.drawn.layer.get().origin;
        let element = div()
            .absolute()
            .left(hint.left() - origin.x)
            .top(hint.top() - origin.y)
            .w(hint.size.width)
            .h(hint.size.height)
            .bg(fill)
            .child(record(&self.drawn.hint));
        Some(element.into_any_element())
    }

    /// The drag value and ghost a dock tab carries.
    pub(super) fn tab_drag<V: DockView>(
        &self,
        panel: PanelId,
        cx: &mut Context<V>,
    ) -> (DraggedDockTab, gpui::SharedString) {
        let title = (self.handle(panel)).map_or_else(Default::default, |handle| handle.title(cx));
        let dragged = DraggedDockTab {
            dock: cx.entity_id(),
            panel,
            group: self.transfer_group,
        };
        (dragged, title)
    }

    /// The ghost a dragged dock tab draws at the pointer.
    pub(super) fn tab_ghost(title: gpui::SharedString, cx: &mut App) -> gpui::Entity<TabGhost> {
        cx.new(|_| TabGhost(title))
    }

    /// A press on the border before the `ix`th drawn group: the start of
    /// a drag, or, pressed twice, evens the groups out.
    fn press_divider(&mut self, ix: usize, event: &MouseDownEvent) {
        if event.click_count == 2 {
            self.groups.iter_mut().for_each(|group| group.weight = 1.);
        }
        let groups = self.drawn_groups();
        let weights = (groups.iter())
            .map(|&group| self.groups[group].weight)
            .collect();
        self.divider_press = Some(DividerPress {
            ix,
            at: event.position,
            groups,
            weights,
        });
    }

    /// Moves the border before the `ix`th drawn group with the pointer, now
    /// at `to`, in the groups' row laid out in `bounds`: only the two
    /// groups either side change, each kept at least the least, and if they
    /// don't fit it, neither does.
    fn drag_divider(&mut self, ix: usize, to: Point<Pixels>, bounds: Bounds<Pixels>) {
        let Some(press) = (self.divider_press.as_ref())
            .filter(|press| press.ix == ix && press.groups == self.drawn_groups())
        else {
            return;
        };
        let (Some(a), b) = (ix.checked_sub(1), ix) else {
            return;
        };
        if b >= press.groups.len() {
            return;
        }
        let (length, moved) = match self.group_axis() {
            Axis::Horizontal => (bounds.size.width, to.x - press.at.x),
            Axis::Vertical => (bounds.size.height, to.y - press.at.y),
        };
        // Each border's pixel is the group's after it.
        let space = f32::from(length) - (press.groups.len() - 1) as f32;
        let total: f32 = press.weights.iter().sum();
        let pair = press.weights[a] + press.weights[b];
        let min = f32::from(self.min_group());
        let max = pair / total * space - min;
        if space <= 0. || min > max {
            return;
        }
        let size = (press.weights[a] / total * space + f32::from(moved)).clamp(min, max);
        let weight = size / space * total;
        let (ga, gb) = (press.groups[a], press.groups[b]);
        self.groups[ga].weight = weight;
        self.groups[gb].weight = pair - weight;
    }

    /// The border's handle before the `ix`th drawn group, over its 1 px and
    /// a little either side, from inside that group's cell: pressing it
    /// starts a drag (twice evens the groups), which draws nothing but the
    /// cursor.
    fn divider_handle<V: DockView>(
        &self,
        ix: usize,
        before: usize,
        cx: &mut Context<V>,
    ) -> Landmark {
        let handle = ScrollHandle::new();
        self.drawn
            .dividers
            .borrow_mut()
            .push((before, handle.clone()));
        let offset = px(-(DIVIDER_HANDLE - 1.) / 2. - 1.);
        let cursor = match self.group_axis() {
            Axis::Horizontal => CursorStyle::ResizeLeftRight,
            Axis::Vertical => CursorStyle::ResizeUpDown,
        };
        self.drawn.divider_cursor.set(Some(cursor));
        let drag = GroupDivider {
            dock: cx.entity_id(),
            ix,
        };
        let element = div()
            .id(("dock-group-divider", ix))
            .absolute()
            // Over the groups either side, for presses, but not for drops.
            .when(!cx.has_active_drag(), |handle| handle.occlude())
            .map(|handle| match self.group_axis() {
                Axis::Horizontal => handle.top_0().bottom_0().left(offset).w(px(DIVIDER_HANDLE)),
                Axis::Vertical => handle.left_0().right_0().top(offset).h(px(DIVIDER_HANDLE)),
            })
            .cursor(cursor)
            .track_scroll(&handle)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view: &mut V, event: &MouseDownEvent, _, cx| {
                    view.dock_mut().press_divider(ix, event);
                    cx.notify();
                }),
            )
            .on_drag(drag, |_, _, _, cx| cx.new(|_| NoGhost));
        Landmark {
            element,
            role: Role::Splitter,
            label: tr!("kiln-shell-dock-group-divider"),
        }
    }

    /// Two or more drawn groups, in a row along the dock's axis, each its
    /// strip, ending in its active panel's actions, over its active panel,
    /// the dock-wide controls at the end of the last strip, and a border
    /// that drags between each two. A bottom dock's rail, if it draws one,
    /// leads the first strip.
    pub(super) fn groups_row<V: DockView>(
        &self,
        drawn: &[usize],
        mut rail: Option<AnyElement>,
        cx: &mut Context<V>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let horizontal = self.group_axis() == Axis::Horizontal;
        let left = self.position == DockPosition::Left;
        let total = drawn.len();
        // Shares of the drawn groups' weights, which may sum to anything.
        let weights: f32 = drawn.iter().map(|&group| self.groups[group].weight).sum();
        let cells: Vec<_> = (drawn.iter().enumerate())
            .filter_map(|(ix, &group)| {
                let id = self.group_shown(group)?;
                let panel = self.handle(id)?;
                let controls = self.controls(panel, ix + 1 == total, cx);
                let strip = match ix {
                    0 => self.drawn.tab_strip.clone(),
                    _ => {
                        let mut strips = self.drawn.strips.borrow_mut();
                        while strips.len() < ix {
                            strips.push(ScrollHandle::new());
                        }
                        strips[ix - 1].clone()
                    }
                };
                let tabs = self.tab_row(group, panel, controls, &strip, cx);
                let header = match self.position {
                    DockPosition::Bottom => div()
                        .flex_none()
                        .flex()
                        .h(TAB_HEIGHT)
                        .bg(ui.panel_background)
                        .children(rail.take())
                        .child(tabs.flex_1().min_w_0()),
                    _ if left => tabs.border_r_1(),
                    _ => tabs.border_l_1(),
                };
                let cell = ScrollHandle::new();
                self.drawn.groups.borrow_mut().push(DrawnGroup {
                    group,
                    tabs: self.open_panels_in(group),
                    cell: cell.clone(),
                    strip,
                });
                let title = panel.title(cx);
                let element = div()
                    .id(("dock-group", ix))
                    .relative()
                    .flex_basis(px(0.))
                    .flex_grow(self.groups[group].weight / weights)
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .border_color(ui.border)
                    .when(ix > 0 && horizontal, |cell| cell.border_l_1())
                    .when(ix > 0 && !horizontal, |cell| cell.border_t_1())
                    .track_scroll(&cell)
                    // A press in it focuses its group.
                    .capture_any_mouse_down(cx.listener(move |view: &mut V, _, _, cx| {
                        let dock = view.dock();
                        if let Some(group) = dock.group_of(id).filter(|&g| g != dock.focused) {
                            view.dock_mut().focus_group(group);
                            cx.notify();
                        }
                    }))
                    .child(header)
                    .child(div().flex_1().min_h_0().flex().child(panel.to_any_view()))
                    .when(ix > 0, |cell| {
                        cell.child(self.divider_handle(ix, drawn[ix - 1], cx))
                    });
                Some(Landmark {
                    element,
                    role: Role::Group,
                    label: title,
                })
            })
            .collect();
        let dock = cx.entity_id();
        div()
            .id("dock-groups")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .when(!horizontal, |row| row.flex_col())
            .on_drag_move(cx.listener(
                move |view: &mut V, event: &DragMoveEvent<GroupDivider>, _, cx| {
                    let divider = *event.drag(cx);
                    if divider.dock == dock {
                        let at = event.event.position;
                        view.dock_mut().drag_divider(divider.ix, at, event.bounds);
                        cx.notify();
                    }
                },
            ))
            .children(cells)
            .into_any_element()
    }
}
