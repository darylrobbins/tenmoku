//! Tells language servers about what the project tree does to files, as
//! VS Code's explorer does: `workspace/didCreateFiles` after a new file or
//! folder or a duplicate, `didRenameFiles` after a rename and
//! `didDeleteFiles` after a move to the Trash. The manager sends each to
//! the servers whose filters match; without a manager nothing is sent.
//! Paths are resolved as [`super::resolve`] does, so a path that is gone is
//! resolved by its folder.

use std::path::{Path, PathBuf};

use super::{lock, resolve};
use crate::Workbench;

impl Workbench {
    /// `paths`, each with whether it's a folder, were created.
    pub(crate) fn lsp_files_created(&self, paths: &[(PathBuf, bool)]) {
        if let (Some(manager), paths) = (&self.lsp.manager, resolved(paths)) {
            lock(manager).did_create_files(paths);
        }
    }

    /// `paths`, each with whether it was a folder, were trashed.
    pub(crate) fn lsp_files_deleted(&self, paths: &[(PathBuf, bool)]) {
        if let (Some(manager), paths) = (&self.lsp.manager, resolved(paths)) {
            lock(manager).did_delete_files(paths);
        }
    }

    /// `from` was renamed to `to`, which is there now.
    pub(crate) fn lsp_files_renamed(&self, from: &Path, to: &Path) {
        let Some(manager) = &self.lsp.manager else {
            return;
        };
        if let (Some(from), Some(resolved)) = (resolve(from), resolve(to)) {
            lock(manager).did_rename_files(vec![(from, resolved, to.is_dir())]);
        }
    }
}

fn resolved(paths: &[(PathBuf, bool)]) -> Vec<(PathBuf, bool)> {
    (paths.iter())
        .filter_map(|(path, folder)| Some((resolve(path)?, *folder)))
        .collect()
}
