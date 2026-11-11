//! The project tree's nvim-tree keys beyond moving the selection:
//! expanding and collapsing every folder, listing ignored entries or not,
//! and listing the tree again from disk. (`o`, nvim-tree's toggle, is
//! Enter's [`ProjectPanel::open_selected`].)
//!
//! Folders are expanded as a click expands one: queued in `expanding` and
//! listed off the main thread. An [`Expansion`] queues the folders below
//! each as its listing lands, until none it queued waits.

use kiln_l10n::text;
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
};

use fe_project::{EntryKind, FileTree, FsChange, NodeId};
use gpui::Context;

use super::{ProjectPanel, State};

/// The most folders `project_panel.expand_all` expands, as nvim-tree's
/// `max_folder_discovery` caps its: a tree as big as `node_modules` is
/// expanded no further, and the panel says so.
pub(crate) const EXPAND_ALL_LIMIT: usize = 300;

/// Folders being expanded as their parents' listings land.
pub(super) struct Expansion {
    /// The folders to expand, by path from the root; every folder if
    /// `None`.
    only: Option<HashSet<PathBuf>>,
    /// The folders it has expanded or queued.
    taken: HashSet<NodeId>,
    /// The most it takes.
    limit: usize,
    /// Whether it passed folders over at the limit.
    capped: bool,
}

impl Expansion {
    fn new(only: Option<HashSet<PathBuf>>, limit: usize) -> Self {
        Self {
            only,
            taken: HashSet::new(),
            limit,
            capped: false,
        }
    }
}

impl ProjectPanel {
    /// `E`: expands every folder under the root, breadth-first, up to
    /// [`EXPAND_ALL_LIMIT`] of them. The selection stays.
    pub(super) fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.status = None;
        self.expansion = Some(Expansion::new(None, EXPAND_ALL_LIMIT));
        // Retries failed listings, as a click does.
        self.epoch += 1;
        self.advance_expansion(cx);
    }

    /// Takes the expansion as far as what's listed allows: queues each
    /// folder it wants under a listed one it took (the root first), then
    /// lists those queued. Once no listing of the root or of a folder it
    /// took runs or waits (one may find more folders), it ends, saying so
    /// in the panel if it stopped at its limit.
    pub(super) fn advance_expansion(&mut self, cx: &mut Context<Self>) {
        let Some(expansion) = self.expansion.as_mut() else {
            return;
        };
        let State::Open(tree) = &self.state else {
            return;
        };
        let mut queue = VecDeque::from([tree.root()]);
        while let Some(dir) = queue.pop_front() {
            // Unlisted: it goes on once the listing lands.
            let Some(children) = tree.children(dir) else {
                continue;
            };
            for &child in children {
                let Some(row) = tree.row(child).filter(|row| row.kind == EntryKind::Dir) else {
                    continue;
                };
                let shown = row.expanded || self.expanding.contains(&child);
                if expansion.taken.contains(&child) {
                    // Unless collapsed since, or its listing failed.
                    if shown {
                        queue.push_back(child);
                    }
                    continue;
                }
                let wanted = expansion.only.as_ref().is_none_or(|only| {
                    rel_path(tree, child).is_some_and(|rel| only.contains(&rel))
                });
                if !wanted {
                    continue;
                }
                if expansion.taken.len() >= expansion.limit {
                    expansion.capped = true;
                    continue;
                }
                expansion.taken.insert(child);
                if !shown {
                    self.expanding.insert(child);
                }
                queue.push_back(child);
            }
        }
        // Those listed already expand here; the rest are listed.
        self.reconcile(cx);
        let (Some(expansion), State::Open(tree)) = (self.expansion.as_ref(), &self.state) else {
            return;
        };
        let root = tree.root();
        let pending = (expansion.taken.iter()).any(|dir| self.expanding.contains(dir))
            || (self.listing.keys()).any(|dir| *dir == root || expansion.taken.contains(dir));
        if pending {
            return;
        }
        if expansion.capped {
            let count = EXPAND_ALL_LIMIT;
            self.status = Some(text!("workbench-project-expand-all-capped", count = count));
        }
        self.expansion = None;
        cx.notify();
    }

    /// `W`: collapses every folder, leaving the root's children. A
    /// selection inside one moves to the root's child it was under.
    pub(super) fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expansion = None;
        self.status = None;
        let selected = self.selected_index();
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let survivor = selected.and_then(|ix| {
            (tree.rows(0..ix + 1))
                .filter(|row| row.depth == 0)
                .last()
                .map(|row| row.id)
        });
        let root = tree.root();
        for dir in self.expanded.drain() {
            if dir != root {
                tree.collapse(dir);
            }
        }
        self.expanding.clear();
        if let Some(id) = survivor {
            self.selected = Some(id);
            if let Some(ix) = self.selected_index() {
                self.select_row(ix, cx);
            }
        }
        cx.notify();
    }

    /// `H`: lists ignored entries too, or stops, opening the tree again
    /// with the folders that were expanded expanded and the selected row
    /// selected, if they're still listed. Pressed again before that's done
    /// (the tree still opening, or restoring), what's still to restore
    /// carries over to the next tree.
    pub(super) fn toggle_ignored(&mut self, cx: &mut Context<Self>) {
        let restoring = (self.expansion.take()).and_then(|expansion| {
            let only = expansion.only?;
            Some((only, expansion.taken))
        });
        let mut selected = self.reveal.take();
        let mut expanded = HashSet::new();
        match self.tree() {
            Some(tree) => {
                // Those waiting for a listing to expand too.
                let shown = self.expanded.iter().chain(&self.expanding);
                expanded.extend(shown.filter_map(|&dir| rel_path(tree, dir)));
                if let Some((only, taken)) = restoring {
                    // Those it hasn't reached yet: the ones it took are
                    // shown now, unless collapsed since.
                    let taken: HashSet<PathBuf> = (taken.iter())
                        .filter_map(|&dir| rel_path(tree, dir))
                        .collect();
                    expanded.extend(only.into_iter().filter(|rel| !taken.contains(rel)));
                }
                let shown =
                    (self.selected_index().and(self.selected)).and_then(|id| self.shown_path(id));
                selected = shown.or(selected);
            }
            None => expanded.extend(restoring.into_iter().flat_map(|(only, _)| only)),
        }
        self.set_show_ignored(!self.show_ignored, cx);
        if !expanded.is_empty() {
            self.expansion = Some(Expansion::new(Some(expanded), usize::MAX));
        }
        // Both go on once the new tree opens.
        self.reveal = selected;
    }

    /// `g r`: lists the root and every loaded folder again from disk, as
    /// the watcher's rescan does, so what changed with no event (or before
    /// the watcher saw it) shows; the expanded folders and the selection
    /// stay. Others that follow the disk get the rescan too. A root that
    /// couldn't be opened is opened again.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        match self.state {
            State::Open(_) => self.apply_fs_changes(&[FsChange::Rescan], cx),
            State::Failed(_) => self.reopen(cx),
            State::NoFolder | State::Opening => {}
        }
    }

    /// The panel's last word on an operation, if it has one.
    #[cfg(test)]
    pub(crate) fn status_text(&self) -> Option<String> {
        self.status
            .as_ref()
            .map(|status| status.resolve().to_string())
    }
}

/// Node `id`'s path from the root. `None` for the root itself.
fn rel_path(tree: &FileTree, id: NodeId) -> Option<PathBuf> {
    let path = tree.path(id)?;
    let rel = path.strip_prefix(tree.root_path()).ok()?;
    (!rel.as_os_str().is_empty()).then(|| rel.to_owned())
}
