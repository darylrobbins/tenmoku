//! The palettes' picker is kiln-shell's ([`kiln_shell::picker`]): a
//! [`TextInput`](fe_ui::TextInput) whose text a [`PickerDelegate`]
//! matches, the matches below it, one selected. Quick open is a delegate,
//! and so are the language, toolbar and navigation pickers.
//!
//! The picker's keys (`picker.*`, and `text_input.*` for any input) are
//! kiln-shell's; [`init`] installs them. What's fastedit's is
//! `quick_open.toggle` and `go_to_line.toggle`, in [`registry`], in the workbench's
//! [`crate::KEY_CONTEXT`]: its handler only says it ran, and the workbench
//! does it.
//!
//! With vim on, the pickers have telescope's insert and normal modes
//! (`kiln_shell::picker`'s module docs): [`init`] hands kiln-shell
//! [`fe_gpui::vim_enabled`] as its switch.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use gpui::{App, Global};

#[cfg(test)]
pub(crate) use kiln_shell::picker::VISIBLE_ROWS;
pub use kiln_shell::picker::{Picker, PickerDelegate};
pub(crate) use kiln_shell::picker::{PickerOp as PaletteOp, op};

/// A palette the workbench's own commands toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Toggle {
    QuickOpen,
    GoToLine,
}

/// The [`CommandTarget`] of `quick_open.toggle` and `go_to_line.toggle`:
/// the palette that ran, if one did.
pub(crate) struct PaletteCommands;

impl CommandTarget for PaletteCommands {
    type Tx = ();
    type Target<'a> = Option<Toggle>;
}

struct Commands(Rc<Registry<PaletteCommands>>);

impl Global for Commands {}

/// Installs the picker's commands and `quick_open.toggle`, and has the
/// pickers take vim keys while vim is on ([`fe_gpui::vim_enabled`]).
pub(crate) fn init(cx: &mut App) {
    kiln_shell::picker::init(cx);
    kiln_shell::picker::set_vim_keys(cx, fe_gpui::vim_enabled);
    cx.set_global(Commands(Rc::new(registry())));
}

/// `quick_open.toggle` and `go_to_line.toggle`. The picker's own commands are in
/// [`kiln_shell::picker::registry`].
pub(crate) fn registry() -> Registry<PaletteCommands> {
    let mut registry = Registry::new();
    registry
        .register(
            "quick_open.toggle",
            "Go to File",
            |ran: &mut Option<Toggle>, _: &Actor| {
                *ran = Some(Toggle::QuickOpen);
                Ok(Dispatched::new(Effect::Notify))
            },
        )
        .expect("the quick open command ID is valid")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
        .register(
            "go_to_line.toggle",
            "Go to Line/Column…",
            |ran: &mut Option<Toggle>, _: &Actor| {
                *ran = Some(Toggle::GoToLine);
                Ok(Dispatched::new(Effect::Notify))
            },
        )
        .expect("the go to line command ID is valid")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
}

/// The palette `run` toggles, if it's one of the workbench's toggle
/// commands, in a context where it applies.
pub(crate) fn toggled(run: &RunCommand, cx: &App) -> Option<Toggle> {
    let commands = cx.try_global::<Commands>()?;
    let registry = commands.0.clone();
    registry.get(&run.0.command)?;
    let mut ran = None;
    if let Err(error) = registry.dispatch(&mut ran, &run.0) {
        log::warn!("{error}");
    }
    ran
}
