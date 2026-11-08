//! A line's blame and its commit: `git.toggle_line_blame`,
//! `git.blame_line` and `git.open_line_commit`, routed here by
//! `git_commands`; and `git.toggle_blame_annotations`, every row's in the
//! gutter, in [`column`].
//!
//! Line blame (`:Gitsigns toggle_current_line_blame`) is off by default,
//! per window. On, the active editor's caret row shows, after its
//! text and in `Muted`, VS Code's `<summary>, <author> (<time ago>)`, or
//! `Not Committed Yet` for a row edited or added since HEAD. It follows the
//! caret however it moves (vim motions, search, clicks), in every mode,
//! once the caret has rested [`REST`], so a held `j` doesn't flicker; it
//! hides while the selection spans rows. Resting the pointer on it shows
//! the commit's popover.
//!
//! The blame is the buffer's text blamed by [`Repo::blame`] on the
//! background executor, kept per buffer for its file, version and HEAD: an
//! edit blames again [`EDIT_REST`] after the last one, and a git refresh
//! (the Changes panel's), or an editor becoming the active one, reads
//! HEAD's id and blames again if HEAD moved, or if the last blame found
//! nothing to show. Of two blames of a buffer landing out of order, the
//! later one asked for wins. Blame annotations follow their buffer the
//! same way, sharing its last blame, and neither blames a version the
//! other is blaming. A file outside a repository, untracked (in
//! neither HEAD nor the index) or over 5 MiB has none, and shows nothing.
//!
//! Each editor marked gets a provider of its own; a model split from it
//! (another tab on its buffer, a diff's working side) copies the
//! registration, which is removed from the window's other editors and
//! emptied once the editor is unmarked.
//!
//! `git.blame_line` (`:Gitsigns blame_line`) shows the caret row's commit
//! in a popover at the caret: `**<author>** <email>`, its date and how long
//! ago, its message and its short id. `git.open_line_commit`
//! (`:Gitsigns show_commit`) opens the change the caret row's commit made to
//! the file: a read-only diff view of the file at the commit's first parent
//! (`<short>^`, empty for a root commit) against the file at the commit
//! (`<short>`), the caret on the row there; or it shows the one open. The
//! commit, the row, and the file's paths at the commit and the parent are
//! `git blame --porcelain`'s for HEAD's row that the buffer's row is (as
//! [`Repo::blame`] matches them), which follows the row through renames
//! and duplicate rows. On a row not committed yet it opens
//! the file against HEAD, as `git.diff_head` does. Both commands read
//! afresh, and what they find lands only if it's the last asked for, the
//! pane and the editor are still the active ones, nothing was navigated to
//! there, and the editor's file, text and caret row are as they were.

use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_buffer::{Buffer, BufferId, BufferSnapshot, Version};
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, ProviderId, Tint, overlaps},
};
use fe_git::{BlameRun, CommitInfo, Repo};
use fe_gpui::{DiffSide, DiffView, EditorView, HoverContent};
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, Subscription, Task, WeakEntity, Window,
};
use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};
use kiln_shell::PaneView as _;

use super::{
    Workbench,
    git_commands::{GitCommand, GitOp, Owner},
    git_diff,
};
use crate::{Pane, git};

/// How long the caret rests on a row before its blame shows.
pub(crate) const REST: Duration = Duration::from_millis(150);

/// How long after the last edit the buffer is blamed again.
pub(crate) const EDIT_REST: Duration = Duration::from_millis(300);

/// What a row edited or added since HEAD shows.
const NOT_COMMITTED: &str = "Not Committed Yet";

/// A buffer over this many bytes isn't blamed.
const MAX_BYTES: usize = 5 * 1024 * 1024;

/// What blame answers outside a repository.
const NO_REPO: &str = "Not in a git repository";

/// What a command for the active file answers when it has none.
const NO_FILE: &str = "E32: No file name";

/// What blame answers for a file git doesn't track.
const UNTRACKED: &str = "Not tracked by git";

/// What blame answers for a buffer over [`MAX_BYTES`].
const TOO_LARGE: &str = "Too large to blame";

/// A buffer's rows' blame, or why it has none.
type Found = Result<Arc<[BlameRun]>, String>;

/// A buffer's last blame: of which file and version, at which HEAD, and
/// the number of the blame that read it, which orders blames of one
/// version.
struct Blamed {
    serial: u64,
    path: PathBuf,
    version: Version,
    head: Option<String>,
    found: Found,
}

/// The caret as line blame last looked at it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Seen {
    editor: EntityId,
    buffer: BufferId,
    path: PathBuf,
    version: Version,
    row: u32,
    /// Whether the selection spans rows, which hides the blame.
    spans_rows: bool,
}

/// The decoration showing the caret row's blame: the buffer version and
/// row it's for, and its text. Nothing in any other version.
#[derive(Default)]
struct Annotation(Option<(Version, u32, Arc<str>)>);

impl DecorationProvider for Annotation {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let Some((version, row, label)) = &self.0 else {
            return;
        };
        let text = snapshot.text();
        if snapshot.version() != *version || *row >= text.row_count() {
            return;
        }
        let start = text.row_start(*row);
        let item = start..start + text.row_len(*row) as usize;
        if overlaps(&item, &range) {
            let kind = DecorationKind::AfterLine(label.clone());
            out.push(Decoration::new(item, kind, Tint::Muted));
        }
    }
}

/// The editor showing a window's annotation: its own provider, registered
/// on its model as `provider`. A model split from it (a tab or a diff's
/// working side opened on its buffer) takes a copy of the registration,
/// which [`look`] removes from the window's editors; the provider is
/// emptied for good once the editor is unmarked, so a copy elsewhere shows
/// nothing.
#[derive(Clone)]
struct Marked {
    editor: Entity<EditorView>,
    provider: ProviderId,
    annotation: Rc<RefCell<Annotation>>,
}

/// A window's line blame, while it's on.
struct LineBlame {
    marked: Option<Marked>,
    seen: Option<Seen>,
    /// Waits out the caret's rest, then shows its row's blame.
    rest: Option<Task<()>>,
    /// The blame waiting out an edit's rest, or running: of which buffer,
    /// file and version, as it was asked for. Marking another editor
    /// cancels it.
    blaming: Option<(BufferId, PathBuf, Version, Task<()>)>,
    /// Reads HEAD's id after a git refresh, or as an editor is marked.
    head_check: Option<Task<()>>,
    /// Whether HEAD is to be read again once the blame pending lands.
    recheck: bool,
    /// Observes the marked editor.
    _marked: Option<Subscription>,
    /// Observes the workbench, its status bar (which follows the active
    /// editor) and the Changes panel (whose refreshes may move HEAD or
    /// change the index).
    _subscriptions: Vec<Subscription>,
}

/// How often line blame did its work, for the tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Buffers blamed.
    pub(crate) blames: usize,
    /// Annotations shown.
    pub(crate) shown: usize,
}

/// Each window's line blame, each buffer's last blame, and the diff views
/// of commits' changes opened: of which file and commit.
#[derive(Default)]
struct BlameState {
    windows: HashMap<EntityId, LineBlame>,
    blamed: HashMap<BufferId, Blamed>,
    commits: Vec<(WeakEntity<DiffView>, PathBuf, String)>,
    /// The editors annotated, by id.
    columns: HashMap<EntityId, column::Column>,
    /// Bumped by each blame started.
    serials: u64,
    /// Bumped by each `git.open_line_commit` and `git.blame_line`: only
    /// the last one asked for lands.
    asks: u64,
    #[cfg(test)]
    counts: Counts,
    /// The next blame waits for this once read, for the tests.
    #[cfg(test)]
    hold: Option<futures::channel::oneshot::Receiver<()>>,
}

impl Global for BlameState {}

fn state(cx: &mut App) -> &mut BlameState {
    cx.default_global::<BlameState>()
}

/// The line blame of the window `workbench` is the root of, if it's on.
fn line(workbench: EntityId, cx: &mut App) -> Option<&mut LineBlame> {
    state(cx).windows.get_mut(&workbench)
}

/// The last blame of `seen`'s buffer, if it's of its file and version.
fn blamed<'a>(seen: &Seen, cx: &'a mut App) -> Option<&'a Blamed> {
    (state(cx).blamed.get(&seen.buffer))
        .filter(|blamed| blamed.version == seen.version && blamed.path == seen.path)
}

/// Starts a blame: its number.
fn next_serial(cx: &mut App) -> u64 {
    let state = state(cx);
    state.serials += 1;
    state.serials
}

#[cfg(test)]
pub(crate) fn counts(cx: &mut App) -> Counts {
    state(cx).counts
}

/// Runs `op`, a blame command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    match op.command {
        GitCommand::ToggleLineBlame => {
            let id = cx.entity_id();
            let on = !state(cx).windows.contains_key(&id);
            set_line_blame(workbench, on, cx);
        }
        GitCommand::ToggleBlameAnnotations => column::toggle(workbench, op, cx),
        GitCommand::BlameLine => blame_line(workbench, op, window, cx),
        GitCommand::OpenLineCommit => open_line_commit(workbench, op, None, window, cx),
        _ => {
            let text = format!("Not supported yet: {}", op.command.title());
            workbench.answer_git_op(&op, text, cx);
        }
    }
}

/// Turns the window's line blame on or off: `git.toggle_line_blame`, and
/// `editor.inline_blame` for a new window and a changed setting.
pub(super) fn set_line_blame(workbench: &mut Workbench, on: bool, cx: &mut Context<Workbench>) {
    let id = cx.entity_id();
    if !on {
        if let Some(line) = state(cx).windows.remove(&id) {
            unmark(line.marked, cx);
        }
        return;
    }
    if state(cx).windows.contains_key(&id) {
        return;
    }
    let status_bar = workbench.status_bar.clone();
    let changes = workbench.dock.read(cx).changes().clone();
    let subscriptions = vec![
        cx.observe_self(look),
        cx.observe(&status_bar, |workbench, _, cx| look(workbench, cx)),
        cx.observe(&changes, |workbench, _, cx| check_head(workbench, cx)),
        cx.on_release(move |_, cx| {
            if let Some(line) = state(cx).windows.remove(&id) {
                unmark(line.marked, cx);
            }
        }),
    ];
    state(cx).windows.insert(
        id,
        LineBlame {
            marked: None,
            seen: None,
            rest: None,
            blaming: None,
            head_check: None,
            recheck: false,
            _marked: None,
            _subscriptions: subscriptions,
        },
    );
    look(workbench, cx);
}

/// Takes the annotation off `marked`'s editor, and empties its provider
/// for any copy of it elsewhere.
fn unmark(marked: Option<Marked>, cx: &mut App) {
    if let Some(marked) = marked {
        marked.annotation.borrow_mut().0 = None;
        marked.editor.update(cx, |editor, cx| {
            editor
                .model_mut()
                .remove_decoration_provider(marked.provider);
            editor.set_after_line_popover(None, cx);
            cx.notify();
        });
    }
}

/// The caret of `editor`, if it shows a file.
fn seen(editor: &Entity<EditorView>, cx: &App) -> Option<Seen> {
    let model = editor.read(cx).model();
    let path = model.path()?.to_owned();
    let text = model.text();
    let selection = model.primary_selection();
    let row = text.offset_to_point(selection.head()).row;
    let rows = [selection.range.start, selection.range.end].map(|at| text.offset_to_point(at).row);
    Some(Seen {
        editor: editor.entity_id(),
        buffer: model.buffer().id(),
        path,
        version: model.snapshot().version(),
        row,
        spans_rows: rows[0] != rows[1],
    })
}

/// Marks `editor`, if any, in place of the editor marked: see [`Marked`].
fn mark(id: EntityId, editor: Option<&Entity<EditorView>>, cx: &mut Context<Workbench>) {
    let Some(line) = line(id, cx) else {
        return;
    };
    let old = line.marked.take();
    line.seen = None;
    line._marked = None;
    line.head_check = None;
    line.blaming = None;
    line.recheck = false;
    unmark(old, cx);
    let Some(editor) = editor else {
        return;
    };
    let annotation = Rc::new(RefCell::new(Annotation::default()));
    let registered = annotation.clone();
    let provider = editor.update(cx, |editor, _| {
        editor.model_mut().add_decoration_provider(registered)
    });
    let observed = cx.observe(editor, |workbench, _, cx| look(workbench, cx));
    if let Some(line) = self::line(id, cx) {
        line.marked = Some(Marked {
            editor: editor.clone(),
            provider,
            annotation,
        });
        line._marked = Some(observed);
    }
}

/// Removes the marked editor's registration from the window's other
/// editors of its buffer, which a split copied it to.
fn sweep(workbench: &Workbench, id: EntityId, cx: &mut App) {
    let Some(marked) = line(id, cx).and_then(|line| line.marked.clone()) else {
        return;
    };
    let copies: Vec<Entity<EditorView>> = (workbench.file_editors(cx).into_iter())
        .filter(|editor| *editor != marked.editor)
        .filter(|editor| {
            let model = editor.read(cx).model();
            model.shares_buffer_with(marked.editor.read(cx).model())
        })
        .collect();
    for editor in copies {
        editor.update(cx, |editor, cx| {
            if editor
                .model_mut()
                .remove_decoration_provider(marked.provider)
            {
                cx.notify();
            }
        });
    }
}

/// Looks at the active editor's caret: on a new row, or a new editor, the
/// annotation hides until the caret has rested; on a new version of the
/// buffer, it's blamed again, after an edit once the edits have rested. A
/// newly marked editor's buffer blamed before has HEAD read again.
fn look(workbench: &mut Workbench, cx: &mut Context<Workbench>) {
    let id = cx.entity_id();
    let active = workbench.active_editor(cx).cloned();
    let Some(line) = line(id, cx) else {
        return;
    };
    let marked = line.marked.as_ref().map(|marked| marked.editor.clone());
    let switched = marked != active;
    if switched {
        mark(id, active.as_ref(), cx);
    }
    sweep(workbench, id, cx);
    let seen = active.as_ref().and_then(|editor| seen(editor, cx));
    let Some(line) = self::line(id, cx) else {
        return;
    };
    if line.seen == seen {
        return;
    }
    let before = std::mem::replace(&mut line.seen, seen.clone());
    hide(id, cx);
    let (Some(seen), Some(editor)) = (seen, active) else {
        return;
    };
    let rest = cx.spawn(async move |workbench, cx| {
        cx.background_executor().timer(REST).await;
        (workbench.update(cx, |workbench, cx| {
            if let Some(line) = self::line(cx.entity_id(), cx) {
                line.rest = None;
            }
            show(workbench, cx);
        }))
        .ok();
    });
    let Some(line) = self::line(id, cx) else {
        return;
    };
    line.rest = Some(rest);
    let pending = (line.blaming.as_ref()).is_some_and(|(buffer, path, version, _)| {
        (*buffer, path, *version) == (seen.buffer, &seen.path, seen.version)
    });
    if pending {
        return;
    }
    // Work for another file or version is of no use now.
    let mut rechecking = false;
    if let Some(line) = self::line(id, cx) {
        // A HEAD read it deferred has no blame to wait for now.
        rechecking = line.blaming.take().is_some() && std::mem::take(&mut line.recheck);
    }
    if blamed(&seen, cx).is_some() {
        if switched || rechecking {
            check_head(workbench, cx);
        }
        return;
    }
    // Annotations blaming it land it here too.
    if column::blaming(seen.buffer, &seen.path, seen.version, cx) {
        return;
    }
    let edited = before.is_some_and(|before| {
        before.buffer == seen.buffer && before.path == seen.path && before.version != seen.version
    });
    let delay = if edited { EDIT_REST } else { Duration::ZERO };
    start_blame(id, &editor, delay, cx);
}

/// Whether a window's line blame is blaming `buffer`'s file `path` at
/// `version`.
fn line_blaming(buffer: BufferId, path: &Path, version: Version, cx: &mut App) -> bool {
    (state(cx).windows.values()).any(|line| {
        (line.blaming.as_ref()).is_some_and(|(blaming, at, asked, _)| {
            (*blaming, at.as_path(), *asked) == (buffer, path, version)
        })
    })
}

/// Hides the annotation, and its popover.
fn hide(id: EntityId, cx: &mut App) {
    let Some(marked) = line(id, cx).and_then(|line| line.marked.clone()) else {
        return;
    };
    if marked.annotation.borrow_mut().0.take().is_none() {
        return;
    }
    marked.editor.update(cx, |editor, cx| {
        editor.set_after_line_popover(None, cx);
        cx.notify();
    });
}

/// Shows the blame of the marked editor's caret row, if its buffer's
/// blame is in and the selection is within the row; else hides it.
fn show(_: &mut Workbench, cx: &mut Context<Workbench>) {
    let id = cx.entity_id();
    let Some(marked) = line(id, cx).and_then(|line| line.marked.clone()) else {
        return;
    };
    let shown = seen(&marked.editor, cx)
        .filter(|seen| !seen.spans_rows)
        .and_then(|seen| {
            let Ok(runs) = &blamed(&seen, cx)?.found else {
                return None;
            };
            let commit = row_commit(runs, seen.row)?;
            Some((seen, commit))
        });
    let Some((seen, commit)) = shown else {
        return hide(id, cx);
    };
    let label: Arc<str> = label(commit.as_deref(), now()).into();
    let content = popover(commit.as_deref(), now());
    marked.annotation.borrow_mut().0 = Some((seen.version, seen.row, label.clone()));
    #[cfg(test)]
    {
        state(cx).counts.shown += 1;
    }
    marked.editor.update(cx, |editor, cx| {
        editor.set_after_line_popover(Some((label, content)), cx);
        cx.notify();
    });
}

/// The commit of `row`, `None` within if it's not committed yet; `None`
/// for a row in no run.
fn row_commit(runs: &[BlameRun], row: u32) -> Option<Option<Arc<CommitInfo>>> {
    let run = runs.iter().find(|run| run.rows.contains(&row))?;
    Some(run.commit.clone())
}

/// Blames `editor`'s buffer once `delay` is up, as it is then.
fn start_blame(
    id: EntityId,
    editor: &Entity<EditorView>,
    delay: Duration,
    cx: &mut Context<Workbench>,
) {
    let model = editor.read(cx).model();
    let Some(path) = model.path().map(Path::to_owned) else {
        return;
    };
    let (buffer, version) = (model.buffer().id(), model.buffer().version());
    let editor = editor.downgrade();
    let serial = next_serial(cx);
    let pending = path.clone();
    let task = cx.spawn(async move |workbench, cx| {
        if !delay.is_zero() {
            cx.background_executor().timer(delay).await;
        }
        let Ok((text, version)) = editor.read_with(cx, |editor, _| {
            let buffer = editor.model().buffer();
            (buffer.text().to_string(), buffer.version())
        }) else {
            return;
        };
        let at = path.clone();
        #[cfg(test)]
        let hold = cx.update(|cx| state(cx).hold.take());
        let (head, found) = (cx.background_spawn(async move {
            let blamed = blame(&at, &text);
            #[cfg(test)]
            if let Some(hold) = hold {
                hold.await.ok();
            }
            blamed
        }))
        .await;
        (workbench.update(cx, |workbench, cx| {
            if let Some(line) = line(cx.entity_id(), cx) {
                line.blaming = None;
            }
            let blamed = Blamed {
                serial,
                path,
                version,
                head,
                found,
            };
            landed(workbench, buffer, blamed, cx);
            let recheck =
                line(cx.entity_id(), cx).is_some_and(|line| std::mem::take(&mut line.recheck));
            if recheck {
                check_head(workbench, cx);
            }
        }))
        .ok();
    });
    if let Some(line) = line(id, cx) {
        line.blaming = Some((buffer, pending, version, task));
    }
}

/// Keeps `blamed`, `buffer`'s, unless a blame of a later version, or a
/// later blame of the same version, landed first; then shows it if the
/// caret has rested, and in the buffer's annotations.
fn landed(
    workbench: &mut Workbench,
    buffer: BufferId,
    blamed: Blamed,
    cx: &mut Context<Workbench>,
) {
    let state = state(cx);
    #[cfg(test)]
    {
        state.counts.blames += 1;
    }
    let superseded = (state.blamed.get(&buffer)).is_some_and(|kept| {
        kept.version > blamed.version
            || (kept.version == blamed.version && kept.serial > blamed.serial)
    });
    if superseded {
        return;
    }
    // Only the open buffers' last blames are worth keeping: a few.
    if state.blamed.len() > 32 {
        state.blamed.clear();
    }
    state.blamed.insert(buffer, blamed);
    let resting = line(cx.entity_id(), cx).is_some_and(|line| line.rest.is_some());
    if !resting {
        show(workbench, cx);
    }
    column::landed(workbench, buffer, cx);
}

/// After a git refresh, or as an editor is marked: reads HEAD's id for the
/// marked editor's file, and blames its buffer again if HEAD moved since
/// its last blame, or if that found nothing to show (the file may be
/// tracked by now). The read lands only if the editor is still marked, its
/// buffer unchanged and no blame is pending.
fn check_head(_: &mut Workbench, cx: &mut Context<Workbench>) {
    let id = cx.entity_id();
    let Some(marked) = line(id, cx).and_then(|line| line.marked.clone()) else {
        return;
    };
    let Some(seen) = seen(&marked.editor, cx) else {
        return;
    };
    // Read once the blame pending has landed.
    if let Some(line) = line(id, cx).filter(|line| line.blaming.is_some()) {
        line.recheck = true;
        return;
    }
    let Some((head, found_none)) =
        blamed(&seen, cx).map(|blamed| (blamed.head.clone(), blamed.found.is_err()))
    else {
        return;
    };
    let path = seen.path.clone();
    let read = cx.background_spawn(async move { repo(&path).ok()?.0.head_id() });
    let editor = marked.editor.clone();
    let task = cx.spawn(async move |workbench, cx| {
        let now = read.await;
        if now == head && !found_none {
            return;
        }
        (workbench.update(cx, |_, cx| {
            let id = cx.entity_id();
            let Some(line) = line(id, cx) else {
                return;
            };
            line.head_check = None;
            let still = (line.marked.as_ref()).is_some_and(|marked| marked.editor == editor);
            if still && line.blaming.is_some() {
                line.recheck = true;
            } else if still
                && self::seen(&editor, cx).is_some_and(|now| now.version == seen.version)
                && !column::blaming(seen.buffer, &seen.path, seen.version, cx)
            {
                start_blame(id, &editor, Duration::ZERO, cx);
            }
        }))
        .ok();
    });
    if let Some(line) = line(id, cx) {
        line.head_check = Some(task);
    }
}

/// The repository `path` is in, found from the nearest folder of it that
/// exists, and `path` through symlinks. Runs on the background executor.
fn repo(path: &Path) -> Result<(Repo, PathBuf), String> {
    let real = git::resolve(path);
    let Some(folder) = real.ancestors().skip(1).find(|folder| folder.is_dir()) else {
        return Err(NO_REPO.to_owned());
    };
    match Repo::discover(folder) {
        Ok(Some(repo)) => Ok((repo, real)),
        Ok(None) => Err(NO_REPO.to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// HEAD's id, and the blame of `text`, `path`'s buffer, or why it has
/// none. Runs on the background executor.
fn blame(path: &Path, text: &str) -> (Option<String>, Found) {
    let (repo, real) = match repo(path) {
        Ok(found) => found,
        Err(answer) => return (None, Err(answer)),
    };
    let head = repo.head_id();
    if text.len() > MAX_BYTES {
        return (head, Err(TOO_LARGE.to_owned()));
    }
    let known = |size: Result<Option<u64>, fe_git::GitError>| matches!(size, Ok(Some(_)));
    if !known(repo.head_blob_size(&real)) && !known(repo.index_blob_size(&real)) {
        return (head, Err(UNTRACKED.to_owned()));
    }
    let found = (repo.blame(&real, text))
        .map(Arc::from)
        .map_err(|error| error.to_string());
    (head, found)
}

#[cfg(test)]
thread_local! {
    /// What [`now`] says while a test has pinned it.
    static PINNED: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

/// Has [`now`] say `at`, or the clock's time again.
#[cfg(test)]
pub(crate) fn pin_now(at: Option<i64>) {
    PINNED.with(|pinned| pinned.set(at));
}

/// Seconds since the epoch.
fn now() -> i64 {
    #[cfg(test)]
    if let Some(at) = PINNED.with(|pinned| pinned.get()) {
        return at;
    }
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs() as i64)
}

/// How long ago `seconds` was, as the branch picker says it: `just now`,
/// `5 minutes ago`, `1 day ago`.
fn ago(seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let seconds = seconds.max(0);
    let (count, unit) = if seconds < MINUTE {
        return "just now".to_owned();
    } else if seconds < HOUR {
        (seconds / MINUTE, "minute")
    } else if seconds < DAY {
        (seconds / HOUR, "hour")
    } else if seconds < 14 * DAY {
        (seconds / DAY, "day")
    } else if seconds < 60 * DAY {
        (seconds / (7 * DAY), "week")
    } else if seconds < 365 * DAY {
        (seconds / (30 * DAY), "month")
    } else {
        (seconds / (365 * DAY), "year")
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// `commit`'s author time in its own offset: `2026-10-14 09:30 +0200`.
pub(super) fn date(commit: &CommitInfo) -> String {
    let offset = i64::from(commit.offset_minutes);
    let local = commit.time + offset * 60;
    let (days, seconds) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let sign = if offset < 0 { '-' } else { '+' };
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} {sign}{:02}{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        offset.abs() / 60,
        offset.abs() % 60,
    )
}

/// What the annotation says of a row last changed by `commit`, or not
/// committed yet, at `now`.
fn label(commit: Option<&CommitInfo>, now: i64) -> String {
    match commit {
        Some(commit) => format!(
            "{}, {} ({})",
            commit.summary,
            commit.author,
            ago(now - commit.time)
        ),
        None => NOT_COMMITTED.to_owned(),
    }
}

/// What the popover says of a row last changed by `commit`, or not
/// committed yet, at `now`.
fn popover(commit: Option<&CommitInfo>, now: i64) -> HoverContent {
    let text = match commit {
        Some(commit) => format!(
            "**{}** {}\n{} ({})\n\n{}\n\n{}",
            commit.author,
            commit.email,
            date(commit),
            ago(now - commit.time),
            commit.message.trim_end(),
            commit.short,
        ),
        None => NOT_COMMITTED.to_owned(),
    };
    HoverContent { text, range: None }
}

/// What `git.blame_line` or `git.open_line_commit` was asked about: the
/// editor, its file, its buffer's version and text, and its caret's row,
/// or the row of an annotation clicked;
/// the active pane and its navigations and opens then; and the ask's
/// number, as only the last ask lands.
struct Ask {
    editor: Entity<EditorView>,
    path: PathBuf,
    version: Version,
    text: String,
    row: u32,
    /// Whether `row` is the caret's, which is then to stay on it.
    caret: bool,
    pane: (Entity<Pane>, u64, u64),
    serial: u64,
    /// The workbench's pane activations then: a round trip through another
    /// pane leaves the pane's counters as they were.
    activations: u64,
}

/// The active pane, and its navigations and opens so far.
fn pane_now(workbench: &Workbench, cx: &App) -> (Entity<Pane>, u64, u64) {
    let pane = workbench.active.clone();
    let (navigations, opens) = {
        let pane = pane.read(cx);
        (pane.navigations(), pane.open_serial())
    };
    (pane, navigations, opens)
}

/// What `op` asks about, of `row` or the caret's row; or the answer.
fn ask(
    workbench: &Workbench,
    op: &GitOp,
    row: Option<u32>,
    cx: &mut App,
) -> Result<Ask, &'static str> {
    let editor = op.editor.clone().ok_or(NO_FILE)?;
    let model = editor.read(cx).model();
    let path = model.path().ok_or(NO_FILE)?.to_owned();
    let caret = (model.text())
        .offset_to_point(model.primary_selection().head())
        .row;
    let (version, text) = {
        let buffer = model.buffer();
        (buffer.version(), buffer.text().to_string())
    };
    let pane = pane_now(workbench, cx);
    let state = state(cx);
    state.asks += 1;
    Ok(Ask {
        editor,
        path,
        version,
        text,
        row: row.unwrap_or(caret),
        caret: row.is_none(),
        pane,
        serial: state.asks,
        activations: workbench.activations,
    })
}

/// Whether what `ask` asked about stands: it's the last ask; its pane is
/// still the active one, nothing navigated to or opened there since; and
/// its editor is still the active one, with the same file and buffer
/// version, and the caret on the row asked about if it was then. A result
/// for one that doesn't is dropped.
fn stands(ask: &Ask, workbench: &Workbench, cx: &mut App) -> bool {
    if state(cx).asks != ask.serial
        || workbench.activations != ask.activations
        || pane_now(workbench, cx) != ask.pane
        || workbench.active_editor(cx) != Some(&ask.editor)
    {
        return false;
    }
    let model = ask.editor.read(cx).model();
    let row = (model.text())
        .offset_to_point(model.primary_selection().head())
        .row;
    model.path() == Some(ask.path.as_path())
        && model.buffer().version() == ask.version
        && (!ask.caret || row == ask.row)
}

/// Has `then` take the blame of `ask`'s buffer, blamed now in the
/// background (a blame kept may be of an older HEAD), and keeps it.
fn with_blame(
    ask: Ask,
    window: &mut Window,
    cx: &mut Context<Workbench>,
    then: impl FnOnce(&mut Workbench, Ask, &Found, &mut Window, &mut Context<Workbench>) + 'static,
) {
    let buffer = ask.editor.read(cx).model().buffer().id();
    let serial = next_serial(cx);
    let (path, text) = (ask.path.clone(), ask.text.clone());
    let blamed = cx.background_spawn(async move { blame(&path, &text) });
    cx.spawn_in(window, async move |workbench, cx| {
        let (head, found) = blamed.await;
        (workbench.update_in(cx, |workbench, window, cx| {
            let blamed = Blamed {
                serial,
                path: ask.path.clone(),
                version: ask.version,
                head,
                found: found.clone(),
            };
            then(workbench, ask, &found, window, cx);
            landed(workbench, buffer, blamed, cx);
        }))
        .ok();
    })
    .detach();
}

/// `git.blame_line`: the caret row's commit, in a popover at the caret.
fn blame_line(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let ask = match ask(workbench, &op, None, cx) {
        Ok(ask) => ask,
        Err(answer) => return workbench.answer_git_op(&op, answer.to_owned(), cx),
    };
    let then = move |workbench: &mut Workbench,
                     ask: Ask,
                     found: &Found,
                     _: &mut Window,
                     cx: &mut Context<Workbench>| {
        if !stands(&ask, workbench, cx) {
            return;
        }
        let runs = match found {
            Ok(runs) => runs,
            Err(answer) => return workbench.answer_git_op(&op, answer.clone(), cx),
        };
        let Some(commit) = row_commit(runs, ask.row) else {
            return;
        };
        let content = popover(commit.as_deref(), now());
        (ask.editor).update(cx, |editor, cx| editor.show_popover(content, cx));
    };
    with_blame(ask, window, cx, then);
}

/// `git.open_line_commit`: the change the commit of `row`, or of the
/// caret's row, made to the file, or the file against HEAD for a row not
/// committed yet.
fn open_line_commit(
    workbench: &mut Workbench,
    op: GitOp,
    row: Option<u32>,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let ask = match ask(workbench, &op, row, cx) {
        Ok(ask) => ask,
        Err(answer) => return workbench.answer_git_op(&op, answer.to_owned(), cx),
    };
    let (path, text, row) = (ask.path.clone(), ask.text.clone(), ask.row);
    let read = cx.background_spawn(async move { read_origin(&path, &text, row) });
    cx.spawn_in(window, async move |workbench, cx| {
        let read = read.await;
        (workbench.update_in(cx, |workbench, window, cx| {
            if !stands(&ask, workbench, cx) {
                return;
            }
            match read {
                Ok(Some((origin, texts))) => {
                    open_commit(workbench, &ask.path, origin, texts, window, cx)
                }
                Ok(None) => {
                    let op = GitOp {
                        command: GitCommand::DiffHead,
                        ..op
                    };
                    // The ask holds through the diff's read.
                    let guard = Rc::new(move |workbench: &Workbench, cx: &mut App| {
                        stands(&ask, workbench, cx)
                    });
                    git_diff::diff_head_guarded(workbench, op, guard, window, cx);
                }
                Err(answer) => workbench.answer_git_op(&op, answer, cx),
            }
        }))
        .ok();
    })
    .detach();
}

/// A diff's left and right texts.
type Texts = (String, String);

/// Where a row came from, as `git blame` traces it: the commit that last
/// changed it, the row and the file's path there, and, unless the commit
/// added the file, the parent the row came through and the file's path
/// there.
struct Origin {
    id: String,
    row: u32,
    path: PathBuf,
    previous: Option<(String, PathBuf)>,
}

/// The origin of row `row` of `text`, `path`'s buffer, and the file's
/// texts at the origin's parent (empty if the commit added it) and at the
/// commit; `None` for a row not committed yet, or a file not in HEAD; or
/// the answer. The row is matched to HEAD's as [`Repo::blame`] matches
/// them ([`head_row`]), and HEAD's row is traced. Runs on the background
/// executor.
fn read_origin(path: &Path, text: &str, row: u32) -> Result<Option<(Origin, Texts)>, String> {
    let (repo, real) = repo(path)?;
    if text.len() > MAX_BYTES {
        return Err(TOO_LARGE.to_owned());
    }
    let error = |error: fe_git::GitError| error.to_string();
    let Some(head) = repo.head_text(&real).map_err(error)? else {
        let added = matches!(repo.index_blob_size(&real), Ok(Some(_)));
        return if added {
            Ok(None)
        } else {
            Err(UNTRACKED.to_owned())
        };
    };
    let Some(row) = head_row(&head, text, row) else {
        return Ok(None);
    };
    let Some(origin) = blame_row(repo.workdir(), &real, row)? else {
        return Ok(None);
    };
    let at = |commit: &str, path: &Path| {
        let text = repo.text_at(commit, path).map_err(error)?;
        Ok::<_, String>(text.unwrap_or_default())
    };
    let right = at(&origin.id, &origin.path)?;
    let left = match &origin.previous {
        Some((parent, path)) => at(parent, path)?,
        None => String::new(),
    };
    Ok(Some((origin, (left, right))))
}

/// The row of `head`, HEAD's text, that row `row` of `text`, the buffer,
/// is, matched as [`Repo::blame`] matches them: imara-diff's Histogram,
/// lines post-processed, over [`rows`]; `None` for a row the buffer
/// changed or added, or for a text blame doesn't blame: over 5 MiB, or
/// binary (a NUL in its first 8000 bytes, as git looks).
fn head_row(head: &str, text: &str, row: u32) -> Option<u32> {
    let blamed = |text: &str| {
        let probe = &text.as_bytes()[..text.len().min(8000)];
        text.len() <= MAX_BYTES && !probe.contains(&0)
    };
    if !blamed(head) || !blamed(text) {
        return None;
    }
    let (head, text) = (rows(head.as_bytes()), rows(text.as_bytes()));
    let head_rows = head.0.len() as u32;
    if row >= text.0.len() as u32 {
        return None;
    }
    let input = InternedInput::new(head, text);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let mut shift = 0i64;
    for hunk in diff.hunks() {
        if row < hunk.after.start {
            break;
        }
        if row < hunk.after.end {
            return None;
        }
        let (before, after) = (
            hunk.before.end - hunk.before.start,
            hunk.after.end - hunk.after.start,
        );
        shift += i64::from(before) - i64::from(after);
    }
    let row = u32::try_from(i64::from(row) + shift).ok()?;
    (row < head_rows).then_some(row)
}

/// A text's rows as [`Repo::blame`] splits them: each ends at `\n`, the
/// empty one after a final `\n` doesn't count, and each is compared
/// without one `\r` at its end.
struct Rows<'a>(Vec<&'a [u8]>);

impl<'a> TokenSource for Rows<'a> {
    type Token = &'a [u8];
    type Tokenizer = std::vec::IntoIter<&'a [u8]>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.0.clone().into_iter()
    }

    fn estimate_tokens(&self) -> u32 {
        self.0.len() as u32
    }
}

fn rows(data: &[u8]) -> Rows<'_> {
    if data.is_empty() {
        return Rows(Vec::new());
    }
    let data = data.strip_suffix(b"\n").unwrap_or(data);
    Rows(
        (data.split(|&byte| byte == b'\n'))
            .map(|row| row.strip_suffix(b"\r").unwrap_or(row))
            .collect(),
    )
}

/// The origin of row `row` of `real`'s blob in HEAD, in `workdir`, from
/// `git blame --porcelain`, which follows the row through renames and
/// rows like it. Blocks until `git` exits.
fn blame_row(workdir: &Path, real: &Path, row: u32) -> Result<Option<Origin>, String> {
    let relative = real.strip_prefix(workdir).unwrap_or(real);
    let lines = format!("{0},{0}", row + 1);
    let output = crate::process_group::no_window(&mut std::process::Command::new("git"))
        .current_dir(workdir)
        .args(["blame", "--porcelain", "-L", &lines, "HEAD", "--"])
        .arg(relative)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let mut lines = output.stdout.split(|&byte| byte == b'\n');
    let first = String::from_utf8_lossy(lines.next().unwrap_or_default()).into_owned();
    let mut first = first.split(' ');
    let (Some(id), Some(row)) = (
        first.next(),
        first.next().and_then(|row| row.parse::<u32>().ok()),
    ) else {
        let out = String::from_utf8_lossy(&output.stdout);
        return Err(format!("git blame said: {out}"));
    };
    let (mut path, mut previous) = (real.to_owned(), None);
    // The commit's headers, up to the row's text.
    for line in lines.take_while(|line| !line.starts_with(b"\t")) {
        if let Some(name) = line.strip_prefix(b"filename ") {
            path = workdir.join(unquote(name));
        } else if let Some(rest) = line.strip_prefix(b"previous ")
            && let Some(at) = rest.iter().position(|&byte| byte == b' ')
        {
            let parent = String::from_utf8_lossy(&rest[..at]).into_owned();
            previous = Some((parent, workdir.join(unquote(&rest[at + 1..]))));
        }
    }
    Ok(Some(Origin {
        id: id.to_owned(),
        row: row.saturating_sub(1),
        path,
        previous,
    }))
}

/// A path as git writes it: as it is, or, if it has bytes git quotes
/// (`core.quotePath`), in double quotes with C escapes: `\"`, `\\`, `\t`
/// and the like, and each other byte as three octal digits.
fn unquote(name: &[u8]) -> PathBuf {
    let quoted = (name.strip_prefix(b"\"")).and_then(|name| name.strip_suffix(b"\""));
    let bytes = match quoted {
        None => name.to_vec(),
        Some(inner) => {
            let mut bytes = Vec::with_capacity(inner.len());
            let mut rest = inner.iter().copied();
            while let Some(byte) = rest.next() {
                if byte != b'\\' {
                    bytes.push(byte);
                    continue;
                }
                let Some(escaped) = rest.next() else {
                    break;
                };
                bytes.push(match escaped {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 0x0b,
                    b'f' => 0x0c,
                    b'r' => b'\r',
                    b'0'..=b'7' => {
                        let digits = [Some(escaped), rest.next(), rest.next()];
                        (digits.into_iter().flatten())
                            .fold(0u8, |byte, digit| (byte << 3) | (digit - b'0'))
                    }
                    other => other,
                });
            }
            bytes
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// Opens `origin`'s change to the file at `path` in a diff view in the
/// active pane, its sides `texts`, or shows the one open: see the module
/// docs.
fn open_commit(
    workbench: &mut Workbench,
    path: &Path,
    origin: Origin,
    texts: Texts,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let row = side_row(&texts.1, origin.row);
    match commit_tab(workbench, path, &origin.id, cx) {
        Some((diff, pane, ix)) => show_tab(workbench, &diff, pane, ix, row, window, cx),
        None => show_commit(workbench, path, &origin, texts, row, window, cx),
    }
}

/// The row of a diff side over `text` that git's row `row` of it starts
/// on: git's rows end at `\n`; the side's, as every editor's, at `\r\n`, a
/// lone `\r` or `\n`.
fn side_row(text: &str, row: u32) -> u32 {
    let before: usize = (text.split_inclusive('\n').take(row as usize))
        .map(str::len)
        .sum();
    let bytes = &text.as_bytes()[..before];
    let breaks = (bytes.iter().enumerate()).filter(|&(at, &byte)| {
        byte == b'\n' || (byte == b'\r' && bytes.get(at + 1) != Some(&b'\n'))
    });
    breaks.count() as u32
}

/// A read-only side titled `title` over `text`, a file's contents.
fn fixed_side(title: String, text: &str) -> DiffSide {
    let buffer = Buffer::from_file_contents(text);
    DiffSide::new(title, EditorModel::new(buffer, None)).read_only()
}

/// This window's diff view of `commit`'s change to `path`, if one is open:
/// it, its pane and its tab's index there.
fn commit_tab(
    workbench: &Workbench,
    path: &Path,
    commit: &str,
    cx: &mut App,
) -> Option<(Entity<DiffView>, Entity<Pane>, usize)> {
    let commits = &mut state(cx).commits;
    commits.retain(|(diff, ..)| diff.upgrade().is_some());
    let views: Vec<Entity<DiffView>> = (commits.iter())
        .filter(|(_, open, id)| open == path && id == commit)
        .filter_map(|(diff, ..)| diff.upgrade())
        .collect();
    views.into_iter().find_map(|diff| {
        workbench.panes(cx).into_iter().find_map(|pane| {
            let tabs = pane.read(cx).tabs();
            let ix = (tabs.iter()).position(|tab| tab.item().downcast_ref() == Some(&diff))?;
            Some((diff.clone(), pane.clone(), ix))
        })
    })
}

/// Shows `diff`, tab `ix` of `pane`, focused, its right caret on `row`.
fn show_tab(
    workbench: &mut Workbench,
    diff: &Entity<DiffView>,
    pane: Entity<Pane>,
    ix: usize,
    row: u32,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    workbench.activate(pane.clone(), window, cx);
    pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
    let right = diff.read(cx).right().clone();
    right.update(cx, |view, cx| {
        let text = view.model().text();
        let at = text.row_start(row.min(text.row_count().saturating_sub(1)));
        view.select_and_reveal(at..at, cx);
    });
}

/// Shows `texts`, the file at `path` at `origin`'s parent and at its
/// commit, in a new tab of the active pane, focused, the right caret on
/// `row`.
fn show_commit(
    workbench: &mut Workbench,
    path: &Path,
    origin: &Origin,
    texts: Texts,
    row: u32,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let (left, right) = texts;
    let short = &origin.id[..origin.id.len().min(7)];
    let left = fixed_side(format!("{short}^"), &left);
    let right = fixed_side(short.to_owned(), &right);
    let name = (path.file_name()).map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let diff =
        cx.new(|cx| DiffView::new(name, left, right, cx).with_compared_path(path.to_owned()));
    let sides = {
        let view = diff.read(cx);
        [view.left().clone(), view.right().clone()]
    };
    for side in &sides {
        workbench.own_editor(side, Owner::DiffView(diff.downgrade()));
    }
    let entry = (diff.downgrade(), path.to_owned(), origin.id.clone());
    state(cx).commits.push(entry);
    let pane = workbench.active.clone();
    let ix = pane.update(cx, |pane, cx| {
        pane.pane_mut().push(Box::new(diff.clone()), cx);
        pane.items_changed(cx);
        cx.notify();
        pane.tabs().len() - 1
    });
    show_tab(workbench, &diff, pane, ix, row, window, cx);
}

#[path = "git_blame_column.rs"]
mod column;

#[cfg(test)]
#[path = "git_blame_tests.rs"]
mod git_blame_tests;
