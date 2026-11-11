// SPDX-License-Identifier: Apache-2.0

//! A group of panes: a tree of them, split side by side or one above
//! another. Members of an axis share its space by ratio, with a 1 px border
//! between them, and a handle on each border drags it. Its panes are any
//! [`PaneView`]: kiln's [`ItemPane`](crate::ItemPane), or a host's own.
//!
//! A tab moves between panes only through [`PaneGroup::move_tab`], for a
//! key or a drop, which can split a new pane off for it.
//!
//! One pane can be zoomed ([`PaneGroup::zoom`]): drawn alone over the
//! group's whole area while the others keep their items and ratios, until
//! the tree changes or the zoom is ended.

use std::{
    cell::RefCell,
    collections::HashMap,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, CursorStyle, DragMoveEvent, Entity,
    InteractiveElement as _, IntoElement as _, MouseButton, MouseDownEvent, ParentElement as _,
    Pixels, Point, Render, Size, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window,
    canvas, div, prelude::FluentBuilder as _, px,
};
use kiln_ui::{UiTheme, ui_theme};

use crate::{
    ItemHandle, ItemId, PaneView,
    close::{self, CloseKind, CloseRequest, Closing, Items, LiveItems},
};

/// The narrowest a pane gets by dragging a border.
pub const MIN_PANE_WIDTH: Pixels = px(120.);
/// The shortest a pane gets by dragging a border.
pub const MIN_PANE_HEIGHT: Pixels = px(80.);
/// Between two members of an axis.
const BORDER: f32 = 1.;
/// How far across a border's handle is, centred on it.
const HANDLE: f32 = 5.;

/// How an axis lays its members out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Side by side, left to right.
    Horizontal,
    /// One above another, top to bottom.
    Vertical,
}

/// A way to go from a pane: where a split, focus or move goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// The axis this direction runs along.
    pub fn axis(self) -> Axis {
        match self {
            Direction::Left | Direction::Right => Axis::Horizontal,
            Direction::Up | Direction::Down => Axis::Vertical,
        }
    }

    /// Whether it goes towards the axis's end (right or down).
    pub fn forward(self) -> bool {
        matches!(self, Direction::Right | Direction::Down)
    }
}

/// A member of the tree.
pub enum Member<V> {
    Pane(Entity<V>),
    Axis { axis: Axis, members: Members<V> },
}

/// An axis's members, each with its share of the axis: their ratios, which
/// sum to 1. A new axis shares equally.
pub struct Members<V> {
    members: Vec<Member<V>>,
    ratios: Vec<f32>,
}

impl<V> Members<V> {
    fn new(members: Vec<Member<V>>) -> Self {
        let ratios = vec![1. / members.len() as f32; members.len()];
        Self { members, ratios }
    }

    /// Puts `member` at `at`, beside member `of`, with half of its share.
    fn insert_beside(&mut self, of: usize, at: usize, member: Member<V>) {
        self.ratios[of] /= 2.;
        self.ratios.insert(at, self.ratios[of]);
        self.members.insert(at, member);
    }

    /// Removes member `ix`, giving its share to the one before it, or after
    /// it if it was first.
    fn remove(&mut self, ix: usize) -> Member<V> {
        let share = self.ratios.remove(ix);
        if let Some(heir) = self.ratios.get_mut(ix.saturating_sub(1)) {
            *heir += share;
        }
        self.members.remove(ix)
    }

    /// Each member's share of the axis.
    pub fn ratios(&self) -> &[f32] {
        &self.ratios
    }

    /// Replaces member `ix`, an axis, with its members, which share its
    /// share as they shared it.
    fn splice(&mut self, ix: usize) {
        let share = self.ratios.remove(ix);
        let Member::Axis { members: inner, .. } = self.members.remove(ix) else {
            unreachable!("only an axis splices");
        };
        let ratios = inner.ratios.iter().map(|ratio| ratio * share);
        self.ratios.splice(ix..ix, ratios);
        self.members.splice(ix..ix, inner.members);
    }
}

/// Members can be read and changed in place, but not added or removed but
/// by `Members`' own methods, so each keeps its ratio.
impl<V> Deref for Members<V> {
    type Target = [Member<V>];

    fn deref(&self) -> &[Member<V>] {
        &self.members
    }
}

impl<V> DerefMut for Members<V> {
    fn deref_mut(&mut self) -> &mut [Member<V>] {
        &mut self.members
    }
}

impl<V> Member<V> {
    /// An axis of `members` sharing it by `ratios`, scaled to sum to 1;
    /// equally if they don't fit (a ratio for each, all positive). One
    /// member is that member.
    pub fn axis(axis: Axis, members: Vec<Member<V>>, ratios: Vec<f32>) -> Self {
        let mut members = Members::new(members);
        let sum: f32 = ratios.iter().sum();
        let fits = ratios.len() == members.len()
            && ratios.iter().all(|ratio| ratio.is_finite() && *ratio > 0.);
        if fits {
            members.ratios = ratios.iter().map(|ratio| ratio / sum).collect();
        }
        if members.len() == 1 {
            return members.members.remove(0);
        }
        Member::Axis { axis, members }
    }
}

impl<V: PaneView> Member<V> {
    fn panes<'a>(&'a self, out: &mut Vec<&'a Entity<V>>) {
        match self {
            Member::Pane(pane) => out.push(pane),
            Member::Axis { members, .. } => members.iter().for_each(|member| member.panes(out)),
        }
    }

    fn contains(&self, pane: &Entity<V>) -> bool {
        match self {
            Member::Pane(member) => member == pane,
            Member::Axis { members, .. } => members.iter().any(|member| member.contains(pane)),
        }
    }

    /// Puts `new` beside `old` in `direction`: as a sibling in an axis that
    /// runs that way, or else in a new axis nested in `old`'s place.
    fn split(&mut self, old: &Entity<V>, new: Entity<V>, direction: Direction) {
        let axis = direction.axis();
        let (pair_axis, members) = match self {
            Member::Pane(pane) if pane == old => {
                let old = Member::Pane(pane.clone());
                let pair = if direction.forward() {
                    vec![old, Member::Pane(new)]
                } else {
                    vec![Member::Pane(new), old]
                };
                *self = Member::Axis {
                    axis,
                    members: Members::new(pair),
                };
                return;
            }
            Member::Pane(_) => return,
            Member::Axis { axis, members } => (*axis, members),
        };
        let Some(ix) = members.iter().position(|member| member.contains(old)) else {
            return;
        };
        if pair_axis == axis && matches!(&members[ix], Member::Pane(pane) if pane == old) {
            let at = if direction.forward() { ix + 1 } else { ix };
            members.insert_beside(ix, at, Member::Pane(new));
        } else {
            members[ix].split(old, new, direction);
        }
    }

    /// Removes `pane`, collapsing an axis left with one member into it. A
    /// member collapsing into an axis that runs the same way as its parent
    /// is spliced into the parent. A removed member's share goes to a
    /// sibling; a collapsed axis's to the member left.
    fn remove(&mut self, pane: &Entity<V>) {
        let Member::Axis { axis, members } = self else {
            return;
        };
        let Some(ix) = members.iter().position(|member| member.contains(pane)) else {
            return;
        };
        if matches!(&members[ix], Member::Pane(member) if member == pane) {
            members.remove(ix);
        } else {
            members[ix].remove(pane);
            if matches!(&members[ix], Member::Axis { axis: inner, .. } if inner == axis) {
                members.splice(ix);
            }
        }
        if members.len() == 1 {
            *self = members.remove(0);
        }
    }

    /// The least size the member takes along `along`, by dragging borders:
    /// an axis that way needs its members' and its borders' (more if its
    /// ratios aren't in proportion to its members' minimums, as each keeps
    /// its ratio), one across its largest member's.
    fn min_size(&self, along: Axis) -> f32 {
        match self {
            Member::Pane(_) => f32::from(match along {
                Axis::Horizontal => MIN_PANE_WIDTH,
                Axis::Vertical => MIN_PANE_HEIGHT,
            }),
            Member::Axis { axis, members } => {
                let mins = members.iter().map(|member| member.min_size(along));
                if *axis == along {
                    let space = mins.zip(&members.ratios).map(|(min, ratio)| min / ratio);
                    space.fold(0., f32::max) + BORDER * (members.len() - 1) as f32
                } else {
                    mins.fold(0., f32::max)
                }
            }
        }
    }

    /// The axis at `path`: member indices from here.
    fn axis_at(&mut self, path: &[usize]) -> Option<(Axis, &mut Members<V>)> {
        let Member::Axis { axis, members } = self else {
            return None;
        };
        match path.split_first() {
            None => Some((*axis, members)),
            Some((ix, rest)) => members.get_mut(*ix)?.axis_at(rest),
        }
    }

    /// The axis at `path`, read only.
    fn axis_ref(&self, path: &[usize]) -> Option<(Axis, &Members<V>)> {
        let Member::Axis { axis, members } = self else {
            return None;
        };
        match path.split_first() {
            None => Some((*axis, members)),
            Some((ix, rest)) => members.get(*ix)?.axis_ref(rest),
        }
    }

    /// The member, with its axes' borders handles that the host's
    /// `listener`s drag, through `group`. `path` is where it is in the
    /// tree.
    fn render<H: 'static>(
        &self,
        path: &mut Vec<usize>,
        ui: &UiTheme,
        sizes: &AxisSizes,
        group: GroupOf<H, V>,
        cx: &mut Context<H>,
    ) -> AnyElement {
        let Member::Axis { axis, members } = self else {
            let Member::Pane(pane) = self else {
                unreachable!("a member is a pane or an axis");
            };
            return pane.clone().into_any_element();
        };
        let horizontal = *axis == Axis::Horizontal;
        let at = path.clone();
        let record = {
            let (sizes, at) = (sizes.clone(), at.clone());
            canvas(
                move |laid_out, _, _| {
                    sizes.borrow_mut().insert(at.clone(), laid_out.size);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let cells: Vec<_> = members
            .iter()
            .zip(&members.ratios)
            .enumerate()
            .map(|(ix, (member, ratio))| {
                path.push(ix);
                let child = member.render(path, ui, sizes, group, cx);
                path.pop();
                let divider = Divider {
                    path: at.clone(),
                    ix,
                };
                div()
                    .relative()
                    .flex_basis(px(0.))
                    .flex_grow(*ratio)
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .border_color(ui.border)
                    .when(ix > 0 && horizontal, |cell| cell.border_l_1())
                    .when(ix > 0 && !horizontal, |cell| cell.border_t_1())
                    .child(child)
                    .when(ix > 0, |cell| cell.child(divider.handle(*axis, group, cx)))
            })
            .collect();
        div()
            .id(gpui::SharedString::from(format!("axis-{at:?}")))
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .when(!horizontal, |axis| axis.flex_col())
            .on_drag_move(
                cx.listener(move |host, event: &DragMoveEvent<Divider>, _, cx| {
                    let divider = event.drag(cx).clone();
                    if divider.path == at {
                        let group = group(host);
                        group.drag_divider(&divider, event.event.position, event.bounds);
                        cx.notify();
                    }
                }),
            )
            .child(record)
            .children(cells)
            .into_any_element()
    }
}

/// Each axis's size in the last frame drawn, by its path.
type AxisSizes = Rc<RefCell<HashMap<Vec<usize>, Size<Pixels>>>>;

/// How a host reaches its pane group, for the dividers' drags.
pub type GroupOf<H, V> = fn(&mut H) -> &mut PaneGroup<V>;

/// The border before member `ix` of the axis at `path`, as dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    path: Vec<usize>,
    ix: usize,
}

impl Divider {
    /// The border's handle, over it and a little either side: pressing it
    /// starts a drag (twice evens the axis out), and moving draws nothing
    /// but the cursor.
    fn handle<H: 'static, V: PaneView>(
        self,
        axis: Axis,
        group: GroupOf<H, V>,
        cx: &mut Context<H>,
    ) -> impl gpui::IntoElement {
        let id = gpui::SharedString::from(format!("divider-{:?}-{}", self.path, self.ix));
        // From inside the cell's border, which comes before it.
        let offset = px(-(HANDLE - BORDER) / 2. - BORDER);
        let pressed = self.clone();
        div()
            .id(id)
            .absolute()
            // Over the panes either side, for presses, but not for drops.
            .when(!cx.has_active_drag(), |handle| handle.occlude())
            .map(|handle| match axis {
                Axis::Horizontal => handle
                    .top_0()
                    .bottom_0()
                    .left(offset)
                    .w(px(HANDLE))
                    .cursor(CursorStyle::ResizeLeftRight),
                Axis::Vertical => handle
                    .left_0()
                    .right_0()
                    .top(offset)
                    .h(px(HANDLE))
                    .cursor(CursorStyle::ResizeUpDown),
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |host, event: &MouseDownEvent, _, cx| {
                    let group = group(host);
                    group.press_divider(&pressed, event);
                    cx.notify();
                }),
            )
            .on_drag(self, |_, _, _, cx| cx.new(|_| NoGhost))
    }
}

/// A border's drag draws nothing at the pointer.
struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        gpui::Empty
    }
}

/// Where a border's drag started: the pointer, and the axis's ratios.
struct Press {
    divider: Divider,
    at: Point<Pixels>,
    ratios: Vec<f32>,
    /// The tree's topology generation when pressed.
    generation: u64,
}

/// Where a tab moves to.
pub enum Destination<V> {
    /// Into `pane`, at `index` among its tabs, or at the end; in the tab's
    /// own pane, the tab moves there.
    Pane {
        pane: Entity<V>,
        index: Option<usize>,
    },
    /// Into a new pane, split from `pane` in `direction`.
    Split {
        pane: Entity<V>,
        direction: Direction,
    },
}

impl<V> Clone for Destination<V> {
    fn clone(&self) -> Self {
        match self {
            Destination::Pane { pane, index } => Destination::Pane {
                pane: pane.clone(),
                index: *index,
            },
            Destination::Split { pane, direction } => Destination::Split {
                pane: pane.clone(),
                direction: *direction,
            },
        }
    }
}

impl<V: 'static> PartialEq for Destination<V> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Destination::Pane { pane, index },
                Destination::Pane {
                    pane: other,
                    index: at,
                },
            ) => pane == other && index == at,
            (
                Destination::Split { pane, direction },
                Destination::Split {
                    pane: other,
                    direction: way,
                },
            ) => pane == other && direction == way,
            _ => false,
        }
    }
}

impl<V: 'static> std::fmt::Debug for Destination<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Destination::Pane { pane, index } => f
                .debug_struct("Pane")
                .field("pane", &pane.entity_id())
                .field("index", index)
                .finish(),
            Destination::Split { pane, direction } => f
                .debug_struct("Split")
                .field("pane", &pane.entity_id())
                .field("direction", direction)
                .finish(),
        }
    }
}

/// The tree of panes. It always has one at least.
pub struct PaneGroup<V> {
    root: Member<V>,
    /// The border being dragged.
    press: Option<Press>,
    /// Counts changes to the tree's topology, which invalidate a press.
    generation: u64,
    /// Where the last frame laid each axis out, for [`PaneGroup::resize`].
    sizes: AxisSizes,
    /// The pane drawn alone, and its bounds before it was.
    zoomed: Option<(Entity<V>, Bounds<Pixels>)>,
}

impl<V: PaneView> PaneGroup<V> {
    pub fn new(pane: Entity<V>) -> Self {
        Self {
            root: Member::Pane(pane),
            press: None,
            generation: 0,
            sizes: AxisSizes::default(),
            zoomed: None,
        }
    }

    pub fn root(&self) -> &Member<V> {
        &self.root
    }

    /// Replaces the tree with `root`, as a saved layout is restored, ending
    /// a zoom. It must have a pane.
    pub fn set_root(&mut self, root: Member<V>) {
        self.zoomed = None;
        self.generation += 1;
        self.press = None;
        self.sizes.borrow_mut().clear();
        self.root = root;
    }

    /// Where `pane` is in the tree: member indices from the root.
    pub fn path_of(&self, pane: &Entity<V>) -> Option<Vec<usize>> {
        fn find<V>(member: &Member<V>, pane: &Entity<V>, path: &mut Vec<usize>) -> bool {
            match member {
                Member::Pane(member) => member == pane,
                Member::Axis { members, .. } => members.iter().enumerate().any(|(ix, member)| {
                    path.push(ix);
                    let found = find(member, pane, path);
                    if !found {
                        path.pop();
                    }
                    found
                }),
            }
        }
        let mut path = Vec::new();
        find(&self.root, pane, &mut path).then_some(path)
    }

    /// The panes, left to right and top to bottom as the tree nests them.
    pub fn panes(&self) -> Vec<&Entity<V>> {
        let mut panes = Vec::new();
        self.root.panes(&mut panes);
        panes
    }

    /// Puts `new` beside `old`, in `direction` from it, ending a zoom.
    pub fn split(&mut self, old: &Entity<V>, new: Entity<V>, direction: Direction) {
        self.zoomed = None;
        self.generation += 1;
        self.sizes.borrow_mut().clear();
        self.root.split(old, new, direction);
    }

    /// Removes `pane`, unless it's the last, ending a zoom.
    pub fn remove(&mut self, pane: &Entity<V>) {
        self.zoomed = None;
        self.generation += 1;
        self.sizes.borrow_mut().clear();
        self.root.remove(pane);
    }

    /// Draws `pane` alone, over the group's whole area, until the tree
    /// changes or [`unzoom`](Self::unzoom): the others keep their items and
    /// ratios. False, changing nothing, for a pane alone or not in the
    /// group.
    pub fn zoom(&mut self, pane: &Entity<V>, cx: &App) -> bool {
        let panes = self.panes();
        if panes.len() < 2 || !panes.contains(&pane) {
            return false;
        }
        if self
            .zoomed
            .as_ref()
            .is_some_and(|(zoomed, _)| zoomed == pane)
        {
            return true;
        }
        let bounds = self.bounds_of(pane, cx);
        self.zoomed = Some((pane.clone(), bounds));
        self.press = None;
        true
    }

    /// Ends the zoom: every pane is laid out again at its ratios. False if
    /// none was zoomed.
    pub fn unzoom(&mut self) -> bool {
        self.zoomed.take().is_some()
    }

    /// The pane drawn alone, if one is.
    pub fn zoomed(&self) -> Option<&Entity<V>> {
        self.zoomed.as_ref().map(|(pane, _)| pane)
    }

    /// Where `pane` was laid out: for the zoomed pane, where it was before
    /// the zoom, as the panes it hides still are.
    fn bounds_of(&self, pane: &Entity<V>, cx: &App) -> Bounds<Pixels> {
        match &self.zoomed {
            Some((zoomed, bounds)) if zoomed == pane => *bounds,
            _ => pane.read(cx).pane().bounds(),
        }
    }

    /// Swaps `pane` with its next sibling, or its previous one if it's
    /// last, as vim's `<C-w>x`: the two trade places and ratios, so each
    /// keeps its size, ending a zoom. Returns the other pane, now in
    /// `pane`'s place; none, changing nothing, if that sibling is an axis or
    /// `pane` is alone.
    pub fn exchange(&mut self, pane: &Entity<V>) -> Option<Entity<V>> {
        let path = self.path_of(pane)?;
        let (&ix, parent) = path.split_last()?;
        let (_, members) = self.root.axis_at(parent)?;
        let other = if ix + 1 < members.len() {
            ix + 1
        } else {
            ix.checked_sub(1)?
        };
        let Member::Pane(sibling) = &members[other] else {
            return None;
        };
        let sibling = sibling.clone();
        members.members.swap(ix, other);
        members.ratios.swap(ix, other);
        self.zoomed = None;
        self.generation += 1;
        self.press = None;
        Some(sibling)
    }

    /// The pane next to `from` in `direction`, by where they were laid out
    /// (the zoomed pane where it was before the zoom): the nearest whose
    /// bounds overlap `from`'s on the other axis, the longest overlap
    /// breaking a tie.
    pub fn neighbour(&self, from: &Entity<V>, direction: Direction, cx: &App) -> Option<Entity<V>> {
        let origin = self.bounds_of(from, cx);
        self.panes()
            .into_iter()
            .filter(|pane| *pane != from)
            .filter_map(|pane| {
                let bounds = self.bounds_of(pane, cx);
                let (gap, overlap) = gap_and_overlap(&origin, &bounds, direction)?;
                Some(((gap, -overlap), pane))
            })
            .min_by(|(a, _), (b, _)| a.partial_cmp(b).expect("finite bounds"))
            .map(|(_, pane)| pane.clone())
    }

    /// The pane nearest `from` by where they were laid out (the zoomed pane
    /// where it was before the zoom), other than it.
    pub fn nearest(&self, from: &Entity<V>, cx: &App) -> Option<Entity<V>> {
        let center = self.bounds_of(from, cx).center();
        self.panes()
            .into_iter()
            .filter(|pane| *pane != from)
            .map(|pane| {
                let to = self.bounds_of(pane, cx).center();
                let (dx, dy) = (f32::from(to.x - center.x), f32::from(to.y - center.y));
                (dx * dx + dy * dy, pane)
            })
            .min_by(|(a, _), (b, _)| a.total_cmp(b))
            .map(|(_, pane)| pane.clone())
    }

    /// Every item of every pane, in the panes' order.
    pub fn items(&self, cx: &App) -> Vec<Box<dyn ItemHandle>> {
        (self.panes().into_iter())
            .flat_map(|pane| pane.read(cx).pane().items().to_vec())
            .collect()
    }

    /// The ratios of the axis at `path`, member indices from the root.
    pub fn ratios(&self, path: &[usize]) -> Option<Vec<f32>> {
        Some(self.root.axis_ref(path)?.1.ratios.clone())
    }

    /// A press on `divider`'s handle: the start of a drag, or, pressed twice,
    /// evens its axis out.
    fn press_divider(&mut self, divider: &Divider, event: &MouseDownEvent) -> Option<()> {
        let (_, members) = self.root.axis_at(&divider.path)?;
        if event.click_count == 2 {
            let even = 1. / members.len() as f32;
            members.ratios.fill(even);
        }
        let (divider, at, ratios) = (divider.clone(), event.position, members.ratios.clone());
        self.press = Some(Press {
            divider,
            at,
            ratios,
            generation: self.generation,
        });
        Some(())
    }

    /// Moves `divider`'s border with the pointer, now at `to`, in its axis
    /// laid out in `bounds`, as far as the members either side's minimums
    /// let it. Only those two members' ratios change; if they don't fit
    /// their minimums, neither does.
    fn drag_divider(
        &mut self,
        divider: &Divider,
        to: Point<Pixels>,
        bounds: Bounds<Pixels>,
    ) -> Option<()> {
        let press = self
            .press
            .as_ref()
            .filter(|press| press.divider == *divider && press.generation == self.generation)?;
        let (axis, members) = self.root.axis_at(&divider.path)?;
        let (a, b) = (divider.ix.checked_sub(1)?, divider.ix);
        if press.ratios.len() != members.len() || b >= members.len() {
            return None;
        }
        let (length, moved) = match axis {
            Axis::Horizontal => (bounds.size.width, to.x - press.at.x),
            Axis::Vertical => (bounds.size.height, to.y - press.at.y),
        };
        let space = f32::from(length) - BORDER * (members.len() - 1) as f32;
        let pair = press.ratios[a] + press.ratios[b];
        let min = members[a].min_size(axis);
        let max = pair * space - members[b].min_size(axis);
        if space <= 0. || min > max {
            return None;
        }
        let size = (press.ratios[a] * space + f32::from(moved)).clamp(min, max);
        members.ratios[a] = size / space;
        members.ratios[b] = pair - members.ratios[a];
        Some(())
    }

    /// Grows (`delta > 0`) or shrinks the member holding `pane` along the
    /// nearest enclosing axis running `axis`, taking the space from, or
    /// giving it to, its next sibling (its previous one if it's last). Only
    /// those two ratios change, within the members' minimums as dragging
    /// is. False, changing nothing, if there's no such axis, no room, no
    /// frame drawn yet, or a pane is zoomed.
    pub fn resize(&mut self, pane: &Entity<V>, axis: Axis, delta: Pixels) -> bool {
        if self.zoomed.is_some() {
            return false;
        }
        let Some(path) = self.path_of(pane) else {
            return false;
        };
        let Some(depth) = (0..path.len()).rev().find(|&depth| {
            matches!(self.root.axis_ref(&path[..depth]), Some((found, _)) if found == axis)
        }) else {
            return false;
        };
        let Some(length) = self
            .sizes
            .borrow()
            .get(&path[..depth])
            .map(|size| match axis {
                Axis::Horizontal => f32::from(size.width),
                Axis::Vertical => f32::from(size.height),
            })
        else {
            return false;
        };
        let Some((_, members)) = self.root.axis_at(&path[..depth]) else {
            return false;
        };
        let ix = path[depth];
        let other = if ix + 1 < members.len() {
            ix + 1
        } else if ix > 0 {
            ix - 1
        } else {
            return false;
        };
        let space = length - BORDER * (members.len() - 1) as f32;
        let pair = members.ratios[ix] + members.ratios[other];
        let (min, other_min) = (members[ix].min_size(axis), members[other].min_size(axis));
        let max = pair * space - other_min;
        if space <= 0. || min > max {
            return false;
        }
        let current = members.ratios[ix] * space;
        let size = (current + f32::from(delta)).clamp(min, max);
        if (size - current).abs() < f32::EPSILON {
            return false;
        }
        members.ratios[ix] = size / space;
        members.ratios[other] = pair - members.ratios[ix];
        // A press in progress holds the old ratios.
        self.generation += 1;
        self.press = None;
        true
    }

    /// Gives every axis's members equal ratios, keeping a zoom.
    pub fn equalize(&mut self) {
        fn even<V>(member: &mut Member<V>) {
            if let Member::Axis { members, .. } = member {
                let share = 1. / members.members.len() as f32;
                members.ratios.fill(share);
                members.members.iter_mut().for_each(even);
            }
        }
        even(&mut self.root);
        self.generation += 1;
        self.press = None;
    }

    /// The pane after (`forward`) or before `from` in [`panes`](Self::panes)
    /// order, wrapping; none if it's the only one.
    pub fn next(&self, from: &Entity<V>, forward: bool) -> Option<Entity<V>> {
        let panes = self.panes();
        let at = panes.iter().position(|pane| *pane == from)?;
        let to = if forward {
            (at + 1) % panes.len()
        } else {
            (at + panes.len() - 1) % panes.len()
        };
        (to != at).then(|| panes[to].clone())
    }

    /// Moves item `item` from `source` to `destination`: the only way a
    /// tab moves, for a drag or a key. Into another pane, it goes at the
    /// index given, shown and focused; if that pane shows its document
    /// already ([`ItemHandle::shares_document`](crate::ItemHandle)), that
    /// item is shown instead and the moved one closes, once it agrees to
    /// ([`close::confirm`]), which it isn't asked while that copy shows its
    /// document. A split makes the pane
    /// first. Within its own pane, the item moves to the index. Returns the
    /// pane the item went to (or that showed its document), and whether it
    /// was split off for the move: the host then joins it to the group if
    /// so, activates it and closes a source the move emptied.
    pub fn move_tab(
        &mut self,
        source: &Entity<V>,
        item: ItemId,
        destination: Destination<V>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<(Entity<V>, bool)> {
        let panes = self.panes();
        let in_group = |pane: &Entity<V>| panes.contains(&pane);
        let ix = source.read(cx).pane().position(item)?;
        let (target, index, new) = match destination {
            Destination::Pane { pane, index } if in_group(&pane) && in_group(source) => {
                (pane, index, false)
            }
            Destination::Split { pane, direction } if in_group(&pane) && in_group(source) => {
                let new = pane.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)));
                self.split(&pane, new.clone(), direction);
                (new, None, true)
            }
            _ => return None,
        };
        if target == *source {
            source.update(cx, |pane, cx| pane.reorder_item(ix, index, window, cx));
            return Some((target, new));
        }
        let moved = source.read(cx).pane().items()[ix].boxed_clone();
        let shown = (target.read(cx).pane().items())
            .iter()
            .any(|other| other.shares_document(&*moved, cx));
        if shown {
            // The moved item closes, the target's copy showing in its place.
            let now = Items {
                closing: vec![moved.boxed_clone()],
                open: self.items(cx),
            };
            let panes: Vec<_> = self.panes().into_iter().map(Entity::downgrade).collect();
            let id = moved.id();
            let live: LiveItems = Rc::new(move |cx| {
                let open: Vec<_> = (panes.iter().filter_map(WeakEntity::upgrade))
                    .flat_map(|pane| pane.read(cx).pane().items().to_vec())
                    .collect();
                let closing = open.iter().filter(|item| item.id() == id).cloned();
                Some(Items {
                    closing: closing.collect(),
                    open,
                })
            });
            let request =
                CloseRequest::new(CloseKind::Items).scope(window.window_handle(), Some(now), live);
            let (from, to) = (source.downgrade(), target.downgrade());
            let commit = move |window: Option<&mut Window>, cx: &mut App| {
                if let Some(window) = window {
                    show_copy(&from, &to, id, window, cx);
                }
            };
            match close::confirm(request, commit, Some(window), cx) {
                Closing::Now => show_copy(&source.downgrade(), &target.downgrade(), id, window, cx),
                Closing::Refused => return None,
                Closing::Later(closing) => {
                    closing.detach();
                    return None;
                }
            }
        } else {
            let item = source.update(cx, |pane, cx| {
                cx.notify();
                pane.take_item(ix, window, cx)
            });
            if let Some(item) = item {
                target.update(cx, |pane, cx| pane.insert_item(item, index, window, cx));
            }
        }
        Some((target, new))
    }

    /// Draws the tree for host `H`, which reaches the group through
    /// `group` when a divider is dragged. Its borders are the active
    /// theme's ([`ui_theme`]), so they follow a theme change; `_ui` is
    /// unused, left for callers until a later API cleanup. A zoomed pane is
    /// drawn alone, with no borders.
    pub fn render<H: 'static>(
        &self,
        _ui: &UiTheme,
        group: GroupOf<H, V>,
        cx: &mut Context<H>,
    ) -> AnyElement {
        let ui = ui_theme(cx);
        let tree = match &self.zoomed {
            Some((pane, _)) => pane.clone().into_any_element(),
            None => (self.root).render(&mut Vec::new(), &ui, &self.sizes, group, cx),
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .child(tree)
            .into_any_element()
    }
}

/// Closes item `id` in `source`, and shows the item in `target` that
/// shows its document.
fn show_copy<V: PaneView>(
    source: &WeakEntity<V>,
    target: &WeakEntity<V>,
    id: ItemId,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(moved) = (source.upgrade()).and_then(|source| {
        source
            .read(cx)
            .pane()
            .items()
            .iter()
            .find(|item| item.id() == id)
            .cloned()
    }) else {
        return;
    };
    source
        .update(cx, |pane, cx| {
            if let Some(ix) = pane.pane().position(id) {
                pane.take_item(ix, window, cx);
            }
            cx.notify();
        })
        .ok();
    target
        .update(cx, |pane, cx| {
            let shown =
                (pane.pane().items().iter()).position(|other| other.shares_document(&*moved, cx));
            if let Some(shown) = shown {
                pane.activate_item(shown, window, cx);
            }
            cx.notify();
        })
        .ok();
}

/// How far `to` lies from `from` in `direction`, and how much they overlap
/// across it; `None` unless `to` is that way and they overlap. Panes tile,
/// so one overlapping across lies wholly before or after.
fn gap_and_overlap(
    from: &Bounds<Pixels>,
    to: &Bounds<Pixels>,
    direction: Direction,
) -> Option<(f32, f32)> {
    let (from_start, from_end, to_start, to_end, across) = match direction.axis() {
        Axis::Horizontal => (
            from.left(),
            from.right(),
            to.left(),
            to.right(),
            from.bottom().min(to.bottom()) - from.top().max(to.top()),
        ),
        Axis::Vertical => (
            from.top(),
            from.bottom(),
            to.top(),
            to.bottom(),
            from.right().min(to.right()) - from.left().max(to.left()),
        ),
    };
    if across <= Pixels::ZERO {
        return None;
    }
    let gap = if direction.forward() {
        (to_start + to_end > from_start + from_end).then(|| to_start - from_end)
    } else {
        (to_start + to_end < from_start + from_end).then(|| from_start - to_end)
    }?;
    Some((f32::from(gap), f32::from(across)))
}
