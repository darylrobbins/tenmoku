//! Opening a folder or a file from the system's open panel.
//!
//! Its commands are those of [`init`]'s registry, in the workbench's
//! [`crate::KEY_CONTEXT`]. Their handlers only name an op: the workbench
//! then shows the panel, and does what its answer asks with the window at
//! hand: a folder re-roots the workbench in place
//! ([`crate::Workbench::set_root`]), and a file opens in the active pane.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use gpui::{App, Global, PathPromptOptions};

/// What an open command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenOp {
    /// Pick a folder to re-root the workbench at.
    Folder,
    /// Pick a file to open in the active pane.
    File,
}

impl OpenOp {
    /// How the open panel is shown for it: one folder, or one file.
    pub(crate) fn prompt(self) -> PathPromptOptions {
        let folder = self == OpenOp::Folder;
        PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some("Open".into()),
        }
    }
}

/// The [`CommandTarget`] of open commands: the op to do.
pub(crate) struct OpenCommands;

impl CommandTarget for OpenCommands {
    type Tx = ();
    type Target<'a> = Option<OpenOp>;
}

struct Commands(Rc<Registry<OpenCommands>>);

impl Global for Commands {}

/// Registers the open commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<OpenCommands> {
    let commands = [
        ("workbench.open_folder", "Open Folder…", OpenOp::Folder),
        ("workbench.open_file", "Open File…", OpenOp::File),
    ];
    let mut registry = Registry::new();
    for (id, title, op) in commands {
        registry
            .register(id, title, move |target: &mut Option<OpenOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("open command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// The op of `run`, if it's an open command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<OpenOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}
