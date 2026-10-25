//! Vim's file and window commands, typed on the status bar's line (`:w`,
//! `:q`, `:wq`, `:x`, `:e`, `:qa`, `:wa`), run as fastedit's: the bar
//! emits each [`HostCommand`] for the editor it was typed in, and
//! [`Workbench::run_vim_host_command`] runs it on that editor's tab, its
//! pane and this window. Every answer, such as `E37`, is the editor's vim
//! message, which the status bar shows.
//!
//! The day-one features' commands run as their own commands do: `:Ex`
//! shows the project tree, focused, with the editor's file revealed and
//! selected; `:oldfiles` opens the Open Recent picker, whose dismissal
//! focuses the editor again; `:copen` shows and focuses the Problems panel
//! and `:cclose` hides it if shown; and `:Replace {text}` runs the project
//! search's Replace All with `text`, through its question, saying "No
//! search to replace" when its query is empty.
//!
//! The buffer list's commands, `:b`, `:bn`, `:bp`, `:bd`, `:bf` and `:bl`,
//! run over the window's open files, numbered as nvim numbers buffers:
//! see [`vim_buffers`].
//!
//! The window commands, `:sp`, `:vs`, `:new`, `:vne`, `:on` and `:clo`,
//! run as the pane commands over the window's panes: see [`vim_windows`].
//!
//! `!` filters and `:r !{cmd}` run through the user's shell, off the UI
//! thread, only from the user's own keys in a trusted folder: see
//! [`vim_filter`].
//!
//! The other commands named with a capital, fugitive's and gitsigns', and
//! `:diffsplit`, run as [`super::git_commands`] says; so do the host
//! commands typed in an editor a git feature shows outside the tabs, which
//! a diff view's sides run with the helpers here.
//!
//! Closing a tab here never asks (no [`kiln_shell::close::confirm`], no
//! Discard/Cancel prompt): `:q` refuses a dirty tab with `E37`, and `:q!`
//! discards by decision. A window left with no tab closes, as does one
//! `:qa` closes; the app quits only when its last window has gone, as
//! fastedit does when any last window closes.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use fe_editor::SaveError;
use fe_gpui::EditorView;
use fe_vim::HostCommand;
use gpui::{App, Context, Entity, Task, WeakEntity, Window};
use kiln_shell::{ItemHandle, PaneView as _};

use super::git_commands::{self, GitCommand, GitOp};
use super::git_diff::E21;
use super::recent_picker::RecentOp;
use super::{Workbench, open};
use crate::Pane;
use crate::dock::{DockOp, DockPanel, ProblemsOp};

#[path = "vim_pick.rs"]
mod vim_pick;

#[path = "vim_buffers.rs"]
pub(crate) mod vim_buffers;

#[path = "vim_windows.rs"]
mod vim_windows;

#[path = "vim_tabs.rs"]
mod vim_tabs;

#[path = "vim_filter.rs"]
pub(super) mod vim_filter;

const E32: &str = "E32: No file name";
pub(crate) const E13: &str = "E13: File exists (add ! to override)";
pub(super) const E37: &str = "E37: No write since last change (add ! to override)";
/// What a filter or `:r !{cmd}` says in an editor shown outside the tabs
/// (a diff view's side), which doesn't run shell commands.
pub(super) const NO_SHELL: &str = "Not supported yet: shell commands";
const E99: &str = "E99: Current buffer is not in diff mode";
pub(super) const RELOAD: &str = "Not supported yet: reloading a file from disk";
const NO_SEARCH: &str = "No search to replace";

impl Workbench {
    /// Runs `command`, typed on the line in `editor`: see the module docs.
    /// An editor closed by now runs nothing. A map in `editor` waiting for
    /// the command goes on, or ends if it failed, once it's done: a command
    /// that runs past this call claims that report itself
    /// ([`EditorView::hold_vim_host`]), and any other is settled here.
    pub(super) fn run_vim_host_command(
        &mut self,
        editor: &WeakEntity<EditorView>,
        command: HostCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = editor.upgrade() else {
            return;
        };
        // Only these report an outcome, themselves (a write claims it) or as
        // they return: a message is a failure. Any other ends a map there: it
        // closes what the map's keys are for (a quit), or finishes on its
        // own time with no report back (`:e`, a git command, a stage or a
        // commit).
        // The commit box hands `:wa` to the active file editor, which claims
        // the report, not the box.
        let in_box = (self.owned_editors.iter()).any(|(owned, owner)| {
            *owned == editor.downgrade() && matches!(owner, git_commands::Owner::CommitBox)
        });
        let delegated = in_box && matches!(command, HostCommand::WriteAll);
        let reports = !delegated
            && matches!(
                command,
                HostCommand::Write { .. }
                    | HostCommand::SaveAs { .. }
                    | HostCommand::WriteAll
                    | HostCommand::Read { .. }
                    | HostCommand::DiffGet { .. }
                    | HostCommand::Explore
                    | HostCommand::OpenProblems
                    | HostCommand::CloseProblems
            );
        let what = match command {
            HostCommand::Quit { .. }
            | HostCommand::WriteQuit { .. }
            | HostCommand::QuitAll { .. } => "a quit",
            _ => "a command that finishes later",
        };
        self.run_host_command(editor.clone(), command, window, cx);
        editor.update(cx, |editor, cx| match reports {
            true => editor.settle_vim_host(cx),
            false => editor.end_vim_host(what, cx),
        });
    }

    fn run_host_command(
        &mut self,
        editor: Entity<EditorView>,
        command: HostCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_of(&editor, cx).is_none() {
            return git_commands::owned_host_command(self, editor, command, window, cx);
        }
        match command {
            HostCommand::Write { path: None, .. } => self.vim_write(editor, false, window, cx),
            HostCommand::Write {
                force,
                path: Some(path),
            } => self.vim_write_to(editor, &path, force, true, window, cx),
            HostCommand::SaveAs { force, path } => {
                self.vim_write_to(editor, &path, force, false, window, cx)
            }
            HostCommand::Quit { force } => self.vim_quit(&editor, force, window, cx),
            HostCommand::WriteQuit {
                only_if_modified, ..
            } => {
                if only_if_modified && !editor.is_dirty(cx) {
                    self.vim_quit(&editor, false, window, cx);
                } else {
                    self.vim_write(editor, true, window, cx);
                }
            }
            HostCommand::Edit {
                path: Some(path), ..
            } => self.vim_edit(&editor, &path, window, cx),
            HostCommand::Edit { path: None, .. } => message(&editor, RELOAD, cx),
            HostCommand::Read { path, below } => self.vim_read(&editor, path, below, cx),
            command @ (HostCommand::Filter { .. } | HostCommand::ReadCommand { .. }) => {
                let window = window.window_handle().window_id();
                self.vim_shell(&editor, command, window, cx)
            }
            HostCommand::QuitAll { force } => self.vim_quit_all(&editor, force, window, cx),
            HostCommand::WriteAll => self.vim_write_all(&editor, window, cx),
            HostCommand::User {
                name,
                bang,
                range,
                args,
            } => match git_commands::ex_command(&name, bang, range.is_some(), &args) {
                Ok((command, args)) => {
                    let op = GitOp {
                        command,
                        args,
                        range,
                        editor: Some(editor),
                        typed: true,
                    };
                    self.run_git_op(op, window, cx);
                }
                Err(answer) => message(&editor, answer, cx),
            },
            HostCommand::DiffGet { .. } | HostCommand::DiffPut { .. } => message(&editor, E99, cx),
            HostCommand::DiffSplit { path } => {
                let op = GitOp {
                    command: GitCommand::CompareFile,
                    args: path,
                    range: None,
                    editor: Some(editor),
                    typed: true,
                };
                self.run_git_op(op, window, cx);
            }
            command @ (HostCommand::Explore
            | HostCommand::OldFiles
            | HostCommand::OpenProblems
            | HostCommand::CloseProblems
            | HostCommand::ReplaceInFiles { .. }
            | HostCommand::Pick(_)
            | HostCommand::Buffer(_)
            | HostCommand::Window(_)
            | HostCommand::Tab(_)) => self.run_window_command(&editor, command, window, cx),
        }
    }

    /// Runs the day-one features' commands, `:Ex`, `:oldfiles`, `:copen`,
    /// `:cclose`, `:Replace`, the lists to pick from (`:marks`), the
    /// buffer list's (`:b`, `:bn`, `:bd`) and the panes' (`:sp`, `:on`,
    /// `:clo`) and tabs' (`:tabn`, `:tabc`), typed in `editor`, which act on the window
    /// whichever editor they are typed in: a tab's, a diff view's side or the
    /// commit box. Any other command runs nothing.
    pub(super) fn run_window_command(
        &mut self,
        editor: &Entity<EditorView>,
        command: HostCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            HostCommand::Explore => self.vim_explore(editor, window, cx),
            HostCommand::OldFiles => self.run_recent_op(RecentOp::Open, window, cx),
            HostCommand::OpenProblems => {
                self.run_dock_op(DockOp::Problems(ProblemsOp::Show), window, cx);
            }
            HostCommand::CloseProblems => {
                let dock = self.dock.read(cx);
                if dock.is_visible() && dock.active() == DockPanel::Problems {
                    self.run_dock_op(DockOp::Toggle, window, cx);
                }
            }
            HostCommand::ReplaceInFiles { text } => {
                self.vim_replace_in_files(editor, text, window, cx)
            }
            HostCommand::Pick(pick) => self.show_vim_pick(editor, pick, window, cx),
            HostCommand::Buffer(command) => {
                self.run_vim_buffer_command(editor, command, window, cx)
            }
            HostCommand::Window(command) => {
                self.run_vim_window_command(editor, command, window, cx)
            }
            HostCommand::Tab(command) => self.run_vim_tab_command(editor, command, window, cx),
            _ => {}
        }
    }

    /// `:Ex`: shows the project tree, focused, and reveals `editor`'s file
    /// there, selected.
    fn vim_explore(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_dock_op(DockOp::ShowProject, window, cx);
        if let Some(path) = editor.path(cx) {
            self.panel.update(cx, |panel, cx| panel.reveal(path, cx));
        }
    }

    /// Has the host commands vim's keys leave in `editor`, the active one,
    /// run as the line's do: `do` and `dp`, which fe-vim hands over as
    /// `:diffget` and `:diffput`. The line takes its own as it submits.
    pub(super) fn follow_vim_keys(
        &mut self,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.note_vim_buffer(cx);
        let id = editor.map(Entity::entity_id);
        if self.vim_keys.as_ref().map(|(followed, _)| *followed) == id {
            return;
        }
        self.vim_keys = editor.map(|editor| {
            let keys = cx.observe_in(editor, window, |workbench, editor, window, cx| {
                let command = editor.update(cx, |editor, _| editor.take_vim_host_command());
                if let Some(command) = command {
                    workbench.run_vim_host_command(&editor.downgrade(), command, window, cx);
                }
            });
            (editor.entity_id(), keys)
        });
    }

    /// `:Replace {text}`: the project search's Replace All with `text` for
    /// its replacement, which asks first; "No search to replace" with an
    /// empty query.
    fn vim_replace_in_files(
        &mut self,
        editor: &Entity<EditorView>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search = self.dock.read(cx).search().clone();
        if search.read(cx).query(cx).pattern.is_empty() {
            return message(editor, NO_SEARCH, cx);
        }
        search.update(cx, |search, cx| {
            search.confirm_replace_all_with(text, window, cx)
        });
    }

    /// The pane showing `editor` and its tab's index there.
    fn tab_of(&self, editor: &Entity<EditorView>, cx: &App) -> Option<(Entity<Pane>, usize)> {
        self.pane_group(cx).panes().into_iter().find_map(|pane| {
            let ix = (pane.read(cx).tabs().iter()).position(|tab| tab.editor() == Some(editor))?;
            Some((pane.clone(), ix))
        })
    }

    /// `:w`: saves `editor`'s tab, then with `quit` closes it as `:q` does
    /// if the save succeeded. A failure is the message.
    fn vim_write(
        &mut self,
        editor: Entity<EditorView>,
        quit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A map waiting for the write goes on, or ends, once the tab is
        // closed too, with `quit`, which ends it either way.
        let hold = editor.update(cx, |editor, cx| editor.hold_vim_host(cx));
        let saved = save(&editor, E32.to_owned(), window, cx);
        let editor = editor.downgrade();
        cx.spawn_in(window, async move |workbench, cx| {
            let failed = saved.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let Some(editor) = editor.upgrade() else {
                        return;
                    };
                    let ok = failed.is_none();
                    match failed {
                        Some(failed) => message(&editor, failed, cx),
                        // Still open, as nothing waited for it.
                        None if quit && workbench.tab_of(&editor, cx).is_some() => {
                            workbench.vim_quit(&editor, false, window, cx)
                        }
                        None => {}
                    }
                    // A quit ends the map, as it ends vim, closed or refused.
                    if let Some(hold) = hold {
                        editor.update(cx, |editor, cx| {
                            editor.finish_vim_host(hold, ok && !quit, cx)
                        });
                    }
                })
                .ok();
        })
        .detach();
    }

    /// `:w {path}` (`copy`) and `:sav {path}`: an untitled buffer, or any
    /// buffer for `:sav`, is saved as `path` and its tab takes the name; a
    /// named one with `:w` is written there as a copy, keeping its own path
    /// and dirty state. `path` as the buffer's own file is a plain `:w`.
    /// An existing file is `E13` unless `force`.
    fn vim_write_to(
        &mut self,
        editor: Entity<EditorView>,
        path: &str,
        force: bool,
        copy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = (self.root.clone()).or_else(|| editor.path(cx)?.parent().map(Path::to_owned));
        let resolved = match resolve(path, folder.as_deref()) {
            Ok(resolved) => resolved,
            Err(error) => return message(&editor, error, cx),
        };
        let own = editor.path(cx);
        let is_own = own.as_deref().is_some_and(|own| {
            own == resolved || identity(own).is_some() && identity(own) == identity(&resolved)
        });
        if is_own && copy {
            return self.vim_write(editor, false, window, cx);
        }
        if !is_own && !force && std::fs::symlink_metadata(&resolved).is_ok() {
            return message(&editor, E13, cx);
        }
        if copy && own.is_some() {
            // Held like a Save As destination until written, so nothing
            // is moved into it meanwhile.
            let buffer = editor.read(cx).model().buffer().id();
            let identity = open::file_identity(&resolved);
            let held = open::reserved(&resolved, identity.as_deref())
                .is_some_and(|(holder, _)| holder != buffer);
            if held {
                let name = resolved.file_name().unwrap_or_default().to_string_lossy();
                let error = format!("“{name}” is being saved by another tab");
                return message(&editor, error, cx);
            }
            let reservation = open::reserve(buffer, resolved.clone(), identity);
            #[cfg(test)]
            after_check(&resolved);
            let written = editor.update(cx, |editor, cx| editor.write_copy(resolved, force, cx));
            let hold = editor.update(cx, |editor, cx| editor.hold_vim_host(cx));
            let editor = editor.downgrade();
            cx.spawn(async move |_, cx| {
                let written = written.await;
                drop(reservation);
                let ok = written.is_ok();
                let failed = written.err().map(|error| {
                    let exists = (error.get_ref())
                        .is_some_and(|inner| inner.downcast_ref::<SaveError>().is_some());
                    match exists {
                        true => E13.to_owned(),
                        false => format!("{error:#}"),
                    }
                });
                editor
                    .update(cx, |editor, cx| {
                        if let Some(error) = failed {
                            editor.set_vim_message(error, cx);
                        }
                        if let Some(hold) = hold {
                            editor.finish_vim_host(hold, ok, cx);
                        }
                    })
                    .ok();
            })
            .detach();
        } else {
            #[cfg(test)]
            after_check(&resolved);
            self.save_editor_as_with(&editor, resolved, force, window, cx);
        }
    }

    /// `:q`: closes `editor`'s tab, unless it's dirty and no other tab
    /// shows its buffer, which gives `E37`; with `force` (`:q!`) it closes
    /// whatever its state. A window left with no tab closes.
    fn vim_quit(
        &mut self,
        editor: &Entity<EditorView>,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pane, ix)) = self.tab_of(editor, cx) else {
            return;
        };
        if !force && editor.is_dirty(cx) && !pane.read(cx).pane().shown_elsewhere(editor, cx) {
            return message(editor, E37, cx);
        }
        // Not through the close veto: it has been decided.
        pane.update(cx, |pane, cx| {
            pane.take_item(ix, window, cx);
            cx.notify();
        });
        let empty =
            (self.pane_group(cx).panes().iter()).all(|pane| pane.read(cx).tabs().is_empty());
        if empty {
            window.remove_window();
        }
    }

    /// `:qa`: closes the window, unless a tab of it, or a diff view's
    /// working side, is dirty: `E37` for `editor`'s, else `E162` naming the
    /// first in pane order. With
    /// `force` (`:qa!`) it closes discarding them.
    pub(super) fn vim_quit_all(
        &mut self,
        editor: &Entity<EditorView>,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !force {
            if editor.is_dirty(cx) {
                return message(editor, E37, cx);
            }
            let dirty = self
                .file_editors(cx)
                .into_iter()
                .find(|tab| tab.is_dirty(cx));
            if let Some(dirty) = dirty {
                let title = dirty.tab_title(cx);
                let error = format!("E162: No write since last change for buffer \"{title}\"");
                return message(editor, error, cx);
            }
        }
        window.remove_window();
    }

    /// `:wa`: saves every dirty tab of the window, and diff view's working
    /// side, each buffer once. The
    /// first failure in pane order is the message, once every other tab is
    /// saved.
    pub(super) fn vim_write_all(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Before the saves, which claim for their own editors.
        let hold = editor.update(cx, |editor, cx| editor.hold_vim_host(cx));
        let mut saving: Vec<Entity<EditorView>> = Vec::new();
        for tab in self.file_editors(cx) {
            let shared = (saving.iter()).any(|saved| saved.shares_document(&tab, cx));
            if tab.is_dirty(cx) && !shared {
                saving.push(tab);
            }
        }
        let saves: Vec<_> = (saving.iter())
            .map(|tab| {
                let title = tab.tab_title(cx);
                let untitled = format!("E141: No file name for buffer \"{title}\"");
                save(tab, untitled, window, cx)
            })
            .collect();
        let editor = editor.downgrade();
        cx.spawn(async move |_, cx| {
            let mut first = None;
            for saved in saves {
                let failed = saved.await;
                first = first.or(failed);
            }
            let ok = first.is_none();
            editor
                .update(cx, |editor, cx| {
                    if let Some(failed) = first {
                        editor.set_vim_message(failed, cx);
                    }
                    if let Some(hold) = hold {
                        editor.finish_vim_host(hold, ok, cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// `:[line]r[ead] [path]`: puts the lines of `path` (none: `editor`'s
    /// own file) below line `below`, from 1 (`0`: above the first), as one
    /// edit with the caret on the first line read, at its first non-blank.
    /// A file that can't be read leaves `E484` and the text as it was.
    pub(super) fn vim_read(
        &self,
        editor: &Entity<EditorView>,
        path: Option<String>,
        below: u32,
        cx: &mut App,
    ) {
        if editor.read(cx).is_read_only() {
            return message(editor, E21, cx);
        }
        let own = editor.path(cx).map(|path| path.to_path_buf());
        let named = match (&path, own) {
            (Some(path), _) => path.clone(),
            (None, Some(own)) => own.display().to_string(),
            (None, None) => return message(editor, E32, cx),
        };
        let folder = (self.root.clone()).or_else(|| editor.path(cx)?.parent().map(Path::to_owned));
        let read = resolve(&named, folder.as_deref())
            .ok()
            .and_then(|resolved| std::fs::read(resolved).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let Some(contents) = read else {
            return message(editor, format!("E484: Can't open file {named}"), cx);
        };
        let lines: Vec<&str> = contents
            .strip_suffix('\n')
            .unwrap_or(&contents)
            .split('\n')
            .collect();
        let lines = if contents.is_empty() {
            Vec::new()
        } else {
            lines
        };
        let noeol = if contents.is_empty() || contents.ends_with('\n') {
            ""
        } else {
            "[noeol] "
        };
        let report = format!("\"{named}\" {noeol}{}L, {}B", lines.len(), contents.len());
        editor.update(cx, |editor, cx| {
            if !lines.is_empty() {
                let model = editor.model_mut();
                model.sync_display();
                let text = model.text();
                let row = below.min(text.row_count());
                let (at, joined) = match row {
                    0 => (0, format!("{}\n", lines.join("\n"))),
                    row => {
                        let end = match row < text.row_count() {
                            true => text.row_start(row) - 1,
                            false => text.len(),
                        };
                        (end, format!("\n{}", lines.join("\n")))
                    }
                };
                model.set_cursor(at);
                model.paste(&joined);
                let text = model.text();
                let start = text.row_start(row);
                let blanks = lines[0].len() - lines[0].trim_start_matches([' ', '\t']).len();
                // A line of blanks only has the cursor on its last, as in
                // normal mode.
                model.set_cursor(start + blanks.min(lines[0].len().saturating_sub(1)));
            }
            editor.report_vim_message(report, cx);
            cx.notify();
        });
    }

    /// `:e {path}`: opens `path` in `editor`'s pane, which keeps `editor`'s
    /// tab open.
    fn vim_edit(
        &mut self,
        editor: &Entity<EditorView>,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pane, _)) = self.tab_of(editor, cx) else {
            return;
        };
        match self.vim_path(editor, path, cx) {
            Ok((resolved, _)) => pane.update(cx, |pane, cx| pane.open(resolved, window, cx)),
            Err(error) => message(editor, error, cx),
        }
    }

    /// `path` typed in `editor` resolved as [`resolve`] does, against the
    /// folder (else the editor's file's), and whether an editor is open on
    /// it: its tab keeps its spelling of the path, so it is reused, not
    /// opened a second time.
    fn vim_path(
        &self,
        editor: &Entity<EditorView>,
        path: &str,
        cx: &App,
    ) -> Result<(PathBuf, bool), String> {
        let folder = (self.root.clone()).or_else(|| editor.path(cx)?.parent().map(Path::to_owned));
        let resolved = resolve(path, folder.as_deref())?;
        let wanted = identity(&resolved);
        let open = (self.file_editors(cx).iter())
            .filter_map(|open| open.path(cx).map(|path| path.to_path_buf()))
            .find(|open| *open == resolved || (wanted.is_some() && identity(open) == wanted));
        Ok(match open {
            Some(open) => (open, true),
            None => (resolved, false),
        })
    }
}

// What a test does between the check for an existing file and the write.
#[cfg(test)]
type Hook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
    static AFTER_CHECK: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn after_check(path: &Path) {
    if let Some(hook) = AFTER_CHECK.with_borrow_mut(Option::take) {
        hook(path);
    }
}

#[cfg(test)]
#[path = "vim_write_path_tests.rs"]
mod write_path_tests;

#[cfg(test)]
#[path = "vim_write_no_clobber_tests.rs"]
mod write_no_clobber_tests;

#[cfg(test)]
#[path = "vim_remap_host_tests.rs"]
mod remap_host_tests;

#[cfg(test)]
#[path = "vim_day_one_tests.rs"]
mod day_one_tests;

#[cfg(test)]
#[path = "vim_ls_tests.rs"]
mod vim_ls_tests;

/// Leaves `text` as `editor`'s vim message.
pub(super) fn message(editor: &Entity<EditorView>, text: impl Into<String>, cx: &mut App) {
    editor.update(cx, |editor, cx| editor.set_vim_message(text, cx));
}

/// Saves `editor` through its [`kiln_shell::PaneItem::save`], resolving to
/// the error to show, if any: `untitled` for a buffer with no file.
pub(super) fn save(
    editor: &Entity<EditorView>,
    untitled: String,
    window: &mut Window,
    cx: &mut App,
) -> Task<Option<String>> {
    if editor.path(cx).is_none() {
        return Task::ready(Some(untitled));
    }
    match editor.save(window, cx) {
        Some(saved) => cx.spawn(async move |_| saved.await.err().map(|error| format!("{error:#}"))),
        None => Task::ready(None),
    }
}

/// `path` as typed after `:e`: `~` and `~/…` under the home folder, an
/// absolute path as is, a relative one under `folder`. `.` is dropped by
/// name, so an open file is found, and its symlinks are kept, as its
/// tab's path has them; but `..` follows the filesystem: it leaves the
/// folder a symlink leads to, not the one it is in. What comes before a
/// `..`, a `.` or a trailing slash must be a directory, or that is the
/// error: only the final file may be missing.
pub(super) fn resolve(path: &str, folder: Option<&Path>) -> Result<PathBuf, String> {
    let cant = |why: &dyn std::fmt::Display| format!("Can't open {path}: {why}");
    let joined = match path.strip_prefix('~') {
        Some("") => std::env::home_dir(),
        Some(rest) if rest.starts_with('/') => {
            std::env::home_dir().map(|home| home.join(&rest[1..]))
        }
        _ if Path::new(path).is_absolute() => Some(PathBuf::from(path)),
        _ => folder.map(|folder| folder.join(path)),
    };
    let Some(joined) = joined else {
        return Err(cant(&"no folder to resolve it against"));
    };
    let directory = |at: &Path| match std::fs::metadata(at) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(cant(&format!("{} is not a directory", at.display()))),
        Err(error) => Err(cant(&format!("{}: {error}", at.display()))),
    };
    let bytes = joined.as_os_str().as_bytes();
    let mut segments = bytes.split(|byte| *byte == b'/').peekable();
    let mut resolved = PathBuf::new();
    if bytes.first() == Some(&b'/') {
        resolved.push("/");
    }
    while let Some(segment) = segments.next() {
        match segment {
            b"" if segments.peek().is_some() => {}
            b"" => {
                if resolved.parent().is_some() {
                    directory(&resolved)?;
                    resolved.push("");
                }
            }
            b"." => directory(&resolved)?,
            b".." => {
                directory(&resolved)?;
                let real = std::fs::canonicalize(&resolved).map_err(|e| cant(&e))?;
                resolved = real;
                resolved.pop();
            }
            name => resolved.push(OsStr::from_bytes(name)),
        }
    }
    Ok(resolved)
}

/// Where `path` is, whatever its symlinks are spelled like: its real path,
/// or its real folder's with its own name while the file is missing.
pub(super) fn identity(path: &Path) -> Option<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(real) => Some(real),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(
            std::fs::canonicalize(path.parent()?)
                .ok()?
                .join(path.file_name()?),
        ),
        Err(_) => None,
    }
}
