//! The editor area: a tree of [`Pane`]s, split side by side or one above
//! another. Members of an axis share its space equally, with a 1 px border
//! between them; there are no drag handles.
//!
//! Its commands (`pane.split_right`, `pane.focus_left`, `pane.move_tab_left`,
//! `pane.close` and so on) are those of [`init`]'s registry, in the
//! workbench's [`crate::KEY_CONTEXT`]. Their handlers only name an op: the
//! workbench does it with the window at hand.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_ui::UiTheme;
use gpui::{
    AnyElement, App, Bounds, Entity, Global, IntoElement as _, ParentElement as _, Pixels,
    Styled as _, div, prelude::FluentBuilder as _,
};

use crate::Pane;

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
    Axis { axis: Axis, members: Vec<Member> },
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
                    members: pair,
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
            members.insert(at, Member::Pane(new));
        } else {
            members[ix].split(old, new, direction);
        }
    }

    /// Removes `pane`, collapsing an axis left with one member into it. A
    /// member collapsing into an axis that runs the same way as its parent
    /// is spliced into the parent.
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
                let Member::Axis { members: inner, .. } = members.remove(ix) else {
                    unreachable!("matched an axis");
                };
                members.splice(ix..ix, inner);
            }
        }
        if members.len() == 1 {
            *self = members.pop().expect("one member");
        }
    }

    fn render(&self, ui: &UiTheme) -> AnyElement {
        match self {
            Member::Pane(pane) => pane.clone().into_any_element(),
            Member::Axis { axis, members } => {
                let horizontal = *axis == Axis::Horizontal;
                let cells = members.iter().enumerate().map(|(ix, member)| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .border_color(ui.border)
                        .when(ix > 0 && horizontal, |cell| cell.border_l_1())
                        .when(ix > 0 && !horizontal, |cell| cell.border_t_1())
                        .child(member.render(ui))
                });
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .when(!horizontal, |axis| axis.flex_col())
                    .children(cells)
                    .into_any_element()
            }
        }
    }
}

/// The tree of panes. It always has one at least.
pub struct PaneGroup {
    root: Member,
}

impl PaneGroup {
    pub fn new(pane: Entity<Pane>) -> Self {
        Self {
            root: Member::Pane(pane),
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
        self.root.split(old, new, direction);
    }

    /// Removes `pane`, unless it's the last.
    pub(crate) fn remove(&mut self, pane: &Entity<Pane>) {
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

    pub(crate) fn render(&self, ui: &UiTheme) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .child(self.root.render(ui))
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
