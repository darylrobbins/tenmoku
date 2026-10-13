//! The docks' sizes and places from the keyboard: `dock.grow` and
//! `dock.shrink` change the size of the dock holding focus (the
//! terminal's while the terminal has focus), else the sidebar's, by
//! [`STEP`] within its limits ([`kiln_shell::dock`]), showing it if
//! hidden; `terminal.move_to_bottom`, `terminal.move_to_left` and
//! `terminal.move_to_right` move the terminal's dock to that edge, and
//! `dock.move_sidebar_left` and `dock.move_sidebar_right` the sidebar,
//! one dock per edge ([`displaced`]). They have no default keys: they're
//! for the palette and the user's keymap.
//!
//! Their handlers only name an op: the workbench does it with the window
//! at hand.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use gpui::{App, Global, Pixels, Window, px};
use kiln_shell::{DockPosition, DockState};

/// How much a command grows or shrinks a dock.
pub(crate) const STEP: Pixels = px(40.);

/// What a dock layout command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockLayoutOp {
    Grow,
    Shrink,
    /// Moves the terminal's dock to that edge.
    MoveTerminal(DockPosition),
    /// Moves the sidebar to that side.
    MoveSidebar(DockPosition),
}

/// The [`CommandTarget`] of dock layout commands: the op to do.
pub(crate) struct DockLayoutCommands;

impl CommandTarget for DockLayoutCommands {
    type Tx = ();
    type Target<'a> = Option<DockLayoutOp>;
}

struct Commands(Rc<Registry<DockLayoutCommands>>);

impl Global for Commands {}

/// Registers the dock layout commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<DockLayoutCommands> {
    use DockLayoutOp::*;
    use DockPosition::*;
    let commands = [
        ("dock.grow", "Grow Dock", Grow),
        ("dock.shrink", "Shrink Dock", Shrink),
        (
            "terminal.move_to_bottom",
            "Move Terminal to Bottom",
            MoveTerminal(Bottom),
        ),
        (
            "terminal.move_to_left",
            "Move Terminal to Left",
            MoveTerminal(Left),
        ),
        (
            "terminal.move_to_right",
            "Move Terminal to Right",
            MoveTerminal(Right),
        ),
        (
            "dock.move_sidebar_left",
            "Move Sidebar to Left",
            MoveSidebar(Left),
        ),
        (
            "dock.move_sidebar_right",
            "Move Sidebar to Right",
            MoveSidebar(Right),
        ),
    ];
    let mut registry = Registry::new();
    for (id, title, op) in commands {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<DockLayoutOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("dock layout command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// The op of `run`, if it's a dock layout command that applies in its
/// context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<DockLayoutOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

/// Grows `dock` by `by`, or shrinks it by less than nothing, within its
/// limits in `window`, and shows it.
pub(crate) fn resize(dock: &mut DockState, by: Pixels, window: &Window, cx: &App) {
    let size = dock.current_size() + by;
    dock.set_size(size.min(dock.max_size(window, cx)));
    dock.set_visible(true);
}

/// Where the other dock goes when a dock moves from `from` to `to`, the
/// other being at `other`: where it is, unless that's `to`, one dock per
/// edge; then to `from` if that's a side, else to the side opposite `to`.
pub(crate) fn displaced(from: DockPosition, to: DockPosition, other: DockPosition) -> DockPosition {
    use DockPosition::*;
    match (from, to) {
        _ if other != to => other,
        (Left | Right, _) => from,
        (Bottom, Left) => Right,
        (Bottom, Right | Bottom) => Left,
    }
}

/// A position's name in a saved layout.
pub(crate) fn position_name(position: DockPosition) -> &'static str {
    match position {
        DockPosition::Left => "left",
        DockPosition::Right => "right",
        DockPosition::Bottom => "bottom",
    }
}

/// The position named `name` in a saved layout.
pub(crate) fn position_named(name: &str) -> Option<DockPosition> {
    match name {
        "left" => Some(DockPosition::Left),
        "right" => Some(DockPosition::Right),
        "bottom" => Some(DockPosition::Bottom),
        _ => None,
    }
}
