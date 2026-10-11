//! The palettes' picker is kiln-shell's ([`kiln_shell::picker`]): a
//! [`TextInput`](fe_ui::TextInput) whose text a [`PickerDelegate`]
//! matches, the matches below it, one selected. Quick open is a delegate,
//! and so are the language, toolbar and navigation pickers.
//!
//! The picker's keys (`picker.*`, and `text_input.*` for any input) are
//! kiln-shell's; [`init`] installs them. What's fastedit's is
//! `quick_open.toggle`, in [`registry`], in the workbench's
//! [`crate::KEY_CONTEXT`]: its handler only says it ran, and the workbench
//! does it.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use gpui::{App, Global};

#[cfg(test)]
pub(crate) use kiln_shell::picker::VISIBLE_ROWS;
pub use kiln_shell::picker::{Picker, PickerDelegate};
pub(crate) use kiln_shell::picker::{PickerOp as PaletteOp, op};

/// The [`CommandTarget`] of `quick_open.toggle`: whether it ran.
pub(crate) struct PaletteCommands;

impl CommandTarget for PaletteCommands {
    type Tx = ();
    type Target<'a> = bool;
}

struct Commands(Rc<Registry<PaletteCommands>>);

impl Global for Commands {}

/// Installs the picker's commands and `quick_open.toggle`.
pub(crate) fn init(cx: &mut App) {
    kiln_shell::picker::init(cx);
    cx.set_global(Commands(Rc::new(registry())));
}

/// `quick_open.toggle`. The picker's own commands are in
/// [`kiln_shell::picker::registry`].
pub(crate) fn registry() -> Registry<PaletteCommands> {
    let mut registry = Registry::new();
    registry
        .register(
            "quick_open.toggle",
            "Go to File",
            |ran: &mut bool, _: &Actor| {
                *ran = true;
                Ok(Dispatched::new(Effect::Notify))
            },
        )
        .expect("the quick open command ID is valid")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
}

/// Whether `run` is `quick_open.toggle`, in a context where it applies.
pub(crate) fn toggles_quick_open(run: &RunCommand, cx: &App) -> bool {
    let Some(commands) = cx.try_global::<Commands>() else {
        return false;
    };
    let registry = commands.0.clone();
    if registry.get(&run.0.command).is_none() {
        return false;
    }
    let mut ran = false;
    if let Err(error) = registry.dispatch(&mut ran, &run.0) {
        log::warn!("{error}");
    }
    ran
}
