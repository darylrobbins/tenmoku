//! The active file's path commands, VS Code's `copyPathOfActiveFile`,
//! `copyRelativePathOfActiveFile` and `revealActiveFileInWindows`: the
//! project tree has its own Copy Path and Reveal, on its selected row;
//! these act on the active editor's tab.
//!
//! `Workbench`'s one `#[kiln::command]` block is `workbench_commands.rs`'s,
//! so the commands are declared on [`PathCommands`], which no element
//! attaches, and [`init`]'s listeners run them on the window's workbench, as
//! `find_commands::init`'s does `find.show_replace`.
//!
//! Revealing goes through a seam, [`RevealSeam`]: GPUI's `App::reveal_path`
//! (Finder, or the file manager) does nothing under the test platform, so a
//! test sets what records the path instead.

use std::path::{Path, PathBuf};

use fe_project::fs_ops;
use gpui::{App, ClipboardItem, Context, Global};
use kiln::{Cx, platform::command_actions::dispatch_window};

use crate::Workbench;

/// The commands' declaration: see the module's docs. They're handled by
/// [`init`]'s listeners, not here.
#[allow(dead_code)]
struct PathCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl PathCommands {
    /// Copy Path of Active File
    #[command(id = "workbench.copy_active_file_path")]
    fn copy_active_file_path(&mut self, _: &mut Cx<Self>) {}

    /// Copy Relative Path of Active File
    #[command(id = "workbench.copy_active_file_relative_path")]
    fn copy_active_file_relative_path(&mut self, _: &mut Cx<Self>) {}

    /// Reveal Active File in File Manager
    #[command(id = "workbench.reveal_active_file_in_os")]
    fn reveal_active_file_in_os(&mut self, _: &mut Cx<Self>) {}
}

/// What reveals a path in the OS's file manager, if not GPUI's: set by
/// tests, which record the path.
#[derive(Clone, Copy)]
pub(crate) struct RevealSeam(pub(crate) fn(&Path, &mut App));

impl Global for RevealSeam {}

impl Workbench {
    /// The active editor's file's path, as the editor has it, if it has one.
    fn active_file(&self, cx: &App) -> Option<PathBuf> {
        let editor = self.active_editor(cx)?;
        editor.read(cx).model().path().map(Path::to_owned)
    }

    /// Puts the active file's path on the clipboard: absolute, or from the
    /// window's folder with `/`s (absolute if it's outside it). Leaves the
    /// clipboard alone if no file is active.
    pub(crate) fn copy_active_file_path(&mut self, relative: bool, cx: &mut Context<Self>) {
        let Some(path) = self.active_file(cx) else {
            return;
        };
        let text = match (relative, self.root()) {
            (true, Some(root)) => fs_ops::relative_path(root, &path),
            _ => path.to_string_lossy().into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Reveals the active file in the OS's file manager, if one is active.
    pub(crate) fn reveal_active_file_in_os(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_file(cx) else {
            return;
        };
        match cx.try_global::<RevealSeam>().copied() {
            Some(RevealSeam(reveal)) => reveal(&path, cx),
            None => cx.reveal_path(&path),
        }
    }
}

/// Runs `run` on the workbench of the window the command was dispatched in,
/// after the dispatch, which has the window out.
fn on_workbench(cx: &mut App, run: impl FnOnce(&mut Workbench, &mut Context<Workbench>) + 'static) {
    let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
        return;
    };
    cx.defer(move |cx| {
        handle
            .update(cx, |_, window, cx| {
                if let Some(Some(workbench)) = window.root::<Workbench>() {
                    workbench.update(cx, run);
                }
            })
            .ok();
    });
}

/// Handles the commands.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &CopyActiveFilePath, cx| {
        on_workbench(cx, |workbench, cx| {
            workbench.copy_active_file_path(false, cx)
        });
    });
    cx.on_action(|_: &CopyActiveFileRelativePath, cx| {
        on_workbench(cx, |workbench, cx| {
            workbench.copy_active_file_path(true, cx)
        });
    });
    cx.on_action(|_: &RevealActiveFileInOs, cx| {
        on_workbench(cx, |workbench, cx| workbench.reveal_active_file_in_os(cx));
    });
}
