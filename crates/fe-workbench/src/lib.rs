//! fastedit's window shell, for any host: [`Workbench`] puts the editor
//! tabs ([`Pane`]s, split side by side or stacked in a [`PaneGroup`])
//! beside the lazy file tree on the right dock ([`ProjectPanel`]), and a
//! terminal on the bottom one ([`TerminalPanel`]).
//!
//! Its commands (`workbench.close_tab`, `workbench.next_tab`,
//! `workbench.previous_tab`, on the active pane, and the `pane.*` ones) are
//! in the [`KEY_CONTEXT`] context, registered by [`init`] and bound in
//! `fe-keymap`'s default keymap, as are the project panel's. The tab
//! commands are typed commands, `#[kiln::command]` methods on [`Workbench`]
//! that its root element handles; the others are `fe-command` commands,
//! which a keystroke reaches as a [`fe_gpui::commands::RunCommand`] that the
//! focused editor doesn't know and propagates.

mod auto_save;
mod config_problems;
mod disk_sync;
mod dock;
mod dock_layout;
mod find_bar;
mod git;
mod git_panel;
mod go_to_line;
mod language_picker;
pub mod loader;
mod lsp;
mod lsp_code_actions;
mod lsp_format;
mod lsp_rename;
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
mod terminal_panel;
mod title_bar;
mod toolbar_picker;
mod trust;
mod workbench;
mod workbench_commands;
mod workspace_edit;

use fe_command::{CommandMeta, Value};
use fe_keymap::{CatalogError, CommandCatalog};
use gpui::App;

pub use dock::{Dock, DockPanel};
pub use git_panel::{GitPanel, GitPanelEvent};
pub use kiln_shell::MenuBarView;
pub use lsp::{set_lsp_settings, set_server_launcher};
pub use open::set_window_opener;
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
pub use terminal_panel::TerminalPanel;
pub use title_bar::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS, titlebar_options};
pub use trust::{TrustChanged, set_trust_file};
pub use workbench::{Launch, RootChanged, WELCOME, Workbench, resolve_launch, window_layouts};

/// The key context the workbench's root element sets.
pub const KEY_CONTEXT: &str = "workbench";

// The crate's typed commands (`#[kiln::command]`), which `init` registers.
kiln::module!(pub(crate) fn module);

/// The global the tests that set up once (`tests/lsp.rs`,
/// `tests/lsp_quit.rs`) look for to tell whether [`init`] has run: kiln's
/// contributions, which [`module`], its first call, sets.
#[cfg(test)]
pub(crate) use kiln::Contributions as Commands;

/// Registers the workbench's commands. Call after [`fe_gpui::init`].
pub fn init(cx: &mut App) {
    // The typed commands first, registered at once whether or not
    // `kiln::init` has run: the test apps never run it.
    module(cx);
    picker::init(cx);
    config_problems::init(cx);
    dock::init(cx);
    lsp::init(cx);
    workbench::git_commands::init(cx);
    kiln_shell::menu_bar::register(cx);
    // `command_palette.toggle`, the palette listing these commands too.
    kiln_shell::command_palette::register(cx);
    kiln_shell::command_palette::add_commands(cx, command_list());
    // Saved layouts restore their editors, and their viewers of files too
    // large to edit.
    kiln_shell::layout::register_item::<fe_gpui::EditorView>(cx);
    kiln_shell::layout::register_item::<fe_gpui::FileViewer>(cx);
    // `app.quit` and `window.close` ask the windows' editors first.
    kiln_shell::close::init(cx);
    nav::init(cx);
    open::init(cx);
    outline_panel::init(cx);
    pane_toolbar::init(cx);
    project_panel::init(cx);
    references_panel::init(cx);
    status_bar::init(cx);
    terminal_panel::init(cx);
    trust::init(cx);
}

/// The workbench's, the editor's and the app's commands, to check keymap
/// bindings against: pass it to [`fe_keymap::KeymapStore::load`].
pub fn command_catalog() -> Box<dyn CommandCatalog> {
    // The typed commands, with no app to have registered them.
    let mut catalogs = vec![kiln::recorded_catalog("fe_workbench")];
    catalogs.extend(registry_catalogs());
    Box::new(Catalog(catalogs))
}

/// The catalogs of the commands that live in registries, the workbench's,
/// the editor's and the app's.
fn registry_catalogs() -> Vec<Box<dyn CommandCatalog>> {
    vec![
        Box::new(picker::registry()),
        kiln_shell::picker::catalog(),
        Box::new(config_problems::registry()),
        Box::new(dock::registry()),
        Box::new(find_bar::registry()),
        Box::new(lsp::registry()),
        Box::new(workbench::git_commands::registry()),
        kiln_shell::menu_bar::catalog(),
        kiln_shell::command_palette::catalog(),
        Box::new(nav::registry()),
        Box::new(open::registry()),
        Box::new(outline_panel::registry()),
        Box::new(pane_toolbar::registry()),
        Box::new(project_panel::registry()),
        Box::new(references_panel::registry()),
        Box::new(status_bar::registry()),
        Box::new(terminal_panel::registry()),
        Box::new(trust::registry()),
        fe_gpui::commands::command_catalog(),
    ]
}

/// The workbench's commands and the editor's, for the command palette,
/// which lists each where its context holds; it lists the typed commands
/// itself.
fn command_list() -> Vec<CommandMeta> {
    let mut list: Vec<CommandMeta> = picker::registry().commands().cloned().collect();
    list.extend(config_problems::registry().commands().cloned());
    list.extend(dock::registry().commands().cloned());
    list.extend(find_bar::registry().commands().cloned());
    list.extend(lsp::registry().commands().cloned());
    list.extend(workbench::git_commands::registry().commands().cloned());
    list.extend(nav::registry().commands().cloned());
    list.extend(open::registry().commands().cloned());
    list.extend(outline_panel::registry().commands().cloned());
    list.extend(pane_toolbar::registry().commands().cloned());
    list.extend(project_panel::registry().commands().cloned());
    list.extend(references_panel::registry().commands().cloned());
    list.extend(status_bar::registry().commands().cloned());
    list.extend(terminal_panel::registry().commands().cloned());
    list.extend(trust::registry().commands().cloned());
    list.extend(fe_gpui::commands::command_list());
    list
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
mod panel_width_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod typed_command_ids_tests;
#[cfg(test)]
mod workbench_commands_tests;
