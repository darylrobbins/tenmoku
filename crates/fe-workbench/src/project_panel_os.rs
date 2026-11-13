//! The project tree's commands that leave it, on the selected row, as VS
//! Code's Explorer and IntelliJ's Project view offer them: Reveal in
//! Finder (the File Manager elsewhere), Open in Terminal and Find in
//! Folder.
//!
//! Reveal goes through the active-file command's seam
//! ([`RevealSeam`], else `App::reveal_path`). The other two are the
//! workbench's to do: the panel asks with a [`PanelEvent`], and
//! [`Workbench`] starts a terminal in the folder, or limits Find in Files
//! to it. A file's folder is its parent; the root row is the root folder.
//! With nothing selected they do nothing.

use std::path::{Path, PathBuf};

use fe_project::{EntryKind, fs_ops};
use gpui::{Context, Window};
use kiln::core::Os;
use kiln_l10n::tr;

use super::{PanelEvent, PanelOp, ProjectPanel};
use crate::{Workbench, dock::DockOp, path_commands::RevealSeam};

/// The title Reveal's menu item shows: Finder's on macOS, else the
/// File Manager's.
pub(super) fn reveal_title() -> gpui::SharedString {
    match Os::current() {
        Os::MacOs => tr!("workbench-project-reveal-in-finder"),
        _ => tr!("workbench-project-reveal-in-file-manager"),
    }
}

impl ProjectPanel {
    /// The selected row's path, and whether it's a folder: the root row is
    /// the root folder.
    fn selected_target(&self) -> Option<(PathBuf, bool)> {
        let id = self.selected_index().and(self.selected)?;
        let tree = self.tree()?;
        if id == tree.root() {
            return Some((self.root.clone()?, true));
        }
        let folder = tree.row(id)?.kind == EntryKind::Dir;
        Some((self.shown_path(id)?, folder))
    }

    /// The selected folder, or a selected file's parent.
    fn selected_folder(&self) -> Option<PathBuf> {
        let (path, folder) = self.selected_target()?;
        match folder {
            true => Some(path),
            false => path.parent().map(Path::to_owned),
        }
    }

    /// Does `op`, one of the three: see the module docs.
    pub(super) fn run_os_op(&mut self, op: PanelOp, cx: &mut Context<Self>) {
        match op {
            PanelOp::RevealInOs => {
                let Some((path, _)) = self.selected_target() else {
                    return;
                };
                match cx.try_global::<RevealSeam>().copied() {
                    Some(RevealSeam(reveal)) => reveal(&path, cx),
                    None => cx.reveal_path(&path),
                }
            }
            PanelOp::OpenInTerminal => {
                if let Some(dir) = self.selected_folder() {
                    cx.emit(PanelEvent::OpenTerminalIn(dir));
                }
            }
            PanelOp::FindInFolder => {
                if let Some(dir) = self.selected_folder() {
                    cx.emit(PanelEvent::FindInFolder(dir));
                }
            }
            _ => {}
        }
    }
}

/// The include glob for `rel`, a folder under the root written with `/`s:
/// everything under it, with the glob characters in its name escaped.
fn folder_glob(rel: &str) -> String {
    let mut glob = String::new();
    for ch in rel.chars() {
        if matches!(ch, '\\' | '*' | '?' | '[' | ']' | '{' | '}') || (glob.is_empty() && ch == '!')
        {
            glob.push('\\');
        }
        glob.push(ch);
    }
    glob.push_str("/**");
    glob
}

impl Workbench {
    /// Shows the terminal dock with a new session in `dir`.
    pub(crate) fn open_terminal_in(
        &mut self,
        dir: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        (self.terminal().clone()).update(cx, |terminal, cx| terminal.add_in(Some(dir), window, cx));
        self.show_terminal_dock(window, cx);
    }

    /// Shows Find in Files limited to `dir`; the whole project if it's the
    /// root, or outside it.
    pub(crate) fn find_in_folder(
        &mut self,
        dir: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let glob = match self.root() {
            Some(root) if dir != root && dir.starts_with(root) => {
                folder_glob(&fs_ops::relative_path(root, dir))
            }
            _ => String::new(),
        };
        self.run_dock_op(DockOp::ShowSearch, window, cx);
        let search = self.dock().read(cx).search().clone();
        search.update(cx, |search, cx| search.scope_to(glob, window, cx));
    }
}
