//! The lazy file tree: an arena of nodes, loaded a directory at a time, with a
//! cached flattened view for a virtual list.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet, VecDeque},
    ffi::{OsStr, OsString},
    fs::{self, File},
    io,
    ops::Range,
    path::{Component, Path, PathBuf},
    sync::{Arc, atomic::AtomicU64},
};

use ignore::gitignore::Gitignore;

use crate::{
    FsChange,
    listing::{self, Access, Entry, EntryKind, Identity, Listed, Listing, Lost, Point, Rules},
};

/// A node of a [`FileTree`]. Ids stay valid until the node is removed, and are
/// never reused for another node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeOptions {
    /// List entries matched by ignore files too, with [`Row::ignored`] set.
    pub show_ignored: bool,
}

/// Counters for tests and benches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeStats {
    /// Directory listings stored or discarded, by [`FileTree::fill`] or
    /// [`FileTree::apply`].
    pub listings: u64,
    /// Entries looked up by [`FileTree::apply`] in their opened parent
    /// directory without following symlinks (`symlink_metadata`, done as
    /// `getattrlistat` on macOS to learn the on-disk name too, and `fstatat`
    /// elsewhere): those it reconciles, and each directory it opens, whose
    /// spelling on disk is checked.
    pub stats: u64,
    /// Nodes in the tree, not counting the root.
    pub loaded_entries: usize,
}

/// One row of the flattened view.
#[derive(Clone, Copy, Debug)]
pub struct Row<'a> {
    pub id: NodeId,
    /// 0 for the root's children.
    pub depth: usize,
    pub name: &'a OsStr,
    pub kind: EntryKind,
    pub expanded: bool,
    pub ignored: bool,
    /// Why the directory couldn't be listed, from its last attempt.
    pub error: Option<&'a io::Error>,
}

#[derive(Debug)]
struct Node {
    name: OsString,
    kind: EntryKind,
    /// A directory's identity, which tells it from another one later put at
    /// its path. `None` until known.
    id: Option<Identity>,
    ignored: bool,
    parent: Option<NodeId>,
    /// Sorted children of a listed directory; `None` until it is listed.
    children: Option<Vec<NodeId>>,
    expanded: bool,
    error: Option<io::Error>,
    /// When the run of the listing the children came from started.
    listed_seq: u64,
    /// When `apply` last dropped a change to this directory because it wasn't
    /// listed (or saw the directory's own path while it wasn't). A listing
    /// started before then may have missed something.
    dropped_seq: u64,
    /// When its children, or anything loaded below them, were last brought
    /// up to date: by `apply` looking an entry up, or by a listing of one of
    /// them, stored, that ended then. A listing of this directory started
    /// before then is older than they are.
    applied_seq: u64,
}

impl Node {
    fn new(
        name: OsString,
        kind: EntryKind,
        id: Option<Identity>,
        ignored: bool,
        parent: Option<NodeId>,
    ) -> Node {
        Node {
            name,
            kind,
            id,
            ignored,
            parent,
            children: None,
            expanded: false,
            error: None,
            listed_seq: 0,
            dropped_seq: 0,
            applied_seq: 0,
        }
    }
}

#[derive(Debug)]
struct Slot {
    generation: u32,
    node: Option<Node>,
}

/// The state of one [`FileTree::apply`] call.
#[derive(Default)]
struct Pass {
    changed: Vec<NodeId>,
    relist: Vec<NodeId>,
    /// Directories listed again at the end with every loaded directory
    /// below them.
    rescan: Vec<NodeId>,
    /// Whether the root's own path is in the batch.
    root_reported: bool,
    /// Directories opened to look up their entries, `None` where that
    /// failed. Only those on the way to the last one used stay open.
    dirs: HashMap<NodeId, Option<File>>,
    /// The directories above the root, from `/`, opened on the way to it.
    above: Vec<File>,
    /// The ignore rules in force in opened directories, read through their
    /// handles. Read afresh by each `apply`, so nothing has to be invalidated.
    rules: HashMap<NodeId, Arc<Rules>>,
}

/// A project's file tree, read lazily: a directory is listed only when it is
/// expanded (or [`FileTree::fill`]ed), and entries matched by `.gitignore`,
/// `.ignore` and git's exclude files are hidden unless
/// [`TreeOptions::show_ignored`] is set. `.git` is never listed.
///
/// The tree doesn't watch the disk itself. Feed it batches from a
/// [`crate::ProjectWatcher`] (or anything else) through [`FileTree::apply`].
pub struct FileTree {
    /// Canonical, as file watchers report paths.
    root: PathBuf,
    /// The root as given to [`FileTree::open`], also accepted by `apply`.
    given_root: PathBuf,
    options: TreeOptions,
    slots: Vec<Slot>,
    free: Vec<u32>,
    root_id: NodeId,
    /// The flattened view: each visible node with its depth.
    rows: Vec<(NodeId, usize)>,
    rows_dirty: bool,
    version: u64,
    stats: TreeStats,
    /// git's global excludes (`core.excludesFile`), read once.
    global: Arc<Gitignore>,
    /// Whether the root's volume matches names regardless of case (APFS's
    /// default), so that a case-only rename can leave an old spelling in the
    /// tree. Checked once, when the tree is opened.
    pub(crate) ignores_case: bool,
    /// Ticks at each observation `apply` makes and at the start and end of
    /// each listing's run, shared with [`Listing`]s so they can run on other
    /// threads. Observations are timed after they are made, so a listing that
    /// started earlier counts as older, whatever it read when.
    clock: Arc<AtomicU64>,
    /// The `apply` with the last [`FsChange::Rescan`]: a listing started
    /// before it may miss anything.
    rescan_seq: u64,
    /// Set once a listing couldn't reach the root as the tree has it: past a
    /// directory above it, for which no events come, or because the root was
    /// replaced. Changes made meanwhile may have gone unreported, so once the
    /// root is reached again everything loaded is listed again.
    root_lost: bool,
    /// When the last result that couldn't reach the root so ended.
    cut_seq: u64,
    /// When the last listing of the root stored started: a result that
    /// couldn't reach it and ended before then is old news.
    found_seq: u64,
    /// File system calls made (opens, stats, ignore lookups, listings), for
    /// tests that check work is done without IO.
    pub(crate) io_calls: u64,
    /// The most directories `apply` has held open at once (not counting
    /// those above the root), for tests.
    #[cfg(test)]
    pub(crate) open_peak: usize,
}

impl FileTree {
    /// Opens the directory `root` and lists its entries, nothing deeper.
    pub fn open(root: impl AsRef<Path>, options: TreeOptions) -> io::Result<FileTree> {
        let given_root = root.as_ref().to_path_buf();
        let root = dunce::canonicalize(&given_root)?;
        if !fs::metadata(&root)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", root.display()),
            ));
        }
        let name = root
            .file_name()
            .map_or_else(|| root.as_os_str().to_os_string(), OsStr::to_os_string);
        let ignores_case = listing::open_dirs(&root, Access::Search)
            .map_err(|unreached| unreached.err)?
            .last()
            .is_some_and(listing::ignores_case);
        let mut tree = FileTree {
            // As the `ignore` crate's walker reads them: relative to the
            // current directory. Unit tests use none, so they don't depend
            // on this machine's git configuration.
            global: Arc::new(if cfg!(test) {
                Gitignore::empty()
            } else {
                Gitignore::global().0
            }),
            ignores_case,
            root,
            given_root,
            options,
            slots: Vec::new(),
            free: Vec::new(),
            root_id: NodeId {
                index: 0,
                generation: 0,
            },
            rows: Vec::new(),
            rows_dirty: false,
            version: 0,
            stats: TreeStats::default(),
            clock: Arc::new(AtomicU64::new(0)),
            rescan_seq: 0,
            root_lost: false,
            cut_seq: 0,
            found_seq: 0,
            io_calls: 0,
            #[cfg(test)]
            open_peak: 0,
        };
        tree.root_id = tree.alloc(Node {
            expanded: true,
            ..Node::new(name, EntryKind::Dir, None, false, None)
        });
        tree.stats.loaded_entries = 0;
        let listing = tree.listing(tree.root_id).expect("the root is unlisted");
        let mut listed = listing.run();
        listed.result = Ok(listed.result?);
        tree.fill(tree.root_id, listed);
        Ok(tree)
    }

    /// The canonical root directory.
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// The root directory's node. It has no row: its children are depth 0.
    pub fn root(&self) -> NodeId {
        self.root_id
    }

    pub fn options(&self) -> TreeOptions {
        self.options
    }

    /// Increases whenever the rows or any loaded node change.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn stats(&self) -> TreeStats {
        self.stats
    }

    /// The node at `path`, absolute under the root or relative to it, if it
    /// is loaded.
    pub fn find(&self, path: impl AsRef<Path>) -> Option<NodeId> {
        let rel = self.relative(path.as_ref())?;
        let mut id = self.root_id;
        for component in rel.components() {
            let Component::Normal(name) = component else {
                return None;
            };
            id = self.child_named(id, name)?.1;
        }
        Some(id)
    }

    /// The absolute path of a node.
    pub fn path(&self, id: NodeId) -> Option<PathBuf> {
        let mut path = self.root.clone();
        path.push(self.rel_path(id)?);
        Some(path)
    }

    /// A directory's children in display order, once it is listed.
    pub fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.get(id)?.children.as_deref()
    }

    /// A node as it would show as a row (whether or not it is visible). `None`
    /// for the root and removed nodes.
    pub fn row(&self, id: NodeId) -> Option<Row<'_>> {
        let node = self.get(id)?;
        let mut depth = 0;
        let mut at = node.parent?;
        while at != self.root_id {
            depth += 1;
            at = self.node(at).parent.expect("non-root nodes have parents");
        }
        Some(Self::make_row(id, depth, node))
    }

    /// The listing that would load directory `id`, to run on any thread.
    /// `None` if `id` is not a directory or is already listed, unless its last
    /// listing may have missed changes (see [`FileTree::fill`]), or it is the
    /// root and couldn't be reached.
    pub fn listing(&self, id: NodeId) -> Option<Listing> {
        let node = self.get(id)?;
        let missed = node.dropped_seq.max(self.rescan_seq) > node.listed_seq
            || (id == self.root_id && self.root_lost);
        (node.kind == EntryKind::Dir && (node.children.is_none() || missed))
            .then(|| self.listing_of(id))
    }

    /// Stores the result of running [`FileTree::listing`] for `id`. No IO.
    /// A directory that is already listed is reconciled with the result; an
    /// error is kept on the node, and the directory can be listed again.
    ///
    /// Listings and [`FileTree::apply`] can interleave, and listings can
    /// finish in any order, so a result is checked against what happened
    /// since its run started:
    ///
    /// - If the stored children came from a listing that started later, or
    ///   they or anything loaded below them were brought up to date after this
    ///   run started (by `apply`, by a listing of one of them, or by a listing
    ///   of the directory that ended then), the result may be older than
    ///   them. A listed directory discards it, as it could undo that; one not
    ///   listed yet stores it. Either way, unless the run ended before the
    ///   stored listing started, it may also have read something newer, so
    ///   [`FileTree::listing`] returns `Some` for `id` again: run that too.
    ///   A directory that owes a listing so is news for those above it too:
    ///   a listing of one of them started before then is older than it.
    /// - So it does if `apply` dropped a change to the unlisted directory (or
    ///   rescanned, even if the rescan was cut short) since: the result is
    ///   stored but may miss it.
    /// - If the directory's ignore flag changed since the listing was made
    ///   (a listing of its parent stored meanwhile reclassified it), the
    ///   result classified its entries by the old flag. It is discarded, and
    ///   `listing` offers the directory again. So it does for a listed
    ///   directory whose flag a stored listing of its parent changes.
    /// - If the path no longer held the directory (it was deleted, replaced,
    ///   or replaced by a symlink), the node is removed. But if since the run
    ///   started anything looked at the directory or its parent (`apply` saw
    ///   its path or updated either's children, or a listing of either was
    ///   stored, as a rescan stores), or a rescan (even one cut short) or a
    ///   root cut off may have missed news of it, the result is discarded
    ///   instead: the directory may be back. The node stays, and `listing`
    ///   still offers it; the watcher's event for the path removes it if it
    ///   is really gone.
    /// - If the listing couldn't get past a directory above it (missing,
    ///   another directory, or a symlink, perhaps only for a moment), what
    ///   the node has loaded stays as it was; a node with nothing loaded
    ///   keeps the error. That directory's own events say what became of it:
    ///   if it left, they remove it with everything below; if it came back,
    ///   they list it again with everything loaded below.
    /// - If the root couldn't be reached as the tree has it (past a directory
    ///   above it, for which no events come, or because it is another
    ///   directory now), what is loaded stays as it was too, whatever becomes
    ///   of the result, and `listing` offers the root (its listing's error
    ///   says why). Once a listing of the root that started after then is
    ///   stored, every directory is offered again, as changes made meanwhile
    ///   went unreported.
    pub fn fill(&mut self, id: NodeId, listed: Listed) {
        self.stats.listings += 1;
        // Whatever becomes of the result.
        if listed.lost == Some(Lost::Root) {
            self.cut_off(listed.done);
        }
        let Some(node) = self.get(id).filter(|node| node.kind == EntryKind::Dir) else {
            return;
        };
        // The run classified entries by an ignore flag the directory no
        // longer has (a listing of its parent changed it since the listing
        // was made): what it read is unusable, and a fresh listing is owed.
        let misjudged = listed.parent_ignored != node.ignored;
        // Something newer than this run's start is known of the directory or
        // below it, or may be: a rescan (even one cut short) or a root cut
        // off since, which no event may report. Unless the run ended before
        // what is stored started, it may also have read something newer than
        // what is stored. (A listing of the root that started before a cut
        // so can't be the one that finds it again.)
        let barrier = node.listed_seq.max(node.applied_seq);
        let behind = listed.seq < barrier.max(self.rescan_seq).max(self.cut_seq);
        let owes = misjudged || (behind && listed.done > node.listed_seq);
        // A listed directory keeps what is stored; one that isn't stores it.
        let older = listed.seq < node.listed_seq || (behind && node.children.is_some());
        let obsolete = listed.lost == Some(Lost::Itself) && self.seen_since(id, listed.seq);
        if !older && !obsolete && !misjudged {
            let mut changed = Vec::new();
            self.take(id, listed, &mut changed);
            for dir in changed {
                self.touch_children(dir);
            }
        }
        if owes && self.get(id).is_some() {
            // Also news for the directories above: a listing of one that
            // started before now may have missed `id` while it was away, and
            // mustn't drop it, with what it owes.
            let now = self.now();
            self.dropped(id, now);
        }
        self.finish();
    }

    /// Records a result that couldn't reach the root, which ended at `done`.
    fn cut_off(&mut self, done: u64) {
        if done > self.found_seq {
            self.root_lost = true;
            self.cut_seq = self.cut_seq.max(done);
        }
    }

    /// Whether after time `seq` anything looked at directory `id` or its
    /// parent: `apply` saw its path, applied or dropped a change in either,
    /// or a listing of either that started then was stored. Or whether
    /// something may have looked without saying so: a rescan, even one cut
    /// short before reaching either, or a result that saw the root cut off,
    /// while changes may have gone unreported.
    fn seen_since(&self, id: NodeId, seq: u64) -> bool {
        let seen = |node: &Node| node.listed_seq.max(node.applied_seq).max(node.dropped_seq) > seq;
        let node = self.node(id);
        self.rescan_seq.max(self.cut_seq) > seq
            || seen(node)
            || node.parent.is_some_and(|parent| seen(self.node(parent)))
    }

    /// Lists `id` if needed (blocking) and shows its children.
    pub fn expand(&mut self, id: NodeId) {
        if let Some(listing) = self.listing(id) {
            self.io_calls += 1;
            let listed = listing.run();
            self.fill(id, listed);
        }
        self.set_expanded(id, true);
    }

    /// Hides a directory's children. They stay loaded.
    pub fn collapse(&mut self, id: NodeId) {
        self.set_expanded(id, false);
    }

    /// The number of rows in the flattened view.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The rows in `range` (clamped to [`FileTree::row_count`]), in display
    /// order. Costs O(range).
    pub fn rows(&self, range: Range<usize>) -> impl Iterator<Item = Row<'_>> {
        let end = range.end.min(self.rows.len());
        let start = range.start.min(end);
        self.rows[start..end]
            .iter()
            .map(|&(id, depth)| Self::make_row(id, depth, self.node(id)))
    }

    /// Brings loaded directories up to date with the disk and returns those
    /// whose children changed.
    ///
    /// Event kinds aren't trusted (FSEvents coalesces them and reports a
    /// rename as two unrelated paths). Instead each path whose parent is a
    /// listed directory is stat'ed there: present means insert or update,
    /// absent means remove, with its loaded subtree. A directory with a new
    /// identity is another directory and comes back collapsed and unlisted.
    /// A path whose parent isn't listed (it is unlisted, ignored, `.git`, a
    /// file, or unknown) is dropped without IO, unless the tree holds a name
    /// differing from a component of it only in case (a case-only rename),
    /// whose directory is then re-listed. A listed directory's own path, if
    /// it is still that directory, lists it and every loaded directory below
    /// it again: it may have been away, under another name, while things in
    /// it changed, reported under that name. So does the root's own path (the
    /// root may also be gone, or another directory). [`FsChange::Rescan`]
    /// lists the root and every loaded directory again, once.
    ///
    /// Directories are opened a component at a time from `/` without
    /// following symlinks, and checked against the directories the tree has,
    /// spelling included; entries are looked up, and ignore files read,
    /// through those handles. A path that now runs through a symlink
    /// therefore never reaches outside the project: the replaced directory
    /// is reconciled instead (or, misspelled, its parent re-listed).
    pub fn apply(&mut self, changes: &[FsChange]) -> Vec<NodeId> {
        let mut pass = Pass::default();
        if changes
            .iter()
            .any(|change| matches!(change, FsChange::Rescan))
        {
            // The root's own path still says what became of it.
            let root_reported = changes.iter().any(|change| match change {
                FsChange::Path(path) => self
                    .relative(path)
                    .is_some_and(|rel| rel.components().next().is_none()),
                FsChange::Rescan => false,
            });
            self.rescan_seq = self.now();
            let (changed, listed) = (&mut pass.changed, &mut HashSet::new());
            self.rescan(self.root_id, changed, listed, root_reported);
        } else {
            // In order, so a directory's paths come together and it is
            // opened once, with only the directories on the way to it open.
            let mut paths: Vec<&Path> = changes
                .iter()
                .filter_map(|change| match change {
                    FsChange::Path(path) => Some(path.as_path()),
                    FsChange::Rescan => None,
                })
                .collect();
            paths.sort();
            paths.dedup();
            for path in paths {
                self.apply_path(path, &mut pass);
            }
            // Listings open their own.
            pass.dirs.clear();
            pass.above.clear();
            // Whole subtrees first, the outermost first, then the directories
            // they didn't list.
            let mut listed = HashSet::new();
            let mut subtrees = std::mem::take(&mut pass.rescan);
            subtrees.retain(|&dir| self.get(dir).is_some());
            subtrees.sort_by_key(|&dir| (self.depth(dir), dir));
            for dir in subtrees {
                if !listed.contains(&dir) && self.get(dir).is_some() {
                    self.rescan(dir, &mut pass.changed, &mut listed, pass.root_reported);
                }
            }
            let mut relist = std::mem::take(&mut pass.relist);
            relist.sort();
            relist.dedup();
            for dir in relist {
                if self.get(dir).is_some() && listed.insert(dir) {
                    self.relist(dir, &mut pass.changed, pass.root_reported);
                }
            }
        }
        let mut changed = pass.changed;
        changed.sort();
        changed.dedup();
        changed.retain(|&dir| self.get(dir).is_some());
        for &dir in &changed {
            self.touch_children(dir);
        }
        self.finish();
        changed
    }

    fn apply_path(&mut self, path: &Path, pass: &mut Pass) {
        let Some(rel) = self.relative(path) else {
            return;
        };
        let mut dir = self.root_id;
        if rel.components().next().is_none() {
            // The root itself, which may be gone, another directory, or back
            // from elsewhere.
            pass.rescan.push(dir);
            pass.root_reported = true;
            return;
        }
        if self.node(dir).children.is_none() {
            // The root's listing failed; it is listed again on request.
            let now = self.now();
            self.dropped(dir, now);
            return;
        }
        let mut components = rel.components().peekable();
        let mut listed = None;
        while let Some(component) = components.next() {
            let Component::Normal(name) = component else {
                return;
            };
            let child = self.child_named(dir, name).map(|(_, id)| id);
            let below = components.peek().is_some();
            match child {
                Some(child) if below => {
                    let node = self.node(child);
                    if node.kind != EntryKind::Dir {
                        // Under a file or link: its own change says what it is.
                        return;
                    }
                    if node.children.is_some() {
                        dir = child;
                        continue;
                    }
                    // Nothing loaded to update, but a listing of it that is
                    // running may miss this.
                    let now = self.now();
                    self.dropped(child, now);
                    return;
                }
                Some(child) => {
                    // The entry itself changed. If it is a directory, a
                    // listing of it that is running may be of what was there
                    // before (a listed one is listed again below).
                    let node = self.node(child);
                    if node.kind == EntryKind::Dir {
                        if node.children.is_some() {
                            listed = Some(child);
                        } else {
                            let now = self.now();
                            self.dropped(child, now);
                        }
                    }
                }
                // Under a name the tree doesn't have: unknown, ignored or
                // `.git`. Only a case-only rename of a sibling needs work.
                None if below && !self.has_case_variant(dir, name) => return,
                None => {}
            }
            self.close_others(pass, dir);
            self.reconcile(dir, name, pass);
            // A listed directory still there as itself may have been away,
            // under another name, while things in it changed, which were
            // reported under that name.
            if let Some(listed) = listed
                && self.get(listed).is_some()
            {
                pass.rescan.push(listed);
            }
            return;
        }
    }

    /// The time on the shared clock, ticked.
    fn now(&self) -> u64 {
        listing::tick(&self.clock)
    }

    /// Records that a change to unlisted `dir` (or the failed root) was
    /// dropped at time `seq`: a listing of it that started before then may
    /// have missed it. It is also news that `dir` is there, for the
    /// directories above.
    fn dropped(&mut self, dir: NodeId, seq: u64) {
        self.node_mut(dir).dropped_seq = seq;
        self.touched(dir, seq);
    }

    /// Records that `dir`'s children, or something loaded below them, were
    /// brought up to date at time `seq`: a listing of `dir`, or of a directory
    /// above it, whose run started before then is older than they are.
    fn touched(&mut self, dir: NodeId, seq: u64) {
        let mut at = Some(dir);
        while let Some(id) = at {
            let node = self.node_mut(id);
            node.applied_seq = node.applied_seq.max(seq);
            at = node.parent;
        }
    }

    /// How many directories are above `id`, the root included.
    fn depth(&self, id: NodeId) -> usize {
        let mut depth = 0;
        let mut at = self.node(id).parent;
        while let Some(parent) = at {
            depth += 1;
            at = self.node(parent).parent;
        }
        depth
    }

    /// Closes the directories opened in `pass` other than `dir` and those
    /// above it, so a batch holds no more open than one path needs.
    fn close_others(&self, pass: &mut Pass, dir: NodeId) {
        let mut chain = Vec::new();
        let mut at = self.get(dir).map(|_| dir);
        while let Some(id) = at {
            chain.push(id);
            at = self.node(id).parent;
        }
        pass.dirs.retain(|id, _| chain.contains(id));
    }

    /// Brings entry `name` of listed directory `dir` up to date with the disk.
    fn reconcile(&mut self, dir: NodeId, name: &OsStr, pass: &mut Pass) {
        // On a case-insensitive file system, after a case-only rename the
        // tree may hold the old spelling of `name`; only a listing tells
        // which is on disk. This comes before the checks that hide `name`,
        // since the old spelling may still need removing.
        if self.has_case_variant(dir, name) {
            pass.relist.push(dir);
            return;
        }
        if listing::is_git_dir(name) {
            return;
        }
        if !self.open_checked(dir, pass) {
            return;
        }
        let Some(Some(handle)) = pass.dirs.get(&dir) else {
            return;
        };
        self.stats.stats += 1;
        self.io_calls += 1;
        let present = match listing::stat_at(handle, name) {
            // The lookup found the entry under another spelling (a case-only
            // rename the tree may not have heard about).
            Ok(stat) if !stat.exact => {
                pass.relist.push(dir);
                return;
            }
            Ok(stat) => Some((stat.kind, stat.id)),
            Err(err) if listing::is_absent(&err) => None,
            // A listing records why, or recovers.
            Err(_) => {
                pass.relist.push(dir);
                return;
            }
        };
        let present = match present {
            Some((kind, id)) => {
                listing::checkpoint(Point::Stat);
                let is_dir = kind == EntryKind::Dir;
                let ignored = self.node(dir).ignored || self.ignores(pass, dir, name, is_dir);
                let id = is_dir.then_some(id);
                (self.options.show_ignored || !ignored).then_some((kind, id, ignored))
            }
            None => None,
        };
        // Timed after the last IO this relies on (the lookup and the ignore
        // files): a listing that started before then may have read either
        // before it did.
        let now = self.now();
        self.touched(dir, now);
        let existing = self.child_named(dir, name).map(|(_, id)| id);
        match (existing, present) {
            (None, None) => return,
            (Some(id), None) => self.unlink(dir, id),
            (Some(id), Some((kind, new_id, ignored))) => {
                let node = self.node(id);
                let same = if kind == EntryKind::Dir {
                    node.kind == EntryKind::Dir && (node.id.is_none() || node.id == new_id)
                } else {
                    node.kind != EntryKind::Dir
                };
                if !same {
                    self.unlink(dir, id);
                    self.insert(
                        dir,
                        Node::new(name.into(), kind, new_id, ignored, Some(dir)),
                    );
                } else if node.kind != kind || node.ignored != ignored {
                    let node = self.node_mut(id);
                    node.kind = kind;
                    node.ignored = ignored;
                } else {
                    return;
                }
            }
            (None, Some((kind, id, ignored))) => {
                self.insert(dir, Node::new(name.into(), kind, id, ignored, Some(dir)));
            }
        }
        pass.changed.push(dir);
    }

    /// Opens listed directory `dir` for looking up its entries, once per
    /// pass: the root a component at a time from `/`, others in their opened
    /// parent, never following a symlink, checked to be the directory the
    /// tree has. If it can't be reached so, this returns false. If its node
    /// is still there after that (it was gone only for a moment, can't be
    /// read, or a directory above it couldn't be reached), it is listed again
    /// at the end of the batch, since changes in it couldn't be looked up.
    fn open_checked(&mut self, dir: NodeId, pass: &mut Pass) -> bool {
        if let Some(handle) = pass.dirs.get(&dir) {
            return handle.is_some();
        }
        let handle = self.open_dir(dir, pass);
        let opened = handle.is_some();
        pass.dirs.insert(dir, handle);
        #[cfg(test)]
        {
            let open = pass.dirs.values().flatten().count();
            self.open_peak = self.open_peak.max(open);
        }
        if !opened && self.get(dir).is_some() {
            pass.relist.push(dir);
        }
        opened
    }

    /// Opens `dir` for [`FileTree::open_checked`]. If the path no longer
    /// leads to it (it was removed, or replaced by another directory or a
    /// symlink), its name is reconciled in its parent instead. If it is
    /// spelled otherwise on disk (renamed changing only case), its parent is
    /// re-listed.
    fn open_dir(&mut self, dir: NodeId, pass: &mut Pass) -> Option<File> {
        let parent = self.node(dir).parent;
        let opened = match parent {
            None => {
                self.io_calls += 1;
                listing::open_dirs(&self.root, Access::Search)
                    .map(|mut dirs| {
                        let root = dirs.pop().expect("`/` is always opened");
                        pass.above = dirs;
                        root
                    })
                    .map_err(|unreached| unreached.err)
            }
            Some(parent) => {
                if !self.open_checked(parent, pass) || self.get(dir).is_none() {
                    return None;
                }
                let parent_handle = pass.dirs[&parent].as_ref().expect("parent is open");
                self.io_calls += 1;
                listing::open_at(parent_handle, &self.node(dir).name, Access::Search)
            }
        };
        match opened.and_then(|file| Ok((listing::identity(&file)?, file))) {
            Ok((id, file)) if self.node(dir).id.is_none_or(|known| known == id) => {
                // A case-insensitive lookup opens the directory under its old
                // spelling too; only its entry in the parent has the new one.
                if let Some(parent) = parent
                    && self.ignores_case
                {
                    let parent_handle = pass.dirs[&parent].as_ref().expect("parent is open");
                    self.stats.stats += 1;
                    self.io_calls += 1;
                    let named = listing::stat_at(parent_handle, &self.node(dir).name);
                    if !named.is_ok_and(|stat| stat.exact && stat.id == id) {
                        pass.relist.push(parent);
                        return None;
                    }
                }
                if parent.is_none() && self.root_lost {
                    // Reached again: list everything loaded again.
                    pass.rescan.push(dir);
                }
                return Some(file);
            }
            // Another directory, a symlink, or nothing.
            Ok(_) => {}
            Err(err) if listing::is_absent(&err) => {}
            // It can't be read (or no descriptor was left).
            Err(_) => return None,
        }
        if let Some(parent) = parent {
            listing::checkpoint(Point::Unreached);
            let name = self.node(dir).name.clone();
            self.reconcile(parent, &name, pass);
        }
        None
    }

    /// Lists `start` and every loaded directory below it again, once each,
    /// skipping those in `listed`, and marks the directories below it that
    /// aren't loaded as missing something (a listing of one that is running
    /// may predate this).
    fn rescan(
        &mut self,
        start: NodeId,
        changed: &mut Vec<NodeId>,
        listed: &mut HashSet<NodeId>,
        root_reported: bool,
    ) {
        let mut queue = VecDeque::from([start]);
        while let Some(dir) = queue.pop_front() {
            // The root is listed even if its last listing failed.
            if self
                .get(dir)
                .is_none_or(|node| node.children.is_none() && dir != self.root_id)
                || !listed.insert(dir)
            {
                continue;
            }
            self.relist(dir, changed, root_reported);
            if dir == self.root_id && self.root_lost {
                // Nothing below it can be reached either.
                return;
            }
            let Some(children) = self.get(dir).and_then(|node| node.children.clone()) else {
                continue;
            };
            let seq = self.now();
            for child in children {
                let node = self.node(child);
                if node.kind != EntryKind::Dir {
                    continue;
                }
                if node.children.is_some() {
                    queue.push_back(child);
                } else {
                    self.dropped(child, seq);
                }
            }
        }
    }

    /// Lists `dir` now and stores the result. `root_reported` says the
    /// root's own path is in the batch.
    fn relist(&mut self, dir: NodeId, changed: &mut Vec<NodeId>, root_reported: bool) {
        let mut listed = self.listing_of(dir).run();
        self.stats.listings += 1;
        self.io_calls += 1;
        if listed.lost == Some(Lost::Root) {
            if root_reported && dir == self.root_id {
                // Reported as itself while it can't be reached past what is
                // above it: it went with that (FSEvents reports nothing when
                // a directory above merely moves).
                listed.lost = Some(Lost::Itself);
            } else {
                self.cut_off(listed.done);
            }
        }
        self.take(dir, listed, changed);
    }

    /// Stores a listing of `dir` that isn't older than its children.
    fn take(&mut self, dir: NodeId, listed: Listed, changed: &mut Vec<NodeId>) {
        let parent = self.node(dir).parent;
        match (listed.lost, parent) {
            // Whatever its name holds now comes back with the name's own
            // event.
            (Some(Lost::Itself), Some(parent)) => {
                self.unlink(parent, dir);
                self.touched(parent, listed.done);
                changed.push(parent);
                return;
            }
            // A directory above it, or the root, couldn't be reached, maybe
            // only for a moment. What this one has loaded stays as it was: that
            // directory's own events remove it if it left, or list it again
            // with this one if it came back (for the root, `listing` offers
            // it). One with nothing loaded keeps the error.
            (Some(Lost::Above | Lost::Root), _) if self.node(dir).children.is_some() => return,
            _ => {}
        }
        let mut owes = false;
        if parent.is_none() {
            if listed.result.is_ok() {
                // Found again: everything, loaded or not, may have missed
                // changes made while it couldn't be reached, and this listing
                // may have read the root before the last sign of that.
                if std::mem::take(&mut self.root_lost) {
                    let seq = self.now();
                    for slot in &mut self.slots {
                        if let Some(node) = &mut slot.node
                            && node.kind == EntryKind::Dir
                        {
                            // The root too is newer than a listing of it that
                            // started before then, which mustn't drop
                            // directories that owe a listing.
                            node.applied_seq = node.applied_seq.max(seq);
                            if node.parent.is_some() {
                                node.dropped_seq = seq;
                            }
                        }
                    }
                    owes = listed.seq < self.cut_seq;
                }
                self.found_seq = self.found_seq.max(listed.seq);
            } else if listed.lost != Some(Lost::Root) {
                // Gone or unreadable, it drops everything, so nothing is left
                // to have missed anything.
                self.root_lost = false;
            }
        }
        let node = self.node_mut(dir);
        node.listed_seq = listed.seq;
        // The root's identity is whatever is at its path now.
        if listed.id.is_some() || node.parent.is_none() {
            node.id = listed.id;
        }
        // As of when the listing ended, for it and the directories above: a
        // listing of either that started before then may have read before it.
        self.touched(dir, listed.done);
        if owes {
            let now = self.now();
            self.dropped(dir, now);
        }
        if self.store(dir, listed.result) {
            changed.push(dir);
        }
    }

    fn listing_of(&self, id: NodeId) -> Listing {
        // What the tree knows of the root and each directory down to `id`;
        // a root replaced by another directory is adopted when listing it.
        let mut ids = Vec::new();
        let mut at = Some(id);
        while let Some(dir) = at {
            let node = self.node(dir);
            ids.push(node.id.filter(|_| node.parent.is_some() || dir != id));
            at = node.parent;
        }
        ids.reverse();
        let node = self.node(id);
        Listing {
            root: self.root.clone(),
            rel: self.rel_path(id).expect("listing a live node"),
            show_ignored: self.options.show_ignored,
            parent_ignored: node.ignored,
            ids,
            clock: self.clock.clone(),
            global: self.global.clone(),
        }
    }

    /// Stores a listing of directory `dir`, reconciling it with the children
    /// already loaded so unchanged entries keep their ids and subtrees.
    /// Returns whether the node changed.
    fn store(&mut self, dir: NodeId, result: io::Result<Vec<Entry>>) -> bool {
        let entries = match result {
            Ok(entries) => entries,
            Err(err) => {
                if let Some(children) = self.node_mut(dir).children.take() {
                    for child in children {
                        self.remove_subtree(child);
                    }
                }
                self.node_mut(dir).error = Some(err);
                return true;
            }
        };
        let node = self.node_mut(dir);
        let mut changed = node.error.take().is_some() || node.children.is_none();
        let old = node.children.take().unwrap_or_default();
        let mut old = old.into_iter().peekable();
        let mut children = Vec::with_capacity(entries.len());
        // Listed directories whose ignore flag changed: their children were
        // classified by the old one.
        let mut reclassified = Vec::new();
        for entry in entries {
            let mut kept = None;
            while let Some(&id) = old.peek() {
                let node = self.node(id);
                match listing::compare(node.kind, &node.name, entry.kind, &entry.name) {
                    Ordering::Less => {
                        old.next();
                        self.remove_subtree(id);
                        changed = true;
                    }
                    // Another directory at the same name, with its own
                    // contents: replace the node.
                    Ordering::Equal
                        if entry.kind == EntryKind::Dir
                            && node.id.is_some()
                            && entry.id.is_some()
                            && node.id != entry.id =>
                    {
                        old.next();
                        self.remove_subtree(id);
                        changed = true;
                        break;
                    }
                    Ordering::Equal => {
                        old.next();
                        kept = Some(id);
                        break;
                    }
                    Ordering::Greater => break,
                }
            }
            let id = match kept {
                Some(id) => {
                    let node = self.node_mut(id);
                    if entry.id.is_some() {
                        node.id = entry.id;
                    }
                    if node.kind != entry.kind || node.ignored != entry.ignored {
                        if node.ignored != entry.ignored && node.children.is_some() {
                            reclassified.push(id);
                        }
                        node.kind = entry.kind;
                        node.ignored = entry.ignored;
                        changed = true;
                    }
                    id
                }
                None => {
                    changed = true;
                    self.alloc(Node::new(
                        entry.name,
                        entry.kind,
                        entry.id,
                        entry.ignored,
                        Some(dir),
                    ))
                }
            };
            children.push(id);
        }
        for id in old {
            self.remove_subtree(id);
            changed = true;
        }
        self.node_mut(dir).children = Some(children);
        if !reclassified.is_empty() {
            let now = self.now();
            for id in reclassified {
                self.dropped(id, now);
            }
        }
        changed
    }

    /// Whether entry `name` of directory `dir`, opened in this pass, is
    /// matched by an ignore file.
    fn ignores(&mut self, pass: &mut Pass, dir: NodeId, name: &OsStr, is_dir: bool) -> bool {
        self.io_calls += 1;
        let mut path = self.path(dir).expect("dir is live");
        path.push(name);
        self.rules(pass, dir).ignores(&self.global, &path, is_dir)
    }

    /// The ignore rules in force in directory `dir`, opened in this pass,
    /// read through the handles of it and every directory above it.
    fn rules(&self, pass: &mut Pass, dir: NodeId) -> Arc<Rules> {
        if let Some(rules) = pass.rules.get(&dir) {
            return rules.clone();
        }
        let parent = match self.node(dir).parent {
            Some(parent) => Some(self.rules(pass, parent)),
            None => self
                .root
                .parent()
                .and_then(|above| Rules::chain(&pass.above, above)),
        };
        let handle = pass.dirs[&dir].as_ref().expect("dir was opened");
        let rules = Rules::load(parent, handle, &self.path(dir).expect("dir is live"));
        pass.rules.insert(dir, rules.clone());
        rules
    }

    /// The path of a node relative to the root.
    fn rel_path(&self, id: NodeId) -> Option<PathBuf> {
        self.get(id)?;
        let mut names = Vec::new();
        let mut at = id;
        while at != self.root_id {
            let node = self.node(at);
            names.push(&node.name);
            at = node.parent.expect("non-root nodes have parents");
        }
        Some(names.into_iter().rev().collect())
    }

    fn set_expanded(&mut self, id: NodeId, expanded: bool) {
        let root = self.root_id;
        let Some(node) = self.get_mut(id) else {
            return;
        };
        if node.kind != EntryKind::Dir || node.expanded == expanded || id == root {
            return;
        }
        node.expanded = expanded;
        self.version += 1;
        if self.row_shown(id) {
            self.rows_dirty = true;
        }
        self.finish();
    }

    /// Records that `dir`'s children changed.
    fn touch_children(&mut self, dir: NodeId) {
        self.version += 1;
        if self.row_shown(dir) || self.children_shown(dir) {
            self.rows_dirty = true;
        }
    }

    fn finish(&mut self) {
        if self.rows_dirty {
            self.rows_dirty = false;
            self.rebuild_rows();
        }
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        let mut stack: Vec<(NodeId, usize)> = Vec::new();
        let push_children = |stack: &mut Vec<(NodeId, usize)>, node: &Node, depth| {
            if let Some(children) = &node.children {
                stack.extend(children.iter().rev().map(|&child| (child, depth)));
            }
        };
        push_children(&mut stack, self.node(self.root_id), 0);
        while let Some((id, depth)) = stack.pop() {
            self.rows.push((id, depth));
            let node = self.node(id);
            if node.expanded {
                push_children(&mut stack, node, depth + 1);
            }
        }
    }

    /// Whether `id`'s children are rows: it and every ancestor are expanded.
    fn children_shown(&self, id: NodeId) -> bool {
        let mut at = Some(id);
        while let Some(id) = at {
            let node = self.node(id);
            if !node.expanded {
                return false;
            }
            at = node.parent;
        }
        true
    }

    /// Whether `id` itself is a row.
    fn row_shown(&self, id: NodeId) -> bool {
        self.node(id)
            .parent
            .is_some_and(|parent| self.children_shown(parent))
    }

    fn make_row(id: NodeId, depth: usize, node: &Node) -> Row<'_> {
        Row {
            id,
            depth,
            name: &node.name,
            kind: node.kind,
            expanded: node.expanded,
            ignored: node.ignored,
            error: node.error.as_ref(),
        }
    }

    /// `path` relative to the root, if it is under it.
    fn relative(&self, path: &Path) -> Option<PathBuf> {
        if path.is_relative() {
            return Some(path.to_path_buf());
        }
        path.strip_prefix(&self.root)
            .or_else(|_| path.strip_prefix(&self.given_root))
            .ok()
            .map(Path::to_path_buf)
    }

    /// The child of `dir` called exactly `name`, with its index.
    fn child_named(&self, dir: NodeId, name: &OsStr) -> Option<(usize, NodeId)> {
        let children = self.get(dir)?.children.as_ref()?;
        let dirs = children.partition_point(|&id| self.node(id).kind == EntryKind::Dir);
        let search = |range: Range<usize>, kind| {
            children[range.clone()]
                .binary_search_by(|&id| {
                    let node = self.node(id);
                    listing::compare(node.kind, &node.name, kind, name)
                })
                .ok()
                .map(|ix| (range.start + ix, children[range.start + ix]))
        };
        search(0..dirs, EntryKind::Dir).or_else(|| search(dirs..children.len(), EntryKind::File))
    }

    /// Whether `dir` has a child whose name equals `name` but for case, on a
    /// volume where the two can be the same entry.
    fn has_case_variant(&self, dir: NodeId, name: &OsStr) -> bool {
        let Some(children) = self.children(dir).filter(|_| self.ignores_case) else {
            return false;
        };
        let dirs = children.partition_point(|&id| self.node(id).kind == EntryKind::Dir);
        [
            (0..dirs, EntryKind::Dir),
            (dirs..children.len(), EntryKind::File),
        ]
        .into_iter()
        .any(|(range, kind)| {
            let part = &children[range];
            let ix = part.partition_point(|&id| {
                let node = self.node(id);
                listing::compare(node.kind, &node.name, kind, name) == Ordering::Less
            });
            // Case variants sort next to each other.
            [ix.checked_sub(1), Some(ix), Some(ix + 1)]
                .into_iter()
                .flatten()
                .filter_map(|ix| part.get(ix))
                .any(|&id| listing::same_folded(&self.node(id).name, name))
        })
    }

    /// Inserts `node`, a new child of listed `dir`, in display order.
    fn insert(&mut self, dir: NodeId, node: Node) {
        let ix = {
            let children = self.node(dir).children.as_ref().expect("dir is listed");
            children.partition_point(|&id| {
                let child = self.node(id);
                listing::compare(child.kind, &child.name, node.kind, &node.name) == Ordering::Less
            })
        };
        let id = self.alloc(node);
        self.node_mut(dir)
            .children
            .as_mut()
            .expect("dir is listed")
            .insert(ix, id);
    }

    /// Removes `id` from its parent `dir`, with its subtree.
    fn unlink(&mut self, dir: NodeId, id: NodeId) {
        let children = self.node_mut(dir).children.as_mut().expect("dir is listed");
        let ix = children
            .iter()
            .position(|&child| child == id)
            .expect("id is a child of dir");
        children.remove(ix);
        self.remove_subtree(id);
    }

    fn alloc(&mut self, node: Node) -> NodeId {
        self.stats.loaded_entries += 1;
        match self.free.pop() {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                slot.node = Some(node);
                NodeId {
                    index,
                    generation: slot.generation,
                }
            }
            None => {
                let index = u32::try_from(self.slots.len()).expect("fewer than 2^32 nodes");
                self.slots.push(Slot {
                    generation: 0,
                    node: Some(node),
                });
                NodeId {
                    index,
                    generation: 0,
                }
            }
        }
    }

    /// Frees `id` and everything loaded under it. The caller unlinks `id`.
    fn remove_subtree(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let slot = &mut self.slots[id.index as usize];
            let node = slot.node.take().expect("removing a live node");
            slot.generation += 1;
            self.free.push(id.index);
            self.stats.loaded_entries -= 1;
            stack.extend(node.children.into_iter().flatten());
        }
    }

    fn get(&self, id: NodeId) -> Option<&Node> {
        let slot = self.slots.get(id.index as usize)?;
        (slot.generation == id.generation).then_some(slot.node.as_ref())?
    }

    fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let slot = self.slots.get_mut(id.index as usize)?;
        (slot.generation == id.generation).then_some(slot.node.as_mut())?
    }

    fn node(&self, id: NodeId) -> &Node {
        self.get(id).expect("live node")
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.get_mut(id).expect("live node")
    }
}
