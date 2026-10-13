//! The docks' sizes from the keyboard: `dock.grow` and `dock.shrink`
//! change the size of the dock holding focus (the terminal's while the
//! terminal has focus), else the sidebar's, by [`STEP`] within its limits
//! ([`kiln_shell::dock`]), showing it if hidden. They have no default keys:
//! they're for the palette and the user's keymap.
//!
//! Their handlers only name an op: the workbench does it with the window
//! at hand.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use gpui::{App, Global, Pixels, Window, px};
use kiln_shell::DockState;

/// How much a command grows or shrinks a dock.
pub(crate) const STEP: Pixels = px(40.);

/// What a dock size command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockSizeOp {
    Grow,
    Shrink,
}

/// The [`CommandTarget`] of dock size commands: the op to do.
pub(crate) struct DockSizeCommands;

impl CommandTarget for DockSizeCommands {
    type Tx = ();
    type Target<'a> = Option<DockSizeOp>;
}

struct Commands(Rc<Registry<DockSizeCommands>>);

impl Global for Commands {}

/// Registers the dock size commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<DockSizeCommands> {
    let commands = [
        ("dock.grow", "Grow Dock", DockSizeOp::Grow),
        ("dock.shrink", "Shrink Dock", DockSizeOp::Shrink),
    ];
    let mut registry = Registry::new();
    for (id, title, op) in commands {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<DockSizeOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("dock size command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// The op of `run`, if it's a dock size command that applies in its
/// context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<DockSizeOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

/// Grows or shrinks `dock` by [`STEP`], within its limits in `window`,
/// and shows it.
pub(crate) fn resize(dock: &mut DockState, op: DockSizeOp, window: &Window, cx: &App) {
    let size = match op {
        DockSizeOp::Grow => dock.current_size() + STEP,
        DockSizeOp::Shrink => dock.current_size() - STEP,
    };
    dock.set_size(size.min(dock.max_size(window, cx)));
    dock.set_visible(true);
}
