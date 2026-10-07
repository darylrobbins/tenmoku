//! The editor area: a tree of [`Pane`]s, split side by side or one above
//! another. Members of an axis share its space by ratio, with a 1 px border
//! between them, and a handle on each border drags it.
//!
//! Its commands (`pane.split_right`, `pane.focus_left`, `pane.move_tab_left`,
//! `pane.close` and so on) are those of [`init`]'s registry, in the
//! workbench's [`crate::KEY_CONTEXT`]. Their handlers only name an op: the
//! workbench does it with the window at hand.

use std::{
    ops::{Deref, DerefMut},
    rc::Rc,
};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_ui::UiTheme;
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, CursorStyle, DragMoveEvent, Entity, Global,
    InteractiveElement as _, IntoElement as _, MouseButton, MouseDownEvent, ParentElement as _,
    Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{Pane, Workbench, pane::TabId};

/// The narrowest a pane gets by dragging a border.
pub(crate) const MIN_PANE_WIDTH: Pixels = px(120.);
/// The shortest a pane gets by dragging a border.
pub(crate) const MIN_PANE_HEIGHT: Pixels = px(80.);
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
    fn axis(self) -> Axis {
        match self {
            Direction::Left | Direction::Right => Axis::Horizontal,
            Direction::Up | Direction::Down => Axis::Vertical,
        }
    }

    /// Whether it goes towards the axis's end (right or down).
    fn forward(self) -> bool {
        matches!(self, Direction::Right | Direction::Down)
    }
}

/// A member of the tree.
pub enum Member {
    Pane(Entity<Pane>),
    Axis { axis: Axis, members: Members },
}

/// An axis's members, each with its share of the axis: their ratios, which
/// sum to 1. A new axis shares equally.
pub struct Members {
    members: Vec<Member>,
    ratios: Vec<f32>,
}

impl Members {
    fn new(members: Vec<Member>) -> Self {
        let ratios = vec![1. / members.len() as f32; members.len()];
        Self { members, ratios }
    }

    /// Puts `member` at `at`, beside member `of`, with half of its share.
    fn insert_beside(&mut self, of: usize, at: usize, member: Member) {
        self.ratios[of] /= 2.;
        self.ratios.insert(at, self.ratios[of]);
        self.members.insert(at, member);
    }

    /// Removes member `ix`, giving its share to the one before it, or after
    /// it if it was first.
    fn remove(&mut self, ix: usize) -> Member {
        let share = self.ratios.remove(ix);
        if let Some(heir) = self.ratios.get_mut(ix.saturating_sub(1)) {
            *heir += share;
        }
        self.members.remove(ix)
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
impl Deref for Members {
    type Target = [Member];

    fn deref(&self) -> &[Member] {
        &self.members
    }
}

impl DerefMut for Members {
    fn deref_mut(&mut self) -> &mut [Member] {
        &mut self.members
    }
}

impl Member {
    fn panes<'a>(&'a self, out: &mut Vec<&'a Entity<Pane>>) {
        match self {
            Member::Pane(pane) => out.push(pane),
            Member::Axis { members, .. } => members.iter().for_each(|member| member.panes(out)),
        }
    }

    fn contains(&self, pane: &Entity<Pane>) -> bool {
        match self {
            Member::Pane(member) => member == pane,
            Member::Axis { members, .. } => members.iter().any(|member| member.contains(pane)),
        }
    }

    /// Puts `new` beside `old` in `direction`: as a sibling in an axis that
    /// runs that way, or else in a new axis nested in `old`'s place.
    fn split(&mut self, old: &Entity<Pane>, new: Entity<Pane>, direction: Direction) {
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
    fn remove(&mut self, pane: &Entity<Pane>) {
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
    fn axis_at(&mut self, path: &[usize]) -> Option<(Axis, &mut Members)> {
        let Member::Axis { axis, members } = self else {
            return None;
        };
        match path.split_first() {
            None => Some((*axis, members)),
            Some((ix, rest)) => members.get_mut(*ix)?.axis_at(rest),
        }
    }

    /// The member, with its axes' borders handles that the workbench's
    /// `listener`s drag. `path` is where it is in the tree.
    fn render(
        &self,
        path: &mut Vec<usize>,
        ui: &UiTheme,
        cx: &mut Context<Workbench>,
    ) -> AnyElement {
        let Member::Axis { axis, members } = self else {
            let Member::Pane(pane) = self else {
                unreachable!("a member is a pane or an axis");
            };
            return pane.clone().into_any_element();
        };
        let horizontal = *axis == Axis::Horizontal;
        let at = path.clone();
        let cells: Vec<_> = members
            .iter()
            .zip(&members.ratios)
            .enumerate()
            .map(|(ix, (member, ratio))| {
                path.push(ix);
                let child = member.render(path, ui, cx);
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
                    .when(ix > 0, |cell| cell.child(divider.handle(*axis, cx)))
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
                cx.listener(move |workbench, event: &DragMoveEvent<Divider>, _, cx| {
                    let divider = event.drag(cx).clone();
                    if divider.path == at {
                        let group = workbench.pane_group_mut();
                        group.drag_divider(&divider, event.event.position, event.bounds);
                        cx.notify();
                    }
                }),
            )
            .children(cells)
            .into_any_element()
    }
}

/// The border before member `ix` of the axis at `path`, as dragged.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Divider {
    path: Vec<usize>,
    ix: usize,
}

impl Divider {
    /// The border's handle, over it and a little either side: pressing it
    /// starts a drag (twice evens the axis out), and moving draws nothing
    /// but the cursor.
    fn handle(self, axis: Axis, cx: &mut Context<Workbench>) -> impl gpui::IntoElement {
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
                cx.listener(move |workbench, event: &MouseDownEvent, _, cx| {
                    let group = workbench.pane_group_mut();
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
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Destination {
    /// Into `pane`, at `index` among its tabs, or at the end; in the tab's
    /// own pane, the tab moves there.
    Pane {
        pane: Entity<Pane>,
        index: Option<usize>,
    },
    /// Into a new pane, split from `pane` in `direction`.
    Split {
        pane: Entity<Pane>,
        direction: Direction,
    },
}

/// The tree of panes. It always has one at least.
pub struct PaneGroup {
    root: Member,
    /// The border being dragged.
    press: Option<Press>,
    /// Counts changes to the tree's topology, which invalidate a press.
    generation: u64,
    /// Calls of [`PaneGroup::move_tab`].
    #[cfg(test)]
    pub(crate) moves: usize,
}

impl PaneGroup {
    pub fn new(pane: Entity<Pane>) -> Self {
        Self {
            root: Member::Pane(pane),
            press: None,
            generation: 0,
            #[cfg(test)]
            moves: 0,
        }
    }

    pub fn root(&self) -> &Member {
        &self.root
    }

    /// The panes, left to right and top to bottom as the tree nests them.
    pub fn panes(&self) -> Vec<&Entity<Pane>> {
        let mut panes = Vec::new();
        self.root.panes(&mut panes);
        panes
    }

    /// Puts `new` beside `old`, in `direction` from it.
    pub(crate) fn split(&mut self, old: &Entity<Pane>, new: Entity<Pane>, direction: Direction) {
        self.generation += 1;
        self.root.split(old, new, direction);
    }

    /// Removes `pane`, unless it's the last.
    pub(crate) fn remove(&mut self, pane: &Entity<Pane>) {
        self.generation += 1;
        self.root.remove(pane);
    }

    /// The pane next to `from` in `direction`, by where they were laid out:
    /// the nearest whose bounds overlap `from`'s on the other axis, the
    /// longest overlap breaking a tie.
    pub(crate) fn neighbour(
        &self,
        from: &Entity<Pane>,
        direction: Direction,
        cx: &App,
    ) -> Option<Entity<Pane>> {
        let origin = from.read(cx).bounds();
        self.panes()
            .into_iter()
            .filter(|pane| *pane != from)
            .filter_map(|pane| {
                let bounds = pane.read(cx).bounds();
                let (gap, overlap) = gap_and_overlap(&origin, &bounds, direction)?;
                Some(((gap, -overlap), pane))
            })
            .min_by(|(a, _), (b, _)| a.partial_cmp(b).expect("finite bounds"))
            .map(|(_, pane)| pane.clone())
    }

    /// The pane nearest `from` by where they were laid out, other than it.
    pub(crate) fn nearest(&self, from: &Entity<Pane>, cx: &App) -> Option<Entity<Pane>> {
        let center = from.read(cx).bounds().center();
        self.panes()
            .into_iter()
            .filter(|pane| *pane != from)
            .map(|pane| {
                let to = pane.read(cx).bounds().center();
                let (dx, dy) = (f32::from(to.x - center.x), f32::from(to.y - center.y));
                (dx * dx + dy * dy, pane)
            })
            .min_by(|(a, _), (b, _)| a.total_cmp(b))
            .map(|(_, pane)| pane.clone())
    }

    /// The ratios of the axis at `path`, member indices from the root.
    #[cfg(test)]
    pub(crate) fn ratios(&mut self, path: &[usize]) -> Option<Vec<f32>> {
        Some(self.root.axis_at(path)?.1.ratios.clone())
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

    /// Moves `tab` from `source` to `destination`: the only way a tab moves,
    /// for a drag or a key. Into another pane, it goes at the index given,
    /// shown and focused; if that pane shows its file already, that tab is
    /// shown instead and the moved one closes, as long as its unsaved
    /// changes live on in another tab. A split makes the pane first. Within its
    /// own pane, the tab moves to the index. Returns the pane the tab went
    /// to (or that showed its file), and whether it was split off for the
    /// move: the workbench then watches it if so, activates it and closes a
    /// source the move emptied.
    pub(crate) fn move_tab(
        &mut self,
        source: &Entity<Pane>,
        tab: TabId,
        destination: Destination,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<(Entity<Pane>, bool)> {
        #[cfg(test)]
        {
            self.moves += 1;
        }
        let panes = self.panes();
        let in_group = |pane: &Entity<Pane>| panes.contains(&pane);
        let ix = source
            .read(cx)
            .tabs()
            .iter()
            .position(|open| open.id() == tab)?;
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
            source.update(cx, |pane, cx| pane.reorder(ix, index, window, cx));
            return Some((target, new));
        }
        let tab = &source.read(cx).tabs()[ix];
        let path = tab.path.as_deref();
        let shown = path.and_then(|path| target.read(cx).position(path));
        if let Some(shown) = shown {
            let model = tab.editor.read(cx).model();
            if model.is_dirty() && !source.read(cx).shown_elsewhere(&tab.editor, cx) {
                log::warn!(
                    "not moving {}: it has unsaved changes, and the target shows another copy",
                    crate::pane::file_name(model.path())
                );
                return None;
            }
            source.update(cx, |pane, cx| {
                pane.take(ix, window, cx);
                cx.notify();
            });
            target.update(cx, |pane, cx| {
                pane.activate(shown, window, cx);
                cx.notify();
            });
        } else {
            let tab = source.update(cx, |pane, cx| {
                cx.notify();
                pane.take(ix, window, cx)
            });
            if let Some(tab) = tab {
                target.update(cx, |pane, cx| pane.insert(tab, index, window, cx));
            }
        }
        Some((target, new))
    }

    pub(crate) fn render(&self, ui: &UiTheme, cx: &mut Context<Workbench>) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .child(self.root.render(&mut Vec::new(), ui, cx))
            .into_any_element()
    }
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

/// What a pane command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneOp {
    /// Opens a pane in that direction (right or down) of the active one.
    Split(Direction),
    /// Activates the neighbouring pane.
    Focus(Direction),
    /// Moves the active tab to the neighbouring pane, splitting first if
    /// there's none.
    MoveTab(Direction),
    /// Closes the active pane with its tabs.
    Close,
}

/// The [`CommandTarget`] of pane commands: the op to do.
pub(crate) struct PaneCommands;

impl CommandTarget for PaneCommands {
    type Tx = ();
    type Target<'a> = Option<PaneOp>;
}

struct Commands(Rc<Registry<PaneCommands>>);

impl Global for Commands {}

/// Registers the pane commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<PaneCommands> {
    use Direction::*;
    use PaneOp::*;
    let commands = [
        ("pane.split_right", "Split Right", Split(Right)),
        ("pane.split_down", "Split Down", Split(Down)),
        ("pane.focus_left", "Focus Left Pane", Focus(Left)),
        ("pane.focus_right", "Focus Right Pane", Focus(Right)),
        ("pane.focus_up", "Focus Pane Above", Focus(Up)),
        ("pane.focus_down", "Focus Pane Below", Focus(Down)),
        ("pane.move_tab_left", "Move Tab Left", MoveTab(Left)),
        ("pane.move_tab_right", "Move Tab Right", MoveTab(Right)),
        ("pane.move_tab_up", "Move Tab Up", MoveTab(Up)),
        ("pane.move_tab_down", "Move Tab Down", MoveTab(Down)),
        ("pane.close", "Close Pane", Close),
    ];
    let mut registry = Registry::new();
    for (id, title, op) in commands {
        registry
            .register(id, title, move |target: &mut Option<PaneOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("pane command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// The op of `run`, if it's a pane command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<PaneOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}
