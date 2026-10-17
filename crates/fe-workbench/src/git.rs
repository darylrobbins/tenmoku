//! Keeps git's view of the project live: the Changes panel's list, and each
//! open editor's diff base, the file's text at HEAD that its changed rows
//! are found against.
//!
//! [`GitState`] is the workbench's, one per root, and the one thing that
//! reads the repository's status: as soon as the root's project tree opens
//! (unless something read it already), as the tree shows the Changes
//! panel's list too, so the status is read whether or not that panel's
//! ever drawn; for the Changes panel when it's first drawn if nothing has
//! run yet; on `git.refresh`; and as files change. Every batch from the
//! project panel's watcher, `.git/` paths and rescans included, schedules a
//! refresh [`DEBOUNCE`] after the last batch. A refresh reads the status
//! and HEAD's id on the background executor, one run at a time: a refresh
//! asked for meanwhile runs once more after it lands, or, if a batch's
//! debounce is still running then, when that runs out. The Changes panel
//! numbers each run as it starts, so a run's result shows only if nothing
//! newer has. The refresh then reloads, in the background, the diff base of
//! each open editor whose file's status changed, or of every one if HEAD
//! moved.
//!
//! A refresh that finds the repository also has each open diff view's git
//! side read again (`git_diff`'s [`Workbench::refresh_git_diffs`]). A click on an editor's change bar opens its file's diff
//! against HEAD there.
//!
//! Each refresh that finds the repository also reads where HEAD points,
//! which the status bar's branch item shows (`git_branch`), with whether
//! anything changed; one that finds none hides the item.
//!
//! An editor opened gets its base when the workbench starts observing it:
//! its file's text at HEAD if it has one; else, as only the status tells
//! untracked from ignored and the last one may be older than the file,
//! from a refresh at once.
//!
//! An editor's base is its file's text at HEAD; an empty text if the file
//! is untracked or added, so every row shows as added; and none if the
//! file is outside the repository, ignored or binary. A base that can't be
//! read as an editor is opened has a refresh at once, as for a file not in
//! HEAD; one that can't be read then either leaves the editor's as it was,
//! and is read again at the next refresh.
//!
//! Known limit: when the repository's `.git` is outside the project root
//! (the root is a subdirectory of the working tree), its changes aren't
//! watched, so a commit, checkout or `git add` shows only after the next
//! change under the root or a `git.refresh`.

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use fe_git::{ChangeKind, FileStatus, GitError, Head, Repo};
use fe_gpui::{EditorView, commands::EditorViewEvent};
use fe_project::FsChange;
use gpui::{App, AppContext as _, Context, Entity, EntityId, Subscription, Task, WeakEntity};

use crate::{
    GitPanel, Workbench,
    git_panel::{self, Located},
};

/// How long after the last watcher batch a refresh runs.
pub(crate) const DEBOUNCE: Duration = Duration::from_millis(300);

/// How far into a file git looks for a NUL byte to decide it's binary.
const BINARY_PROBE_LEN: usize = 8000;

/// How many symlinks a path's resolved through, at most.
const MAX_LINKS: usize = 40;

/// The repository the root is in.
struct Found {
    repo: Repo,
    /// Its working tree, through symlinks.
    resolved: PathBuf,
    /// Where the root is in it, for the Changes panel.
    located: Located,
}

/// What a refresh read: HEAD's id, where HEAD points, and the changed
/// files' statuses, by path from the working tree.
#[derive(Default)]
struct Snapshot {
    head: Option<String>,
    /// `None` if it couldn't be read.
    branch: Option<Head>,
    kinds: HashMap<PathBuf, ChangeKind>,
}

/// What a refresh's status run found: the repository the root is in, if
/// any, with its statuses and what was read.
type Statuses = Result<Option<(Arc<Found>, Vec<FileStatus>, Snapshot)>, GitError>;

/// An open editor's file, for a background run.
struct Wanted {
    editor: EntityId,
    path: PathBuf,
    /// Whether it has no base for this path yet.
    new: bool,
}

/// What reading a file's base found.
enum Read {
    Base(Option<Arc<str>>),
    /// Not in HEAD or unreadable, and only a status read now tells what it is.
    Unlisted,
    /// HEAD's text couldn't be read.
    Failed,
}

/// A base for an editor's file: the editor, the path it's for, the base.
type Base = (EntityId, PathBuf, Read);

pub(crate) struct GitState {
    root: Option<PathBuf>,
    /// The root's Changes panel, which refreshes list in.
    changes: WeakEntity<GitPanel>,
    /// Whose open editors are given bases.
    workbench: WeakEntity<Workbench>,
    /// The repository, once found.
    repo: Option<Arc<Found>>,
    /// Whether a refresh has found the root is in no repository.
    no_repo: bool,
    /// What the last refresh read.
    snapshot: Option<Arc<Snapshot>>,
    /// The editors given a base, and the path it's for.
    based: HashMap<EntityId, PathBuf>,
    /// The editors whose base couldn't be read, and the path it's for: the
    /// next refresh reads it again.
    failed: HashMap<EntityId, PathBuf>,
    /// Hears each tracked editor's change bar clicked.
    bar_clicks: HashMap<EntityId, Subscription>,
    /// The refresh due after the last watcher batch.
    debounce: Option<Task<()>>,
    /// Whether `debounce` is still to run out.
    due: bool,
    /// The run in flight: a refresh's status or bases, or new editors'
    /// bases.
    running: Option<Task<()>>,
    /// Whether a refresh was asked for while something ran.
    again: bool,
    /// Whether a refresh has started or been asked for.
    asked: bool,
    /// The branch picker's listings started, the last one the only one
    /// that may show (`git_branch`).
    branch_listings: u64,
    #[cfg(test)]
    test: std::rc::Rc<std::cell::RefCell<git_panel::TestHooks>>,
}

impl GitState {
    /// The state for `root`, listing in `changes` and giving `workbench`'s
    /// editors their bases. Nothing runs until a refresh is asked for (the
    /// workbench asks once the root's tree opens), the panel's drawn, an
    /// editor's tracked or a batch comes.
    pub(crate) fn new(
        root: Option<PathBuf>,
        changes: &Entity<GitPanel>,
        workbench: WeakEntity<Workbench>,
        #[allow(unused_variables)] cx: &App,
    ) -> Self {
        Self {
            root,
            changes: changes.downgrade(),
            workbench,
            repo: None,
            no_repo: false,
            snapshot: None,
            based: HashMap::new(),
            failed: HashMap::new(),
            bar_clicks: HashMap::new(),
            debounce: None,
            due: false,
            running: None,
            again: false,
            asked: false,
            branch_listings: 0,
            #[cfg(test)]
            test: changes.read(cx).test_hooks(),
        }
    }

    /// Stops for good, the root replaced: what's running or due is
    /// cancelled, and nothing runs again.
    pub(crate) fn close(&mut self) {
        self.root = None;
        self.debounce = None;
        self.due = false;
        self.running = None;
        self.again = false;
    }

    /// The workbench whose editors are given bases.
    pub(crate) fn workbench(&self) -> WeakEntity<Workbench> {
        self.workbench.clone()
    }

    /// Starts another branch listing, superseding those before: its number.
    pub(crate) fn start_branch_listing(&mut self) -> u64 {
        self.branch_listings += 1;
        self.branch_listings
    }

    /// Whether listing `number` is the last one started.
    pub(crate) fn is_last_branch_listing(&self, number: u64) -> bool {
        self.branch_listings == number
    }

    /// A batch from the watcher: refreshes [`DEBOUNCE`] after the last.
    pub(crate) fn fs_changed(&mut self, changes: &[FsChange], cx: &mut Context<Self>) {
        if self.root.is_none() || changes.is_empty() {
            return;
        }
        // Replacing the timer cancels the one before.
        self.due = true;
        self.debounce = Some(cx.spawn(async move |git, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            git.update(cx, |git, cx| {
                git.due = false;
                git.refresh(cx);
            })
            .ok();
        }));
    }

    /// Reads the status again, in the background: now if nothing's
    /// running, else once more after it lands. Touches neither the panel
    /// nor the workbench until the run starts, so either may ask.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.asked = true;
        if self.running.is_some() {
            self.again = true;
            return;
        }
        let repo = self.repo.clone();
        let changes = self.changes.clone();
        #[cfg(test)]
        let (ran, gate) = self.test.borrow_mut().started(cx);
        self.running = Some(cx.spawn(async move |git, cx| {
            let seq = changes.update(cx, |changes, _| changes.begin()).ok();
            // The files open as the status is read, which it's fresh for.
            let Ok(files) = git.update(cx, |git, cx| git.open_files(cx)) else {
                return;
            };
            let statuses = cx.background_executor().spawn(async move {
                let statuses = read(&root, repo);
                #[cfg(test)]
                {
                    ran();
                    gate.await;
                }
                statuses
            });
            let statuses = statuses.await;
            git.update(cx, |git, cx| git.read_status(seq, statuses, &files, cx))
                .ok();
        }));
    }

    /// Refreshes unless a refresh has started or been asked for already, as
    /// for an editor opened first: what the root's tree opening asks, so
    /// the tree shows the status without reading it twice.
    pub(crate) fn refresh_once(&mut self, cx: &mut Context<Self>) {
        if !self.asked {
            self.refresh(cx);
        }
    }

    /// Gives the editors among `editors` that have no base for their file
    /// one, and forgets closed ones: from HEAD in the background, or a
    /// refresh now for files HEAD doesn't have. While something runs,
    /// they wait for it to land; one whose base failed waits for a
    /// refresh. Each one's change bar clicked opens its file's diff.
    pub(crate) fn track(&mut self, editors: &[Entity<EditorView>], cx: &mut Context<Self>) {
        let open: Vec<EntityId> = editors.iter().map(Entity::entity_id).collect();
        self.based.retain(|editor, _| open.contains(editor));
        self.failed.retain(|editor, _| open.contains(editor));
        self.bar_clicks.retain(|editor, _| open.contains(editor));
        for editor in editors {
            if !self.bar_clicks.contains_key(&editor.entity_id()) {
                let clicks = self.hear_bar_clicks(editor, cx);
                self.bar_clicks.insert(editor.entity_id(), clicks);
            }
        }
        if self.root.is_none() || self.running.is_some() {
            return;
        }
        let wanted = self.wanted(editors, true, cx);
        if wanted.is_empty() {
            return;
        }
        if self.no_repo {
            let bases = wanted
                .into_iter()
                .map(|w| (w.editor, w.path, Read::Base(None)));
            return self.set_bases(bases.collect(), editors, cx);
        }
        let (Some(repo), Some(snapshot)) = (self.repo.clone(), self.snapshot.clone()) else {
            return self.refresh(cx);
        };
        self.load_bases(cx, move || {
            let bases = wanted.into_iter().map(|wanted| {
                let relative = relative(&repo, &wanted.path);
                let base = base(&repo, &snapshot, false, relative.as_deref());
                (wanted.editor, wanted.path, base)
            });
            bases.collect()
        });
    }

    /// Opens `editor`'s file against HEAD as its change bar is clicked.
    fn hear_bar_clicks(&self, editor: &Entity<EditorView>, cx: &mut Context<Self>) -> Subscription {
        let workbench = self.workbench.clone();
        cx.subscribe(editor, move |_, editor, event: &EditorViewEvent, cx| {
            if let EditorViewEvent::ChangeBarClicked { rows } = event {
                Workbench::change_bar_clicked(workbench.clone(), editor, rows.clone(), cx);
            }
        })
    }

    /// `editors`' files, each marked new if it has no base for its path:
    /// only the new ones whose base hasn't failed if `only_new`.
    fn wanted(&self, editors: &[Entity<EditorView>], only_new: bool, cx: &App) -> Vec<Wanted> {
        editors
            .iter()
            .filter_map(|editor| {
                let path = editor.read(cx).model().path()?.to_owned();
                let id = editor.entity_id();
                let new = self.based.get(&id) != Some(&path);
                let wanted = match only_new {
                    true => new && self.failed.get(&id) != Some(&path),
                    false => true,
                };
                wanted.then_some(Wanted {
                    editor: id,
                    path,
                    new,
                })
            })
            .collect()
    }

    /// Takes in a refresh's status, read for the editors and paths in
    /// `files`: lists it in the panel as run `seq`, then reads the bases it
    /// changed. Editors opened on a file since are left to
    /// [`Self::track`], as the status may be older than the file.
    fn read_status(
        &mut self,
        seq: Option<u64>,
        statuses: Statuses,
        files: &[(EntityId, PathBuf)],
        cx: &mut Context<Self>,
    ) {
        let editors = self.open_editors(cx);
        let wanted = |git: &Self, only_new, cx: &App| {
            let mut wanted = git.wanted(&editors, only_new, cx);
            wanted.retain(|w| {
                files
                    .iter()
                    .any(|(id, path)| *id == w.editor && *path == w.path)
            });
            wanted
        };
        let (statuses, landed) = match statuses {
            Ok(Some((repo, statuses, snapshot))) => {
                if let Some(head) = snapshot.branch.clone() {
                    self.show_branch(Some((head, !statuses.is_empty())), cx);
                }
                let after = Arc::new(snapshot);
                let before = self.snapshot.replace(after.clone());
                self.repo = Some(repo.clone());
                self.no_repo = false;
                self.refresh_diffs(cx);
                let wanted = wanted(self, false, cx);
                let located = repo.located.clone();
                self.load_bases(cx, move || rebase(&repo, before, &after, wanted));
                (Ok(Some((located, statuses))), false)
            }
            Ok(None) => {
                self.show_branch(None, cx);
                self.repo = None;
                self.no_repo = true;
                self.snapshot = None;
                let wanted = wanted(self, true, cx);
                let bases = wanted
                    .into_iter()
                    .map(|w| (w.editor, w.path, Read::Base(None)));
                self.set_bases(bases.collect(), &editors, cx);
                (Ok(None), true)
            }
            Err(error) => {
                log::warn!("can't read the git status: {error}");
                // Their bases stand until a refresh succeeds.
                for wanted in wanted(self, true, cx) {
                    self.failed.insert(wanted.editor, wanted.path);
                }
                (Err(error), true)
            }
        };
        if let Some(seq) = seq {
            (self.changes)
                .update(cx, |changes, cx| changes.landed(seq, statuses, cx))
                .ok();
        }
        if landed {
            self.finish(false, cx);
        }
    }

    /// Has the workbench read its diff views' git sides again: see the
    /// module docs.
    fn refresh_diffs(&self, cx: &mut App) {
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| workbench.refresh_git_diffs(cx));
        }
    }

    /// Has the workbench's status bar show `branch`, where HEAD points and
    /// whether anything changed; none hides it.
    fn show_branch(&self, branch: Option<(Head, bool)>, cx: &mut App) {
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| workbench.set_branch_item(branch, cx));
        }
    }

    /// Reads bases with `load` in the background, then sets them.
    fn load_bases(
        &mut self,
        cx: &mut Context<Self>,
        load: impl FnOnce() -> Vec<Base> + Send + 'static,
    ) {
        let bases = cx.background_spawn(async move { load() });
        self.running = Some(cx.spawn(async move |git, cx| {
            let bases = bases.await;
            git.update(cx, |git, cx| {
                let unlisted = (bases.iter()).any(|(_, _, read)| matches!(read, Read::Unlisted));
                let editors = git.open_editors(cx);
                git.set_bases(bases, &editors, cx);
                git.finish(unlisted, cx);
            })
            .ok();
        }));
    }

    /// A run has landed: refreshes again if asked to meanwhile and no
    /// debounce is still to run out, or if a new editor's file needs it;
    /// else gives editors opened meanwhile their bases.
    fn finish(&mut self, unlisted: bool, cx: &mut Context<Self>) {
        self.running = None;
        let again = std::mem::take(&mut self.again) && !self.due;
        if again || unlisted {
            self.refresh(cx);
        } else {
            let editors = self.open_editors(cx);
            self.track(&editors, cx);
        }
    }

    /// Sets each base read on its editor, if it's still open on that path.
    /// One that failed is left as it was, to be read again.
    fn set_bases(&mut self, bases: Vec<Base>, editors: &[Entity<EditorView>], cx: &mut App) {
        for (id, path, read) in bases {
            let Some(editor) = editors.iter().find(|editor| editor.entity_id() == id) else {
                continue;
            };
            if editor.read(cx).model().path() != Some(&path) {
                continue;
            }
            let base = match read {
                Read::Base(base) => base,
                Read::Unlisted => continue,
                Read::Failed => {
                    self.based.remove(&id);
                    self.failed.insert(id, path);
                    continue;
                }
            };
            self.failed.remove(&id);
            self.based.insert(id, path);
            editor.update(cx, |editor, cx| {
                editor.model_mut().set_diff_base(base);
                cx.notify();
            });
        }
    }

    /// The open editors' files: each editor and its path.
    fn open_files(&self, cx: &App) -> Vec<(EntityId, PathBuf)> {
        (self.open_editors(cx).into_iter())
            .filter_map(|editor| {
                let path = editor.read(cx).model().path()?.to_owned();
                Some((editor.entity_id(), path))
            })
            .collect()
    }

    fn open_editors(&self, cx: &App) -> Vec<Entity<EditorView>> {
        (self.workbench.upgrade())
            .map(|workbench| workbench.read(cx).open_editors(cx))
            .unwrap_or_default()
    }
}

/// Finds the repository containing `root` unless `repo` is it, and reads
/// its status and HEAD's id. Runs on the background executor.
fn read(root: &Path, repo: Option<Arc<Found>>) -> Statuses {
    let repo = match repo {
        Some(repo) => repo,
        None => {
            let Some(repo) = Repo::discover(root)? else {
                return Ok(None);
            };
            let Some(located) = git_panel::locate(root, repo.workdir()) else {
                return Ok(None);
            };
            let resolved =
                (repo.workdir().canonicalize()).unwrap_or_else(|_| repo.workdir().to_owned());
            Arc::new(Found {
                repo,
                resolved,
                located,
            })
        }
    };
    let statuses = repo.repo.status()?;
    let workdir = repo.repo.workdir();
    let snapshot = Snapshot {
        head: repo.repo.head_id(),
        branch: (repo.repo.current_branch())
            .inspect_err(|error| log::warn!("can't read where HEAD points: {error}"))
            .ok(),
        kinds: (statuses.iter())
            .filter_map(|status| {
                let relative = status.path.strip_prefix(workdir).ok()?;
                Some((relative.to_owned(), status.kind))
            })
            .collect(),
    };
    Ok(Some((repo, statuses, snapshot)))
}

/// The bases of the `wanted` files that are new, or whose status changed
/// from `before` to `after`, or all of them if HEAD moved. Runs on the
/// background executor.
fn rebase(
    repo: &Found,
    before: Option<Arc<Snapshot>>,
    after: &Snapshot,
    wanted: Vec<Wanted>,
) -> Vec<Base> {
    let moved = (before.as_ref()).is_none_or(|before| before.head != after.head);
    let before = before.unwrap_or_default();
    wanted
        .into_iter()
        .filter_map(|wanted| {
            let relative = relative(repo, &wanted.path);
            let changed = (relative.as_ref())
                .is_some_and(|relative| before.kinds.get(relative) != after.kinds.get(relative));
            (wanted.new || moved || changed).then(|| {
                let base = base(repo, after, true, relative.as_deref());
                (wanted.editor, wanted.path, base)
            })
        })
        .collect()
}

/// The base of the file at `relative` in `repo`'s working tree: its text
/// at HEAD; empty if it's untracked or added; none if it's outside the
/// working tree (`relative` is `None`), ignored or binary. Unlisted if it's
/// not in HEAD or can't be read, unless `snapshot` was read just now, when
/// a read that fails is failed.
fn base(repo: &Found, snapshot: &Snapshot, fresh: bool, relative: Option<&Path>) -> Read {
    let Some(relative) = relative else {
        return Read::Base(None);
    };
    let path = repo.repo.workdir().join(relative);
    match repo.repo.head_text(&path) {
        Ok(Some(text)) => Read::Base(Some(text.into())),
        Ok(None) if !fresh => Read::Unlisted,
        Ok(None) => match snapshot.kinds.get(relative) {
            Some(ChangeKind::Untracked | ChangeKind::Added) if !is_binary(&path) => {
                Read::Base(Some("".into()))
            }
            _ => Read::Base(None),
        },
        Err(error) => {
            log::warn!("can't read {} at HEAD: {error}", relative.display());
            // A read not from a fresh status is tried once more, by a
            // refresh, which reads it fresh and settles on failing.
            match fresh {
                true => Read::Failed,
                false => Read::Unlisted,
            }
        }
    }
}

/// Whether the file at `path` is binary, as git decides: a NUL byte in its
/// first 8000. A file that can't be read isn't.
fn is_binary(path: &Path) -> bool {
    use std::io::Read as _;
    let mut head = Vec::with_capacity(BINARY_PROBE_LEN);
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(BINARY_PROBE_LEN as u64).read_to_end(&mut head));
    read.is_ok() && head.contains(&0)
}

/// `path` from `repo`'s working tree, through symlinks: `None` if it's
/// outside it, or in `.git`.
fn relative(repo: &Found, path: &Path) -> Option<PathBuf> {
    let relative = resolve(path).strip_prefix(&repo.resolved).ok()?.to_owned();
    let inside = relative.components().next()?.as_os_str() != ".git";
    inside.then_some(relative)
}

/// `path` through symlinks and `..`: its nearest existing ancestor's (the
/// file's own, if it exists), joined with the rest of it (a new or deleted
/// file, or directory). A symlink whose target is gone is followed to it
/// all the same. As given if that fails, or past [`MAX_LINKS`] symlinks.
pub(crate) fn resolve(path: &Path) -> PathBuf {
    let mut missing: Vec<OsString> = Vec::new();
    let mut at = path.to_owned();
    let mut links = 0;
    loop {
        if let Ok(resolved) = at.canonicalize() {
            return missing
                .into_iter()
                .rev()
                .fold(resolved, |path, name| path.join(name));
        }
        if let Ok(target) = std::fs::read_link(&at)
            && links < MAX_LINKS
        {
            links += 1;
            at = match at.parent() {
                Some(parent) => parent.join(target),
                None => target,
            };
            continue;
        }
        let Some(name) = at.file_name().map(ToOwned::to_owned) else {
            return path.to_owned();
        };
        missing.push(name);
        if !at.pop() {
            return path.to_owned();
        }
    }
}

#[cfg(test)]
impl GitState {
    /// Holds the refreshes started from now until [`Self::release_runs`],
    /// once they've read the status.
    pub(crate) fn hold_runs(&mut self) {
        self.test.borrow_mut().hold();
    }

    /// Lets the held refreshes, and the ones started from now, land.
    pub(crate) fn release_runs(&mut self) {
        self.test.borrow_mut().release();
    }

    /// Refreshes started, and refreshes that read the status and of them
    /// those off the main thread.
    pub(crate) fn runs(&self) -> (usize, (usize, usize)) {
        self.test.borrow().runs()
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
    }
}

#[cfg(test)]
#[path = "git_live_tests.rs"]
mod tests;
