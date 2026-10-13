//! The editor area: kiln-shell's tree of panes over fastedit's [`Pane`]s,
//! split side by side or one above another
//! ([`kiln_shell::pane_group`]).
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
use gpui::{AnyElement, App, Context, Entity, Global, Window};
pub use kiln_shell::pane_group::{Axis, Direction};
#[cfg(test)]
pub(crate) use kiln_shell::pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH};

use crate::{Pane, Workbench, pane::TabId};

/// A member of the tree: one of fastedit's panes, or an axis of members.
pub type Member = kiln_shell::Member<Pane>;

/// An axis's members, each with its share of the axis.
pub type Members = kiln_shell::Members<Pane>;

/// Where a tab moves to.
pub(crate) type Destination = kiln_shell::Destination<Pane>;

/// The tree of panes, kiln-shell's over fastedit's [`Pane`]s. It always has
/// one at least.
pub struct PaneGroup {
    group: kiln_shell::PaneGroup<Pane>,
    /// Calls of [`PaneGroup::move_tab`].
    #[cfg(test)]
    pub(crate) moves: usize,
}

impl Deref for PaneGroup {
    type Target = kiln_shell::PaneGroup<Pane>;

    fn deref(&self) -> &Self::Target {
        &self.group
    }
}

impl DerefMut for PaneGroup {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.group
    }
}

impl PaneGroup {
    pub fn new(pane: Entity<Pane>) -> Self {
        Self {
            group: kiln_shell::PaneGroup::new(pane),
            #[cfg(test)]
            moves: 0,
        }
    }

    /// Moves `tab` from `source` to `destination`: see
    /// [`kiln_shell::PaneGroup::move_tab`].
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
        self.group.move_tab(source, tab, destination, window, cx)
    }

    /// The tree, its dividers dragged through the workbench's group.
    pub(crate) fn render(&self, ui: &UiTheme, cx: &mut Context<Workbench>) -> AnyElement {
        self.group
            .render(ui, |workbench| &mut workbench.pane_group_mut().group, cx)
    }
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
    /// Grows or shrinks the active pane by [`RESIZE_STEP`] along an axis.
    Resize(Axis, bool),
    /// Evens out every axis's panes.
    Equalize,
    /// Activates the next (or previous) pane, wrapping.
    FocusNext(bool),
}

/// How far a resize command moves a border.
pub(crate) const RESIZE_STEP: gpui::Pixels = gpui::px(32.);

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
        (
            "pane.grow_width",
            "Grow Pane Width",
            Resize(Axis::Horizontal, true),
        ),
        (
            "pane.shrink_width",
            "Shrink Pane Width",
            Resize(Axis::Horizontal, false),
        ),
        (
            "pane.grow_height",
            "Grow Pane Height",
            Resize(Axis::Vertical, true),
        ),
        (
            "pane.shrink_height",
            "Shrink Pane Height",
            Resize(Axis::Vertical, false),
        ),
        ("pane.equalize", "Equalize Panes", Equalize),
        ("pane.focus_next", "Focus Next Pane", FocusNext(true)),
        (
            "pane.focus_previous",
            "Focus Previous Pane",
            FocusNext(false),
        ),
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

#[cfg(test)]
#[path = "pane_resize_tests.rs"]
mod resize_tests;
