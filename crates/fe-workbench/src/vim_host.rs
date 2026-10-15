//! Vim's file and window commands, typed on the status bar's line (`:w`,
//! `:q`, `:wq`, `:x`, `:e`, `:qa`, `:wa`), run as fastedit's: the bar
//! emits each [`HostCommand`] for the editor it was typed in, and
//! [`Workbench::run_vim_host_command`] runs it on that editor's tab, its
//! pane and this window. Every answer, such as `E37`, is the editor's vim
//! message, which the status bar shows.
//!
//! The other commands named with a capital, fugitive's and gitsigns', and
//! `:diffsplit`, run as [`super::git_commands`] says; so do the host
//! commands typed in an editor a git feature shows outside the tabs.
//!
//! Closing a tab here never asks (no [`kiln_shell::close::confirm`], no
//! Discard/Cancel prompt): `:q` refuses a dirty tab with `E37`, and `:q!`
//! discards by decision. A window left with no tab closes, as does one
//! `:qa` closes; the app quits only when its last window has gone, as
//! fastedit does when any last window closes.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use fe_gpui::EditorView;
use fe_vim::HostCommand;
use gpui::{App, Context, Entity, Task, WeakEntity, Window};
use kiln_shell::{ItemHandle, PaneView as _};

use super::Workbench;
use super::git_commands::{self, GitCommand, GitOp};
use crate::Pane;

const E32: &str = "E32: No file name";
const E37: &str = "E37: No write since last change (add ! to override)";
const E99: &str = "E99: Current buffer is not in diff mode";
const RELOAD: &str = "Not supported yet: reloading a file from disk";

impl Workbench {
    /// Runs `command`, typed on the line in `editor`: see the module docs.
    /// An editor closed by now runs nothing.
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
        if self.tab_of(&editor, cx).is_none() {
            return git_commands::owned_host_command(self, editor, command, window, cx);
        }
        match command {
            HostCommand::Write { .. } => self.vim_write(editor, false, window, cx),
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

    /// The pane showing `editor` and its tab's index there.
    fn tab_of(&self, editor: &Entity<EditorView>, cx: &App) -> Option<(Entity<Pane>, usize)> {
        self.panes.panes().into_iter().find_map(|pane| {
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
        let saved = save(&editor, E32.to_owned(), window, cx);
        let editor = editor.downgrade();
        cx.spawn_in(window, async move |workbench, cx| {
            let failed = saved.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let Some(editor) = editor.upgrade() else {
                        return;
                    };
                    match failed {
                        Some(failed) => message(&editor, failed, cx),
                        // Still open, as nothing waited for it.
                        None if quit && workbench.tab_of(&editor, cx).is_some() => {
                            workbench.vim_quit(&editor, false, window, cx)
                        }
                        None => {}
                    }
                })
                .ok();
        })
        .detach();
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
        let empty = (self.panes.panes().iter()).all(|pane| pane.read(cx).tabs().is_empty());
        if empty {
            window.remove_window();
        }
    }

    /// `:qa`: closes the window, unless a tab of it is dirty: `E37` for
    /// `editor`'s, else `E162` naming the first in pane order. With
    /// `force` (`:qa!`) it closes discarding them.
    fn vim_quit_all(
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
                .open_editors(cx)
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

    /// `:wa`: saves every dirty tab of the window, each buffer once. The
    /// first failure in pane order is the message, once every other tab is
    /// saved.
    fn vim_write_all(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut saving: Vec<Entity<EditorView>> = Vec::new();
        for tab in self.open_editors(cx) {
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
            if let Some(failed) = first {
                editor
                    .update(cx, |editor, cx| editor.set_vim_message(failed, cx))
                    .ok();
            }
        })
        .detach();
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
        let folder = (self.root.clone()).or_else(|| editor.path(cx)?.parent().map(Path::to_owned));
        let resolved = match resolve(path, folder.as_deref()) {
            Ok(resolved) => resolved,
            Err(error) => return message(editor, error, cx),
        };
        // A tab already on this file keeps its spelling of the path, so it
        // is reused, not opened a second time.
        let wanted = identity(&resolved);
        let resolved = (self.open_editors(cx).iter())
            .filter_map(|open| open.path(cx).map(|path| path.to_path_buf()))
            .find(|open| *open == resolved || (wanted.is_some() && identity(open) == wanted))
            .unwrap_or(resolved);
        pane.update(cx, |pane, cx| pane.open(resolved, window, cx));
    }
}

/// Leaves `text` as `editor`'s vim message.
fn message(editor: &Entity<EditorView>, text: impl Into<String>, cx: &mut App) {
    editor.update(cx, |editor, cx| editor.set_vim_message(text, cx));
}

/// Saves `editor` through its [`kiln_shell::PaneItem::save`], resolving to
/// the error to show, if any: `untitled` for a buffer with no file.
fn save(
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
fn resolve(path: &str, folder: Option<&Path>) -> Result<PathBuf, String> {
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
fn identity(path: &Path) -> Option<PathBuf> {
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
