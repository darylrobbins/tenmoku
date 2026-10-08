//! fastedit's window shell, for any host: [`Workbench`] puts the editor
//! tabs ([`Pane`]s, split side by side or stacked in a [`PaneGroup`])
//! beside the lazy file tree on the right dock ([`ProjectPanel`]).
//!
//! Its commands (`workbench.close_tab`, `workbench.next_tab`,
//! `workbench.previous_tab`, on the active pane, and the `pane.*` ones) are
//! `fe-command` commands in the [`KEY_CONTEXT`] context, registered by
//! [`init`] and bound in
//! `fe-keymap`'s default keymap, as are the project panel's. A keystroke reaches them as a
//! [`fe_gpui::commands::RunCommand`] that the focused editor doesn't know
//! and propagates.

mod dock;
mod language_picker;
pub mod loader;
mod lsp;
mod nav;
mod nav_picker;
mod open;
mod outline_panel;
mod pane;
mod pane_drag;
mod pane_group;
mod pane_toolbar;
mod persist;
mod picker;
mod project_panel;
mod quick_open;
mod references_panel;
mod search_panel;
mod status_bar;
mod title_bar;
mod toolbar_picker;
mod trust;
mod workbench;

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry, Value};
use fe_keymap::{CatalogError, CommandCatalog};
use gpui::{App, Global, Window};

pub use dock::{Dock, DockPanel};
pub use lsp::{set_lsp_settings, set_server_launcher};
pub use outline_panel::OutlinePanel;
pub use pane::{Pane, PaneEvent, Tab};
pub use pane_group::{Axis, Direction, Member, Members, PaneGroup};
pub use persist::save_toggles_to;
pub use picker::{Picker, PickerDelegate};
pub use project_panel::{PANEL_WIDTH, PanelEvent, ProjectPanel};
pub use quick_open::QuickOpen;
pub use references_panel::ReferencesPanel;
pub use search_panel::SearchPanel;
pub use status_bar::{StatusBar, StatusItem, StatusSide};
pub use title_bar::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS, titlebar_options};
pub use trust::{TrustChanged, set_trust_file};
pub use workbench::{Launch, RootChanged, WELCOME, Workbench, resolve_launch};

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
    lsp::init(cx);
    nav::init(cx);
    open::init(cx);
    outline_panel::init(cx);
    pane_group::init(cx);
    pane_toolbar::init(cx);
    project_panel::init(cx);
    references_panel::init(cx);
    status_bar::init(cx);
    trust::init(cx);
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
    Box::new(Catalog(vec![
        Box::new(new_registry()),
        Box::new(picker::registry()),
        Box::new(dock::registry()),
        Box::new(lsp::registry()),
        Box::new(nav::registry()),
        Box::new(open::registry()),
        Box::new(outline_panel::registry()),
        Box::new(pane_group::registry()),
        Box::new(pane_toolbar::registry()),
        Box::new(project_panel::registry()),
        Box::new(references_panel::registry()),
        Box::new(status_bar::registry()),
        Box::new(trust::registry()),
        fe_gpui::commands::command_catalog(),
    ]))
}

/// Each catalog in turn, until one knows the command.
struct Catalog(Vec<Box<dyn CommandCatalog>>);

impl CommandCatalog for Catalog {
    fn check(&self, command: &fe_command::CommandId, args: &Value) -> Result<(), CatalogError> {
        for catalog in &self.0 {
            match catalog.check(command, args) {
                Err(CatalogError::UnknownCommand) => {}
                result => return result,
            }
        }
        Err(CatalogError::UnknownCommand)
    }
}

#[cfg(test)]
mod tests;
