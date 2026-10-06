//! fastedit's window shell, for any host: [`Workbench`] puts the editor
//! tabs ([`Pane`]) beside the lazy file tree on the right dock
//! ([`ProjectPanel`]).
//!
//! Its commands (`workbench.close_tab`, `workbench.next_tab`,
//! `workbench.previous_tab`) are `fe-command` commands in the
//! [`KEY_CONTEXT`] context, registered by [`init`] and bound in
//! `fe-keymap`'s default keymap. A keystroke reaches them as a
//! [`fe_gpui::commands::RunCommand`] that the focused editor doesn't know
//! and propagates.

mod dock;
pub mod loader;
mod outline_panel;
mod pane;
mod picker;
mod project_panel;
mod quick_open;
mod search_panel;
mod workbench;

use std::rc::Rc;

use dock::DockCommands;
use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry, Value};
use fe_keymap::{CatalogError, CommandCatalog};
use gpui::{App, Global, Window};
use outline_panel::OutlineCommands;
use picker::PaletteCommands;

pub use dock::{Dock, DockPanel};
pub use outline_panel::OutlinePanel;
pub use pane::{Pane, Tab};
pub use picker::{Picker, PickerDelegate};
pub use project_panel::{PANEL_WIDTH, PanelEvent, ProjectPanel};
pub use quick_open::QuickOpen;
pub use search_panel::SearchPanel;
pub use workbench::{Launch, WELCOME, Workbench, resolve_launch};

/// The key context the workbench's root element sets.
pub const KEY_CONTEXT: &str = "workbench";

/// The [`CommandTarget`] of workbench commands: the pane, in its window.
pub struct WorkbenchCommands;

impl CommandTarget for WorkbenchCommands {
    type Tx = ();
    type Target<'a> = PaneTarget<'a>;
}

type PaneTarget<'a> = (&'a mut Pane, &'a mut Window, &'a mut App);

type PaneCommand = fn(&mut Pane, &mut Window, &mut App);

struct Commands(Rc<Registry<WorkbenchCommands>>);

impl Global for Commands {}

/// Registers the workbench's commands. Call after [`fe_gpui::init`].
pub fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(new_registry())));
    picker::init(cx);
    dock::init(cx);
    outline_panel::init(cx);
}

fn registry(cx: &App) -> Option<Rc<Registry<WorkbenchCommands>>> {
    cx.try_global::<Commands>()
        .map(|commands| commands.0.clone())
}

fn new_registry() -> Registry<WorkbenchCommands> {
    let mut registry = Registry::new();
    let commands: [(&str, &str, PaneCommand); 3] = [
        ("workbench.close_tab", "Close Tab", |pane, window, cx| {
            pane.close_active(window, cx)
        }),
        ("workbench.next_tab", "Next Tab", |pane, window, cx| {
            pane.cycle(false, window, cx)
        }),
        (
            "workbench.previous_tab",
            "Previous Tab",
            |pane, window, cx| pane.cycle(true, window, cx),
        ),
    ];
    for (id, title, run) in commands {
        registry
            .register(id, title, move |target: &mut PaneTarget, _: &Actor| {
                let (pane, window, cx) = target;
                run(pane, window, cx);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("workbench command IDs are valid and distinct")
            .context(ContextPredicate::identifier(KEY_CONTEXT));
    }
    registry
}

/// The workbench's, the editor's and the app's commands, to check keymap
/// bindings against: pass it to [`fe_keymap::KeymapStore::load`].
pub fn command_catalog() -> Box<dyn CommandCatalog> {
    Box::new(Catalog {
        workbench: new_registry(),
        palette: picker::registry(),
        dock: dock::registry(),
        outline: outline_panel::registry(),
        rest: fe_gpui::commands::command_catalog(),
    })
}

struct Catalog {
    workbench: Registry<WorkbenchCommands>,
    palette: Registry<PaletteCommands>,
    dock: Registry<DockCommands>,
    outline: Registry<OutlineCommands>,
    rest: Box<dyn CommandCatalog>,
}

impl CommandCatalog for Catalog {
    fn check(&self, command: &fe_command::CommandId, args: &Value) -> Result<(), CatalogError> {
        match self.workbench.check(command, args) {
            Err(CatalogError::UnknownCommand) => match self.palette.check(command, args) {
                Err(CatalogError::UnknownCommand) => match self.dock.check(command, args) {
                    Err(CatalogError::UnknownCommand) => match self.outline.check(command, args) {
                        Err(CatalogError::UnknownCommand) => self.rest.check(command, args),
                        result => result,
                    },
                    result => result,
                },
                result => result,
            },
            result => result,
        }
    }
}

#[cfg(test)]
mod tests;
