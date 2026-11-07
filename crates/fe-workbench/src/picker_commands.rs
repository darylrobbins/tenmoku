//! The pickers' commands: Show Buffers (`buffers.show`), Go to Tracked
//! File (`git.show_files`) and Open Config File… (`workbench.show_config_files`),
//! LazyVim's `<leader>fb` (and `<leader>,`, `<leader>bb`), `<leader>fg` and
//! `<leader>fc` in the `lazyvim` preset; and Search Keymaps
//! (`keymaps.search`), Search Diagnostics in File and in Workspace
//! (`diagnostics.search_document`, `diagnostics.search_workspace`) and
//! Search Lines in Buffer (`search.buffer_lines`), its `<leader>sk`,
//! `<leader>sd`, `<leader>sD` and `<leader>sb`.
//!
//! `Workbench`'s one `#[kiln::command]` block is `workbench_commands.rs`'s,
//! so the commands are declared on [`PickerCommands`], which no element
//! attaches, and the workbench's root element handles their actions
//! ([`Workbench::attach_picker_commands`]).

use gpui::{Context, Div, InteractiveElement as _};
use kiln::Cx;

use super::diagnostics_picker::Scope;
use crate::Workbench;

/// The commands' declaration: see the module's docs. They're handled by
/// the workbench's root element, not here.
#[allow(dead_code)]
struct PickerCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl PickerCommands {
    /// Show Buffers
    #[command(id = "buffers.show")]
    fn show_buffers(&mut self, _: &mut Cx<Self>) {}

    /// Go to Tracked File
    #[command(id = "git.show_files")]
    fn show_git_files(&mut self, _: &mut Cx<Self>) {}

    /// Open Config File…
    #[command(id = "workbench.show_config_files")]
    fn show_config_files(&mut self, _: &mut Cx<Self>) {}

    /// Search Keymaps
    #[command(id = "keymaps.search")]
    fn search_keymaps(&mut self, _: &mut Cx<Self>) {}

    /// Search Diagnostics in File
    #[command(id = "diagnostics.search_document")]
    fn search_document_diagnostics(&mut self, _: &mut Cx<Self>) {}

    /// Search Diagnostics in Workspace
    #[command(id = "diagnostics.search_workspace")]
    fn search_workspace_diagnostics(&mut self, _: &mut Cx<Self>) {}

    /// Search Lines in Buffer
    #[command(id = "search.buffer_lines")]
    fn search_buffer_lines(&mut self, _: &mut Cx<Self>) {}
}

impl Workbench {
    /// `root` handling the pickers' commands, in the workbench's key
    /// context.
    pub(super) fn attach_picker_commands(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(
            cx.listener(|workbench, _: &ShowBuffers, window, cx| {
                workbench.show_buffers(window, cx)
            }),
        )
        .on_action(cx.listener(|workbench, _: &ShowGitFiles, window, cx| {
            workbench.show_git_files(window, cx)
        }))
        .on_action(cx.listener(|workbench, _: &ShowConfigFiles, window, cx| {
            workbench.show_config_files(window, cx)
        }))
        .on_action(cx.listener(|workbench, _: &SearchKeymaps, window, cx| {
            workbench.show_keymaps(window, cx)
        }))
        .on_action(
            cx.listener(|workbench, _: &SearchDocumentDiagnostics, window, cx| {
                workbench.show_diagnostics(Scope::Document, window, cx)
            }),
        )
        .on_action(
            cx.listener(|workbench, _: &SearchWorkspaceDiagnostics, window, cx| {
                workbench.show_diagnostics(Scope::Workspace, window, cx)
            }),
        )
        .on_action(cx.listener(|workbench, _: &SearchBufferLines, window, cx| {
            workbench.show_buffer_lines(window, cx)
        }))
    }
}
