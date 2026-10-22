//! Tells language servers what the project watcher saw on disk, as VS Code
//! does: `workspace/didChangeWatchedFiles`, sent by the manager to the
//! servers whose registered globs match ([`fe_lsp::LspManager::files_changed`]).
//! Without a manager nothing is sent.
//!
//! The watcher's changes carry no kind, so each path is stat-ed off the UI
//! thread: missing is `Deleted`; present and created at or after the
//! previous batch's arrival (the manager's start for the first) is
//! `Created`; present otherwise, or where the platform gives no creation
//! time, `Changed`. A file replaced by an atomic save, a new inode, reads as
//! `Created`; servers generally re-read a file on either.
//!
//! Paths in VS Code's `files.watcherExclude` defaults (`**/.git/objects/**`,
//! `**/.git/subtree-cache/**`, `**/node_modules/*/**`, `**/.hg/store/**`)
//! are skipped; a batch of only those sends nothing. Paths are resolved as
//! [`super::resolve`] does.
//!
//! A [`FsChange::Rescan`] sends nothing: the watcher dropped events and
//! doesn't know which. A known limit: a server that missed a change sees it
//! at its next watched event or restart. VS Code has the same gap when its
//! watcher overflows.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use fe_lsp::FileChangeType;
use fe_project::FsChange;
use gpui::{AppContext as _, Context};

use super::{lock, resolve};
use crate::Workbench;

impl Workbench {
    /// Sends the watcher's `changes` to the servers that registered for
    /// them.
    pub(crate) fn lsp_fs_changed(&mut self, changes: &[FsChange], cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        let (Some(manager), Some(root)) = (lsp.manager.clone(), lsp.root.clone()) else {
            return;
        };
        let now = SystemTime::now();
        let since = lsp.watched_at.replace(now).unwrap_or(now);
        let mut paths = Vec::new();
        for change in changes {
            match change {
                FsChange::Path(path) => paths.push(path.clone()),
                FsChange::Rescan => log::debug!("watcher rescan: no file changes sent to servers"),
            }
        }
        if paths.is_empty() {
            return;
        }
        let generation = lsp.generation;
        let kinds = cx.background_spawn(async move { kinds(&root, paths, since) });
        cx.spawn(async move |workbench, cx| {
            let changes = kinds.await;
            if changes.is_empty() {
                return;
            }
            workbench
                .update(cx, |workbench, _| {
                    if workbench.lsp.generation == generation {
                        lock(&manager).files_changed(changes);
                    }
                })
                .ok();
        })
        .detach();
    }
}

/// Each of `paths` not excluded, resolved, with what happened to it.
fn kinds(root: &Path, paths: Vec<PathBuf>, since: SystemTime) -> Vec<(PathBuf, FileChangeType)> {
    paths
        .into_iter()
        .filter_map(|path| {
            let path = resolve(&path)?;
            if excluded(path.strip_prefix(root).unwrap_or(&path)) {
                return None;
            }
            let kind = match fs::metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileChangeType::DELETED,
                Err(_) => FileChangeType::CHANGED,
                Ok(meta) => match meta.created() {
                    Ok(created) if created >= since => FileChangeType::CREATED,
                    _ => FileChangeType::CHANGED,
                },
            };
            Some((path, kind))
        })
        .collect()
}

/// Whether `path` (from the root) is one VS Code's watcher skips.
fn excluded(path: &Path) -> bool {
    let names: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    names.iter().enumerate().any(|(i, name)| {
        let below = names.len() - i - 1;
        match *name {
            ".git" => below >= 1 && matches!(names[i + 1], "objects" | "subtree-cache"),
            ".hg" => below >= 1 && names[i + 1] == "store",
            "node_modules" => below >= 1,
            _ => false,
        }
    })
}
