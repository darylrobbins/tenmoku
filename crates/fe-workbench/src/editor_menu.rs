//! What the editor's right-click menu offers (T1897): fe-gpui draws the
//! menu ([`fe_gpui::set_editor_menu_provider`]); the workbench says what's
//! in it, for the editor right-clicked.
//!
//! For an editor of one of the window's panes, in groups, each separated
//! from the next and left out when it has nothing:
//!
//! 1. Go to Definition, Peek Definition, Go to Implementation, Find
//!    References, Peek References;
//! 2. Rename Symbol, Code Actions… (once the server has offered some, and
//!    elsewhere with the language server's items without Rename, first
//!    with none: [`code_actions`]), Format Document, Format Selection;
//! 3. Cut, Copy, Paste;
//! 4. Copy Path, Copy Reference;
//! 5. Run Test (or Run Tests), Toggle Breakpoint, Annotate with Git Blame
//!    (or Hide Git Blame), Ask Agent About Selection (or This Line): see
//!    [`context`].
//!
//! - **Language server items (1, 2)** show when the buffer's language has a
//!   configured server. While no server the buffer is on has initialized
//!   they're disabled; once one has, an item no such server provides is
//!   left out. Format Document also shows, enabled, when a plugin would
//!   format the buffer, as `lsp.format_document` would pick it. Format
//!   Selection needs a selection that isn't empty.
//! - **Cut and Paste** are left out of a read-only editor; Copy always shows.
//! - **Copy Path and Copy Reference** need a buffer with a path.
//!
//! A right-click in a pane that isn't the active one makes it active first,
//! so every item acts on the editor clicked. Other editors in the window (a
//! diff view's sides, a peek's editor) get Cut, Copy and Paste only. Each
//! item shows its key as the editor's context had it at the press, and runs
//! there, through fe-gpui.

use std::rc::Rc;

use fe_command::{ContextStack, Invocation};
use fe_gpui::{EditorMenuItem, EditorMenuRequest, EditorView};
use fe_lsp::LspManager;
use gpui::{App, Context, Entity, SharedString, Window};
use kiln_l10n::tr;
use lsp_types::{
    CodeActionProviderCapability, ImplementationProviderCapability, OneOf, ServerCapabilities,
};

use crate::Workbench;

mod code_actions;
mod context;

#[cfg(test)]
pub(crate) use code_actions::answered as code_actions_answered;

/// Registers the workbench's items as every editor's right-click menu.
pub(crate) fn init(cx: &mut App) {
    fe_gpui::set_editor_menu_provider(cx, Rc::new(entries));
}

/// The menu for `request`, from the workbench of its window; none outside
/// one.
fn entries(
    request: &EditorMenuRequest,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Option<EditorMenuItem>> {
    let Some(Some(workbench)) = window.root::<Workbench>() else {
        return Vec::new();
    };
    let Some(editor) = request.editor.upgrade() else {
        return Vec::new();
    };
    workbench.update(cx, |workbench, cx| {
        workbench.editor_menu(request, &editor, window, cx)
    })
}

/// One of the menu's items: a command, titled for menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    GoToDefinition,
    PeekDefinition,
    GoToImplementation,
    FindReferences,
    PeekReferences,
    Rename,
    FormatDocument,
    FormatSelection,
    Cut,
    Copy,
    Paste,
    CopyPath,
    CopyReference,
    CodeActions,
    RunTest,
    RunTests,
    ToggleBreakpoint,
    AnnotateBlame,
    HideBlame,
    AskAboutSelection,
    AskAboutLine,
}

impl Item {
    fn command(self) -> &'static str {
        match self {
            Item::GoToDefinition => "nav.goto_definition",
            Item::PeekDefinition => "editor.peek_definition",
            Item::GoToImplementation => "nav.goto_implementation",
            Item::FindReferences => "nav.find_references",
            Item::PeekReferences => "editor.peek_references",
            Item::Rename => "lsp.rename",
            Item::FormatDocument => "lsp.format_document",
            Item::FormatSelection => "lsp.format_selection",
            Item::Cut => "editor.cut",
            Item::Copy => "editor.copy",
            Item::Paste => "editor.paste",
            Item::CopyPath => "workbench.copy_active_file_path",
            Item::CopyReference => "workbench.copy_reference",
            Item::CodeActions => "lsp.code_actions",
            Item::RunTest | Item::RunTests => "tests.run_at_cursor",
            Item::ToggleBreakpoint => "debug.toggle_breakpoint",
            Item::AnnotateBlame | Item::HideBlame => "git.toggle_blame_annotations",
            Item::AskAboutSelection | Item::AskAboutLine => "chat.ask_about_selection",
        }
    }

    /// Its title in a menu, which has no palette category.
    fn title(self) -> SharedString {
        match self {
            Item::GoToDefinition => tr!("workbench-editor-menu-go-to-definition"),
            Item::PeekDefinition => tr!("workbench-editor-menu-peek-definition"),
            Item::GoToImplementation => tr!("workbench-editor-menu-go-to-implementation"),
            Item::FindReferences => tr!("workbench-editor-menu-find-references"),
            Item::PeekReferences => tr!("workbench-editor-menu-peek-references"),
            Item::Rename => tr!("workbench-editor-menu-rename"),
            Item::FormatDocument => tr!("workbench-editor-menu-format-document"),
            Item::FormatSelection => tr!("workbench-editor-menu-format-selection"),
            Item::Cut => tr!("workbench-editor-menu-cut"),
            Item::Copy => tr!("workbench-editor-menu-copy"),
            Item::Paste => tr!("workbench-editor-menu-paste"),
            Item::CopyPath => tr!("workbench-editor-menu-copy-path"),
            Item::CopyReference => tr!("workbench-editor-menu-copy-reference"),
            Item::CodeActions => tr!("workbench-editor-menu-code-actions"),
            Item::RunTest => tr!("workbench-editor-menu-run-test"),
            Item::RunTests => tr!("workbench-editor-menu-run-tests"),
            Item::ToggleBreakpoint => tr!("workbench-editor-menu-toggle-breakpoint"),
            Item::AnnotateBlame => tr!("workbench-editor-menu-annotate-blame"),
            Item::HideBlame => tr!("workbench-editor-menu-hide-blame"),
            Item::AskAboutSelection => tr!("workbench-editor-menu-ask-about-selection"),
            Item::AskAboutLine => tr!("workbench-editor-menu-ask-about-line"),
        }
    }

    /// The item, its key read in `contexts`.
    fn entry(self, enabled: bool, contexts: &ContextStack, cx: &App) -> EditorMenuItem {
        let command = self.command();
        EditorMenuItem {
            title: self.title(),
            shortcut: crate::pane_toolbar::menu_shortcut(command, contexts, cx).map(Into::into),
            invocation: Invocation::of(command).expect("a menu item's command id parses"),
            enabled,
        }
    }

    /// Whether a server with `capabilities` provides it; true for an item
    /// that isn't a language server's.
    fn provided_by(self, capabilities: &ServerCapabilities) -> bool {
        match self {
            Item::GoToDefinition | Item::PeekDefinition => {
                offered(&capabilities.definition_provider)
            }
            Item::GoToImplementation => !matches!(
                capabilities.implementation_provider,
                None | Some(ImplementationProviderCapability::Simple(false))
            ),
            Item::FindReferences | Item::PeekReferences => {
                offered(&capabilities.references_provider)
            }
            Item::Rename => offered(&capabilities.rename_provider),
            Item::FormatDocument => offered(&capabilities.document_formatting_provider),
            Item::FormatSelection => offered(&capabilities.document_range_formatting_provider),
            Item::CodeActions => !matches!(
                capabilities.code_action_provider,
                None | Some(CodeActionProviderCapability::Simple(false))
            ),
            Item::Cut
            | Item::Copy
            | Item::Paste
            | Item::CopyPath
            | Item::CopyReference
            | Item::RunTest
            | Item::RunTests
            | Item::ToggleBreakpoint
            | Item::AnnotateBlame
            | Item::HideBlame
            | Item::AskAboutSelection
            | Item::AskAboutLine => true,
        }
    }
}

/// Whether a capability given as a flag or options offers the feature.
fn offered<T>(provider: &Option<OneOf<bool, T>>) -> bool {
    !matches!(provider, None | Some(OneOf::Left(false)))
}

const NAVIGATION: &[Item] = &[
    Item::GoToDefinition,
    Item::PeekDefinition,
    Item::GoToImplementation,
    Item::FindReferences,
    Item::PeekReferences,
];

const REFACTORING: &[Item] = &[Item::Rename, Item::FormatDocument, Item::FormatSelection];

/// What the buffer's servers can do for the menu.
enum Servers {
    /// Its language has no configured server.
    None,
    /// It has one, but no server the buffer is on has initialized.
    Starting,
    /// The capabilities of each server the buffer is on that has.
    Ready(Vec<ServerCapabilities>),
}

impl Workbench {
    /// The menu for `request` on `editor`: see the [module](self).
    fn editor_menu(
        &mut self,
        request: &EditorMenuRequest,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Option<EditorMenuItem>> {
        let contexts = &request.contexts;
        let clipboard = |entries: &mut Vec<Option<EditorMenuItem>>, cx: &App| {
            entries.push(None);
            for item in [Item::Cut, Item::Copy, Item::Paste] {
                if request.read_only && item != Item::Copy {
                    continue;
                }
                entries.push(Some(item.entry(true, contexts, cx)));
            }
        };
        let mut entries = Vec::new();
        let pane = (self.panes(cx).into_iter()).find(|pane| {
            (pane.read(cx).tabs().iter())
                .any(|tab| tab.editor().is_some_and(|shown| shown == editor))
        });
        let Some(pane) = pane else {
            clipboard(&mut entries, cx);
            return entries;
        };
        if pane != *self.pane() {
            self.activate(pane, window, cx);
        }
        let servers = self.editor_menu_servers(editor, cx);
        let view = editor.read(cx);
        let plugin_formats = crate::plugin_format::formatter(view, cx).is_some();
        let selected = !view.model().primary_selection().range.is_empty();
        let has_path = view.model().path().is_some();
        for group in [NAVIGATION, REFACTORING] {
            entries.push(None);
            for &item in group {
                let plugin = item == Item::FormatDocument && plugin_formats;
                if item == Item::FormatSelection && !selected {
                    continue;
                }
                let enabled = match &servers {
                    _ if plugin => true,
                    Servers::None => continue,
                    Servers::Starting => false,
                    Servers::Ready(capabilities) => {
                        if !capabilities.iter().any(|caps| item.provided_by(caps)) {
                            continue;
                        }
                        true
                    }
                };
                entries.push(Some(item.entry(enabled, contexts, cx)));
            }
        }
        clipboard(&mut entries, cx);
        if has_path {
            entries.push(None);
            for item in [Item::CopyPath, Item::CopyReference] {
                entries.push(Some(item.entry(true, contexts, cx)));
            }
        }
        entries.push(None);
        entries.extend(
            self.editor_menu_context(editor, contexts, cx)
                .into_iter()
                .map(Some),
        );
        if let Servers::Ready(capabilities) = &servers {
            self.ask_editor_menu_code_actions(request, editor, capabilities, &entries, cx);
        }
        entries
    }

    /// What `editor`'s buffer's servers can do, as the manager knows now:
    /// read through every server it's on, not only its lead.
    fn editor_menu_servers(&self, editor: &Entity<EditorView>, cx: &App) -> Servers {
        let Some(manager) = self.lsp.manager() else {
            return Servers::None;
        };
        let manager = manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let view = editor.read(cx);
        let buffer = view.model().buffer().id();
        if !configured(&manager, view, buffer) {
            return Servers::None;
        }
        let ready: Vec<ServerCapabilities> = (manager.servers_of(buffer).into_iter())
            .filter_map(|server| manager.capabilities(server))
            .collect();
        match ready.is_empty() {
            true => Servers::Starting,
            false => Servers::Ready(ready),
        }
    }
}

/// Whether `buffer`'s language has a configured server: one it's routed
/// to or on, or, before it's opened on the manager, one its language would
/// route to.
fn configured(manager: &LspManager, view: &EditorView, buffer: fe_buffer::BufferId) -> bool {
    !manager.route_servers(buffer).is_empty()
        || crate::lsp::routed_language(view)
            .is_some_and(|language| manager.route_for(&language).is_some())
}
