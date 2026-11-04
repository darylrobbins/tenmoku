//! fastedit's window shell, for any host: [`Workbench`] puts the editor
//! tabs ([`Pane`]s, split side by side or stacked in a [`PaneGroup`])
//! beside the lazy file tree on the right dock ([`ProjectPanel`]), and a
//! terminal on the bottom one ([`TerminalPanel`]).
//!
//! Its commands (`workbench.close_tab`, `workbench.next_tab`,
//! `workbench.previous_tab`, on the active pane, and the `pane.*` ones) are
//! in the [`KEY_CONTEXT`] context, registered by [`init`] and bound in
//! `fe-keymap`'s default keymap, as are the project panel's. They are
//! typed commands, `#[kiln::command]` methods on [`Workbench`] that its
//! root element handles; the few left in `fe-command` registries (the
//! search, project and Changes panels' own-context keys, the vim command
//! line's) a keystroke reaches as a [`fe_gpui::commands::RunCommand`] that
//! the panel takes. [`command_catalog`] checks them all with no app.

mod ai_changes_buffers;
mod ai_changes_panel;
mod auto_save;
mod chat_panel;
mod closed_editors;
mod config_problems;
mod disk_sync;
mod dock;
mod dock_focus;
mod dock_layout;
mod encoding_picker;
mod extension_snippets;
mod extensions_picker;
pub mod external_format;
mod find_bar;
mod find_commands;
mod git;
mod git_panel;
mod go_to_line;
mod grammar_prompt;
mod hierarchy_panel;
mod ignore_assist;
mod ignore_diagnostics;
mod inline_edit;
mod language_picker;
pub mod loader;
mod lsp;
mod lsp_code_actions;
mod lsp_consent;
mod lsp_consent_prompt;
mod lsp_format;
mod lsp_rename;
mod nav;
mod nav_picker;
mod open;
mod open_at;
mod outline_panel;
mod pane;
mod pane_drag;
mod pane_group;
mod pane_toolbar;
mod path_commands;
mod peek;
mod persist;
mod picker;
mod plugin_host;
mod plugin_prompt;
mod preserve_case;
mod project_panel;
mod quick_open;
mod references_panel;
mod save_commands;
mod search_panel;
mod settings_editor;
mod setup_wizard;
mod setup_wizard_model;
mod setup_wizard_preview;
mod shortcuts_editor;
pub mod spell;
mod spell_commands;
pub mod spell_ranges;
mod spell_squiggles;
mod status_bar;
mod symbol_picker;
mod tab_index_commands;
mod tab_switcher;
mod task_commands;
pub mod task_defs;
pub mod task_problems;
mod task_run;
mod terminal_panel;
mod theme_picker;
mod title_bar;
mod toolbar_picker;
mod trust;
pub mod update_check;
mod workbench;
mod workbench_commands;
mod workspace_edit;

use fe_command::{
    Actor, CommandMeta, CommandTarget, ContextPredicate, Dispatched, Effect, Registry, Value,
};
use fe_keymap::{CatalogError, CommandCatalog};
use gpui::App;

pub use dock::{Dock, DockPanel};
pub use extension_snippets::{
    ExtensionSnippet, extension_snippets, set_extension_snippets, snippet_body_error,
};
pub use git_panel::{GitPanel, GitPanelEvent};
pub use grammar_prompt::{
    FetchGrammar, GrammarInstallError, GrammarInstalled, GrammarOffer, GrammarOffers,
    InstallGrammar, PROMPT_FILE, set_grammar_offers,
};
/// The plugins' grant records, for a host to read and write.
pub use kiln_plugin::grants::GrantStore;
pub use kiln_shell::MenuBarView;
pub use lsp::{extension_servers, set_lsp_settings, set_server_launcher};
pub use lsp_consent::{
    CONSENT_FILE, ExtensionConsent, ExtensionServer, Invocation, PendingConsent,
    decline_extension_server, grant_extension_server, pending_extension_consents,
    set_extension_consent_file, set_extension_servers,
};
pub use open::{
    DocumentRoute, file_identity, open_documents, path_of_url, route_document, set_window_opener,
};
pub use outline_panel::OutlinePanel;
pub use pane::{Pane, PaneEvent, Tab};
pub use pane_group::{Axis, Direction, Member, Members, PaneGroup};
pub use persist::save_toggles_to;
pub use picker::{Picker, PickerDelegate};
pub use plugin_host::{
    CACHE_DIR, GRANTS_FILE, PluginDiagnostic, PluginInfo, PluginState, REBUILD, SECRETS_INDEX_FILE,
    StopReason, pending_plugin_grants, plugin_kind, plugin_states, reevaluate, set_plugin_dirs,
    set_plugin_extensions, unload_plugin,
};
pub use plugin_prompt::{
    PendingPluginGrant, PluginAnswer, PluginAsk, plugin_prompt_text, show_plugin_prompt,
};
pub use project_panel::{PANEL_WIDTH, PanelEvent, ProjectPanel};
pub use quick_open::QuickOpen;
pub use references_panel::ReferencesPanel;
pub use search_panel::SearchPanel;
pub use status_bar::{StatusBar, StatusItem, StatusSide};
pub use terminal_panel::TerminalPanel;
pub use title_bar::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS, titlebar_options};
pub use trust::{TrustChanged, set_trust_file};
pub use workbench::{Launch, RootChanged, Workbench, resolve_launch, window_layouts};

use chat_panel::ChatPanel;
use hierarchy_panel::HierarchyPanel;
use workbench::git_commands::{git_history::HistoryPanel, git_log::GitLogPanel};

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
    search_panel::init(cx);
    find_commands::init(cx);
    path_commands::init(cx);
    save_commands::init(cx);
    lsp::init(cx);
    grammar_prompt::init(cx);
    kiln_shell::menu_bar::register(cx);
    // `command_palette.toggle`, the palette listing these commands too.
    kiln_shell::command_palette::register(cx);
    // The Keyboard Shortcuts tab lists them too.
    let listed = command_list();
    cx.set_global(ListedCommands(listed.clone()));
    kiln_shell::command_palette::add_commands(cx, listed);
    // A vim map's `fe:` command of any of them isn't E492.
    fe_gpui::set_host_catalog(command_catalog());
    // And its which-key popup shows them by title.
    fe_gpui::set_host_commands(command_list());
    // Saved layouts restore their editors, and their viewers of files too
    // large to edit.
    kiln_shell::layout::register_item::<fe_gpui::EditorView>(cx);
    kiln_shell::layout::register_item::<fe_gpui::FileViewer>(cx);
    // ... and their image viewers.
    kiln_shell::layout::register_item::<fe_gpui::ImageViewer>(cx);
    // `app.quit` and `window.close` ask the windows' editors first.
    kiln_shell::close::init(cx);
    project_panel::init(cx);
    status_bar::init(cx);
    trust::init(cx);
    // The AI chat's view's commands, and `chat.open` from anywhere.
    fe_chat_view::init(cx);
    chat_panel::init(cx);
}

/// The workbench's, the editor's and the app's commands, to check keymap
/// bindings against: pass it to [`fe_keymap::KeymapStore::load`].
pub fn command_catalog() -> Box<dyn CommandCatalog> {
    // The typed commands, with no app to have registered them, ahead of
    // the registries: the one a registry still names
    // (`project_panel.reveal_active`) is checked as typed.
    let mut catalogs = vec![kiln::recorded_catalog("fe_workbench")];
    // The chat view's, which the default keymap binds (`chat.send`).
    catalogs.push(kiln::recorded_catalog("fe_chat_view"));
    catalogs.extend(registry_catalogs());
    Box::new(Catalog(catalogs))
}

/// The catalogs of the commands that live in registries, each once: the
/// window chrome's, the workbench's panels' own-context ones, and the
/// editor's.
fn registry_catalogs() -> Vec<Box<dyn CommandCatalog>> {
    vec![
        kiln_shell::picker::catalog(),
        Box::new(search_panel::registry()),
        Box::new(commit_box_registry()),
        kiln_shell::menu_bar::catalog(),
        kiln_shell::command_palette::catalog(),
        Box::new(project_panel::registry()),
        Box::new(status_bar::registry()),
        fe_gpui::commands::command_catalog(),
    ]
}

/// The commands the palette doesn't get from the typed ones: the
/// workbench's registries' and the editor's. It lists each where its
/// context holds.
pub fn command_list() -> Vec<CommandMeta> {
    let typed = kiln::recorded_catalog("fe_workbench");
    let mut list: Vec<CommandMeta> = (search_panel::registry().commands())
        .chain(commit_box_registry().commands())
        .chain(project_panel::registry().commands())
        .chain(status_bar::registry().commands())
        // A typed command a registry still names, for its panel to pass
        // on, the palette lists as typed.
        .filter(|meta| typed.check(&meta.id, &Value::Null) == Err(CatalogError::UnknownCommand))
        .cloned()
        .collect();
    list.extend(fe_gpui::commands::command_list());
    list.extend(workbench_commands::markdown_preview::palette_commands());
    list
}

/// [`command_list`] as [`init`] gave it to the palette, for the Keyboard
/// Shortcuts tab to list without building the registries again.
pub(crate) struct ListedCommands(pub(crate) Vec<CommandMeta>);

impl gpui::Global for ListedCommands {}

/// The [`CommandTarget`] of the Changes panel's commit box key: nothing,
/// as the panel runs it itself.
struct CommitBoxCommands;

impl CommandTarget for CommitBoxCommands {
    type Tx = ();
    type Target<'a> = ();
}

/// The commit box's key, [`git_panel::COMMIT_COMMAND`], in the Changes
/// panel's own context: the panel takes the [`fe_gpui::commands::RunCommand`]
/// it sends, so it stays a registry command, here for the keymap and the
/// palette. The workbench's git commands are typed.
fn commit_box_registry() -> Registry<CommitBoxCommands> {
    let mut registry = Registry::new();
    registry
        .register(
            git_panel::COMMIT_COMMAND,
            "Commit from the Message Box",
            |_: &mut (), _: &Actor| Ok(Dispatched::new(Effect::Notify)),
        )
        .expect("the commit box's command ID is valid")
        .context(ContextPredicate::identifier(git_panel::KEY_CONTEXT));
    registry
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
mod ai_changes_panel_tests;
#[cfg(test)]
mod close_others_tests;
#[cfg(test)]
mod closed_editors_tests;
#[cfg(test)]
mod command_catalog_tests;
#[cfg(test)]
mod extension_snippets_tests;
#[cfg(test)]
mod external_format_tests;
#[cfg(test)]
mod external_format_wiring_tests;
#[cfg(test)]
mod grammar_prompt_tests;
#[cfg(test)]
mod list_vim_keys_tests;
#[cfg(test)]
mod lsp_consent_relaunch_tests;
#[cfg(test)]
mod lsp_consent_tests;
#[cfg(test)]
mod middle_click_close_tests;
#[cfg(test)]
mod panel_width_tests;
#[cfg(test)]
mod path_commands_tests;
#[cfg(test)]
mod save_commands_tests;
#[cfg(test)]
mod settings_editor_controls_tests;
#[cfg(test)]
mod settings_editor_tests;
#[cfg(test)]
mod setup_wizard_model_tests;
#[cfg(test)]
mod setup_wizard_preview_tests;
#[cfg(test)]
mod setup_wizard_tests;
#[cfg(test)]
mod shortcuts_editor_tests;
#[cfg(test)]
mod spell_commands_tests;
#[cfg(test)]
mod spell_ranges_tests;
#[cfg(test)]
mod spell_squiggles_tests;
#[cfg(test)]
mod spell_tests;
#[cfg(test)]
mod tab_index_commands_tests;
#[cfg(test)]
mod task_commands_tests;
#[cfg(test)]
mod task_defs_tests;
#[cfg(test)]
mod task_problems_panel_tests;
#[cfg(test)]
mod task_problems_tests;
#[cfg(test)]
mod task_run_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod theme_tests;
#[cfg(test)]
mod typed_command_ids_tests;
#[cfg(test)]
mod update_check_tests;
#[cfg(test)]
mod update_hook_tests;
#[cfg(test)]
mod workbench_commands_tests;
#[cfg(test)]
mod zen_mode_escape_tests;
#[cfg(test)]
mod zen_mode_tests;
