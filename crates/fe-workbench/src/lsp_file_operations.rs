//! Tells language servers about what the project tree does to files, as
//! VS Code's explorer does: `workspace/didCreateFiles` after a new file or
//! folder or a duplicate, `didRenameFiles` after a rename and
//! `didDeleteFiles` after a move to the Trash. The manager sends each to
//! the servers whose filters match; without a manager nothing is sent.
//! Before a rename the tree asks the servers whose `willRename` filters
//! match for the edits it needs (`workspace/willRenameFiles`). Paths are
//! resolved as [`super::resolve`] does, so a path that is gone is resolved
//! by its folder; a rename's paths are resolved by their folders always, so
//! the old path keeps its spelling once a case-only rename has made it
//! resolve to the new one.
//!
//! The servers asked before a rename each answer with a workspace edit;
//! they're made as one ([`Workbench::apply_file_edits`]), so one file's
//! edits from several servers are joined, sorted, an edit two of them make
//! alike made once. A server's edits to a file that overlap those of a
//! server before it, or that are for another version of it, are left out,
//! and the others made.

use std::path::{Path, PathBuf};
use std::task::{Context, Poll, Waker};

use fe_lsp::{EditTarget, FileEdits};
use gpui::{App, AppContext as _, Task};

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
        if let Some(moved) = renamed(from, to, to.is_dir()) {
            lock(manager).did_rename_files(vec![moved]);
        }
    }

    /// The edits the servers whose `willRename` filters match want made
    /// before `from` is renamed to `to`, each file's joined ([`joined`]);
    /// none if no server's filters match (the task is ready at once then),
    /// or a server fails or times out. Dropping the task cancels the
    /// requests.
    pub(crate) fn will_rename(&self, from: &Path, to: &Path, cx: &App) -> Task<Vec<FileEdits>> {
        let (Some(manager), Some(moved)) = (&self.lsp.manager, renamed(from, to, from.is_dir()))
        else {
            return Task::ready(Vec::new());
        };
        let mut asked = Box::pin(lock(manager).will_rename_files(vec![moved]));
        match asked.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(edits) => Task::ready(joined(edits)),
            Poll::Pending => cx.background_spawn(async move { joined(asked.await) }),
        }
    }
}

fn resolved(paths: &[(PathBuf, bool)]) -> Vec<(PathBuf, bool)> {
    (paths.iter())
        .filter_map(|(path, folder)| Some((resolve(path)?, *folder)))
        .collect()
}

/// A rename of `from` to `to` (a folder if `folder`), each resolved by its
/// folder and named as given.
fn renamed(from: &Path, to: &Path, folder: bool) -> Option<(PathBuf, PathBuf, bool)> {
    Some((by_folder(from)?, by_folder(to)?, folder))
}

/// `path` with its folder resolved.
fn by_folder(path: &Path) -> Option<PathBuf> {
    match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) => Some(resolve(folder)?.join(name)),
        _ => resolve(path),
    }
}

/// `edits`, servers' answers in server order, with each file's text edits
/// in one entry, where the first server's for it was: a server's that
/// overlap those kept, or are for another version of the buffer or in
/// another encoding, are left out (and logged). Other entries (creating,
/// renaming or deleting a file) stay as they are.
fn joined(edits: Vec<FileEdits>) -> Vec<FileEdits> {
    let mut kept: Vec<(Option<PathBuf>, FileEdits)> = Vec::new();
    for file in edits {
        let text = |target: &EditTarget| {
            matches!(target, EditTarget::Open { .. } | EditTarget::Closed { .. })
        };
        if !text(&file.target) {
            kept.push((None, file));
            continue;
        }
        let key = resolve(&file.path).unwrap_or_else(|| file.path.clone());
        let same = (kept.iter_mut())
            .find(|(path, _)| path.as_ref() == Some(&key))
            .map(|(_, kept)| kept);
        match same {
            None => kept.push((Some(key), file)),
            Some(same) => {
                if !join(&mut same.target, file.target) {
                    log::warn!(
                        "{}: a language server's edits for the rename clash with another's, \
                         and aren't made",
                        file.path.display()
                    );
                }
            }
        }
    }
    kept.into_iter().map(|(_, file)| file).collect()
}

/// Adds `more`'s edits to `kept`'s, if they're for the same buffer version
/// or encoding and none overlaps; whether they were.
fn join(kept: &mut EditTarget, more: EditTarget) -> bool {
    match (kept, more) {
        (
            EditTarget::Open {
                buffer,
                version,
                edits,
            },
            EditTarget::Open {
                buffer: other,
                version: at,
                edits: more,
            },
        ) if *buffer == other && *version == at => {
            merged(edits, more, |(range, _)| (range.start, range.end))
        }
        (
            EditTarget::Closed { edits, encoding },
            EditTarget::Closed {
                edits: more,
                encoding: other,
            },
        ) if *encoding == other => merged(edits, more, |edit| (edit.range.start, edit.range.end)),
        _ => false,
    }
}

/// Adds `more` to `edits`, sorted by `range` with `edits`' first where they
/// start alike, those in both once, unless one overlaps another; whether
/// they were.
fn merged<T: PartialEq, K: Ord>(
    edits: &mut Vec<T>,
    more: Vec<T>,
    range: impl Fn(&T) -> (K, K),
) -> bool {
    let more: Vec<T> = (more.into_iter())
        .filter(|edit| !edits.contains(edit))
        .collect();
    let mut all: Vec<&T> = edits.iter().chain(&more).collect();
    all.sort_by_key(|edit| range(edit));
    if all.windows(2).any(|w| range(w[1]).0 < range(w[0]).1) {
        return false;
    }
    edits.extend(more);
    // Stable, so inserts at one place keep their order.
    edits.sort_by_key(|edit| range(edit));
    true
}
