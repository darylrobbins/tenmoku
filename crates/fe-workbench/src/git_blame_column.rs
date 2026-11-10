//! Blame annotations: `git.toggle_blame_annotations` (`:Git blame`, which
//! in fugitive opens a blame column) shows every row's commit in the
//! editor's gutter annotation column, left of the line numbers, as
//! JetBrains' "Annotate with Git Blame" does.
//!
//! - **Per editor.** It turns them on or off for the active editor only;
//!   another tab or split on its buffer isn't annotated. They go when the
//!   editor's tab closes or its path changes. A file with no blame (no
//!   file, outside a repository, untracked, too large) answers why, and
//!   shows nothing.
//! - **Entries.** One per run of rows with a commit, anchored over its
//!   rows: `<when> <author>`, `<when>` how long ago (`3 days ago`) for an
//!   author date less than 30 days before now and its `YYYY-MM-DD` after,
//!   the author cut to 20 characters. Rows not committed yet are blank.
//!   The tint is the commit's rank by author time among the file's, newest
//!   1.0, oldest 0.0. The text is made afresh as each blame lands, and as
//!   the editor becomes the active one again, so relative ages age then.
//! - **Pointer.** Resting on an entry shows the commit's popover, as
//!   `git.blame_line` does; clicking it opens the commit's change to the
//!   file, the caret on the row clicked, as `git.open_line_commit` does.
//! - **Following the file.** The buffer is blamed again as line blame's
//!   is: [`EDIT_REST`] after the last edit, and on a git refresh or as the
//!   editor becomes active if HEAD moved. Each blame lands in the buffer's
//!   last blame, which line blame shares, and replaces the annotations;
//!   rows edited since the last one are blank until the next lands.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_buffer::{BufferId, Version};
use fe_git::{BlameRun, CommitInfo};
use fe_gpui::{EditorView, GutterAnnotation, GutterAnnotations};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Subscription, Task, WeakEntity};
use kiln_l10n::tr;

use super::{
    Blamed, EDIT_REST, ago, blame, date, landed as blame_landed, line_blaming, next_serial,
    no_file, now, open_line_commit, popover, repo, state,
};
use crate::workbench::{
    Workbench,
    git_commands::{GitCommand, GitOp},
};
use crate::workbench_commands::provenance_column;

/// An author date less than this many seconds ago shows how long ago it
/// was; from then on, its date.
const RELATIVE: i64 = 30 * 86_400;

/// The most characters of an author shown.
const AUTHOR_CHARS: usize = 20;

/// An annotated editor.
pub(super) struct Column {
    editor: WeakEntity<EditorView>,
    workbench: WeakEntity<Workbench>,
    /// Its file when annotated: another path takes the annotations away.
    path: PathBuf,
    buffer: BufferId,
    /// The toggle that turned it on, answered if the first blame finds
    /// none; `None` once one is shown.
    asked: Option<GitOp>,
    /// The buffer last followed: an edit to it is blamed after [`EDIT_REST`],
    /// even while the first blame is still running.
    followed: Option<BufferId>,
    /// The buffer and version the annotations shown are for.
    shown: Option<(BufferId, Version)>,
    /// Whether it was the active editor when last looked at.
    active: bool,
    /// Whether it was one of the window's file editors then.
    listed: bool,
    /// The blame waiting out an edit's rest, or running, of the version
    /// asked for.
    blaming: Option<(Version, Task<()>)>,
    /// Reads HEAD's id after a git refresh, or as the editor becomes active.
    head_check: Option<Task<()>>,
    /// Whether HEAD is to be read again once the blame pending lands.
    recheck: bool,
    _subscriptions: Vec<Subscription>,
}

/// `git.toggle_blame_annotations`: annotates `op`'s editor, or takes its
/// annotations away.
pub(super) fn toggle(workbench: &mut Workbench, op: GitOp, cx: &mut Context<Workbench>) {
    let Some(editor) = op.editor.clone() else {
        return workbench.answer_git_op(&op, no_file().into(), cx);
    };
    let id = editor.entity_id();
    state(cx)
        .columns
        .retain(|_, column| column.editor.upgrade().is_some());
    if let Some(column) = state(cx).columns.remove(&id) {
        return unshow(column, cx);
    }
    let model = editor.read(cx).model();
    let Some(path) = model.path().map(Path::to_owned) else {
        return workbench.answer_git_op(&op, no_file().into(), cx);
    };
    let buffer = model.buffer().id();
    // One annotation column: provenance yields to blame, and blame to it.
    provenance_column::yield_to_blame(remove, cx);
    provenance_column::off(id, cx);
    let status_bar = workbench.status_bar.clone();
    let changes = workbench.dock.read(cx).changes().clone();
    let subscriptions = vec![
        cx.observe(&editor, move |workbench, _, cx| follow(workbench, id, cx)),
        cx.observe_release(&editor, move |_, _, cx| {
            state(cx).columns.remove(&id);
        }),
        // Closing the window before the first blame lands frees the editor.
        cx.on_release(move |_, cx| {
            state(cx).columns.remove(&id);
        }),
        cx.observe_self(move |workbench, cx| look(workbench, id, cx)),
        cx.observe(&status_bar, move |workbench, _, cx| look(workbench, id, cx)),
        cx.observe(&changes, move |workbench, _, cx| {
            check_head(workbench, id, cx)
        }),
    ];
    let listed = workbench.file_editors(cx).contains(&editor);
    let column = Column {
        editor: editor.downgrade(),
        workbench: cx.weak_entity(),
        path,
        buffer,
        asked: Some(op),
        followed: None,
        shown: None,
        active: true,
        listed,
        blaming: None,
        head_check: None,
        recheck: false,
        _subscriptions: subscriptions,
    };
    state(cx).columns.insert(id, column);
    follow(workbench, id, cx);
    // A blame kept may be of an older HEAD.
    if (state(cx).columns.get(&id)).is_some_and(|column| column.shown.is_some()) {
        check_head(workbench, id, cx);
    }
}

/// Whether an annotated editor is blaming `buffer`'s file `path` at
/// `version`.
pub(super) fn blaming(buffer: BufferId, path: &Path, version: Version, cx: &mut App) -> bool {
    (state(cx).columns.values()).any(|column| {
        column.buffer == buffer
            && column.path == path
            && (column.blaming.as_ref()).is_some_and(|(asked, _)| *asked == version)
    })
}

/// Shows `buffer`'s blame just kept in its annotated editors at its
/// version.
pub(super) fn landed(workbench: &Workbench, buffer: BufferId, cx: &mut App) {
    let columns = &mut state(cx).columns;
    columns.retain(|_, column| column.editor.upgrade().is_some());
    let ids: Vec<EntityId> = (columns.iter())
        .filter(|(_, column)| column.buffer == buffer)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        apply(workbench, id, cx);
    }
}

/// Takes `column`'s annotations off its editor.
fn unshow(column: Column, cx: &mut App) {
    if let Some(editor) = column.editor.upgrade() {
        editor.update(cx, |editor, cx| editor.set_gutter_annotations(None, cx));
    }
}

/// Takes editor `id`'s annotations away.
fn remove(id: EntityId, cx: &mut App) {
    if let Some(column) = state(cx).columns.remove(&id) {
        unshow(column, cx);
    }
}

/// As the window changes: drops editor `id`'s annotations once its tab has
/// closed; as it becomes the active editor, makes its entries' text afresh
/// and reads HEAD.
fn look(workbench: &mut Workbench, id: EntityId, cx: &mut Context<Workbench>) {
    let Some(editor) = (state(cx).columns.get(&id)).and_then(|column| column.editor.upgrade())
    else {
        return;
    };
    let listed = workbench.file_editors(cx).contains(&editor);
    let active = workbench.active_editor(cx) == Some(&editor);
    let Some(column) = state(cx).columns.get_mut(&id) else {
        return;
    };
    if column.listed && !listed {
        return remove(id, cx);
    }
    column.listed |= listed;
    let became = active && !column.active;
    column.active = active;
    if became {
        apply(workbench, id, cx);
    }
    follow(workbench, id, cx);
    if became {
        check_head(workbench, id, cx);
    }
}

/// As editor `id` changes: drops its annotations if its path changed;
/// shows its buffer's blame if it's of the version it has now, or blames
/// that version, once its edits have rested.
fn follow(workbench: &mut Workbench, id: EntityId, cx: &mut Context<Workbench>) {
    let Some(editor) = (state(cx).columns.get(&id)).and_then(|column| column.editor.upgrade())
    else {
        return;
    };
    let model = editor.read(cx).model();
    let path = model.path().map(Path::to_owned);
    let (buffer, version) = (model.buffer().id(), model.buffer().version());
    let Some(column) = state(cx).columns.get_mut(&id) else {
        return;
    };
    if path.as_ref() != Some(&column.path) {
        return remove(id, cx);
    }
    column.buffer = buffer;
    let pending = (column.blaming.as_ref()).is_some_and(|(asked, _)| *asked == version);
    if column.shown == Some((buffer, version)) || pending {
        return;
    }
    // A blame of another version is of no use now.
    column.blaming = None;
    let first = column.asked.is_some();
    let edited = column.followed.replace(buffer) == Some(buffer);
    let path = column.path.clone();
    // The first blame shown is a fresh one unless it found something: the
    // file may be tracked by now.
    let kept = (state(cx).blamed.get(&buffer))
        .filter(|blamed| blamed.path == path && blamed.version == version)
        .is_some_and(|blamed| blamed.found.is_ok() || !first);
    if kept {
        return apply(workbench, id, cx);
    }
    // Line blame blaming it lands it here too.
    if line_blaming(buffer, &path, version, cx) {
        return;
    }
    let delay = if edited { EDIT_REST } else { Duration::ZERO };
    start(id, &editor, delay, cx);
}

/// Blames editor `id`'s buffer once `delay` is up, as it is then.
fn start(id: EntityId, editor: &Entity<EditorView>, delay: Duration, cx: &mut Context<Workbench>) {
    let model = editor.read(cx).model();
    let Some(path) = model.path().map(Path::to_owned) else {
        return;
    };
    let (buffer, asked) = (model.buffer().id(), model.buffer().version());
    let editor = editor.downgrade();
    let serial = next_serial(cx);
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
        let (head, found) = cx.background_spawn(async move { blame(&at, &text) }).await;
        (workbench.update(cx, |workbench, cx| {
            if let Some(column) = state(cx).columns.get_mut(&id) {
                column.blaming = None;
            }
            let blamed = Blamed {
                serial,
                path,
                version,
                head,
                found,
            };
            blame_landed(workbench, buffer, blamed, cx);
            let recheck = (state(cx).columns.get_mut(&id))
                .is_some_and(|column| std::mem::take(&mut column.recheck));
            if recheck {
                check_head(workbench, id, cx);
            }
        }))
        .ok();
    });
    if let Some(column) = state(cx).columns.get_mut(&id) {
        column.blaming = Some((asked, task));
    }
}

/// Shows editor `id`'s buffer's last blame in its annotation column, if
/// it's of the buffer's version now. If it found none: the toggle that
/// turned the column on is answered and the column goes, or, later, the
/// column shows nothing.
fn apply(workbench: &Workbench, id: EntityId, cx: &mut App) {
    let Some(column) = state(cx).columns.get(&id) else {
        return;
    };
    let (buffer, path) = (column.buffer, column.path.clone());
    let Some(editor) = column.editor.upgrade() else {
        return;
    };
    let version = editor.read(cx).model().buffer().version();
    let Some(found) = (state(cx).blamed.get(&buffer))
        .filter(|blamed| blamed.path == path && blamed.version == version)
        .map(|blamed| blamed.found.clone())
    else {
        return;
    };
    let Some(column) = state(cx).columns.get_mut(&id) else {
        return;
    };
    let asked = column.asked.take();
    let runs = match found {
        Ok(runs) => runs,
        Err(answer) => match asked {
            Some(op) => {
                state(cx).columns.remove(&id);
                return workbench.answer_git_op(&op, answer, cx);
            }
            None => Arc::from([]),
        },
    };
    column.shown = Some((buffer, version));
    let owner = column.workbench.clone();
    let annotations = annotations(&editor, owner, &runs, now(), cx);
    editor.update(cx, |editor, cx| {
        editor.set_gutter_annotations(Some(annotations), cx)
    });
}

/// `runs`, the blame of `editor`'s buffer at its version now, as its
/// annotations at `now`; clicked, they open commits in `workbench`.
fn annotations(
    editor: &Entity<EditorView>,
    workbench: WeakEntity<Workbench>,
    runs: &[BlameRun],
    now: i64,
    cx: &App,
) -> GutterAnnotations {
    let model = editor.read(cx).model();
    let snapshot = model.snapshot();
    let text = model.buffer().text().to_string();
    // Blame's rows end at `\n` only; the editor's also at a lone `\r`.
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let start = |row: u32| starts.get(row as usize).copied().unwrap_or(text.len());
    let heats = heats(runs);
    let (mut entries, mut commits) = (Vec::new(), Vec::new());
    for run in runs {
        let Some(commit) = &run.commit else {
            continue;
        };
        entries.push(GutterAnnotation {
            rows: snapshot.anchor_before(start(run.rows.start))
                ..snapshot.anchor_before(start(run.rows.end)),
            text: label(commit, now).into(),
            heat: heats.get(commit.id.as_str()).copied(),
        });
        commits.push(commit.clone());
    }
    let commits: Rc<[Arc<CommitInfo>]> = commits.into();
    let editor = editor.downgrade();
    GutterAnnotations {
        version: snapshot.version(),
        entries,
        hover: Rc::new(move |ix, _| {
            let commit = commits.get(ix)?;
            Some(popover(Some(commit), super::now()))
        }),
        on_click: Rc::new(move |_, row, window, cx| {
            let (Some(workbench), Some(editor)) = (workbench.upgrade(), editor.upgrade()) else {
                return;
            };
            let op = GitOp {
                command: GitCommand::OpenLineCommit,
                args: String::new(),
                range: None,
                editor: Some(editor),
                typed: false,
            };
            workbench.update(cx, |workbench, cx| {
                open_line_commit(workbench, op, Some(row), window, cx)
            });
        }),
    }
}

/// Each commit's tint, by id: its rank by author time among `runs`'
/// commits, newest first, `r` of `n` getting `1 - r / (n - 1)`, and a lone
/// one 1.0.
fn heats(runs: &[BlameRun]) -> HashMap<&str, f32> {
    let mut commits: Vec<&CommitInfo> = Vec::new();
    for commit in runs.iter().filter_map(|run| run.commit.as_deref()) {
        if !commits.iter().any(|seen| seen.id == commit.id) {
            commits.push(commit);
        }
    }
    commits.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.id.cmp(&b.id)));
    let last = commits.len().saturating_sub(1).max(1) as f32;
    (commits.iter().enumerate())
        .map(|(rank, commit)| (commit.id.as_str(), 1. - rank as f32 / last))
        .collect()
}

/// An entry's text for `commit` at `now`: `<when> <author>`.
pub(super) fn label(commit: &CommitInfo, now: i64) -> String {
    let age = now - commit.time;
    let when = if age < RELATIVE {
        ago(age)
    } else {
        date(commit)[..10].to_owned()
    };
    let mut author: String = commit.author.chars().take(AUTHOR_CHARS).collect();
    if commit.author.chars().count() > AUTHOR_CHARS {
        author.pop();
        author.push('…');
    }
    tr!("workbench-git-blame-column", when = when, author = author).into()
}

/// After a git refresh, or as editor `id` becomes active: reads HEAD's id
/// for its file, and blames its buffer again if HEAD moved since its last
/// blame, or if that found nothing to show. The read lands only if the
/// editor is still annotated, its buffer unchanged and no blame is pending.
fn check_head(_: &mut Workbench, id: EntityId, cx: &mut Context<Workbench>) {
    let Some(column) = state(cx).columns.get_mut(&id) else {
        return;
    };
    // Read once the blame pending has landed.
    if column.blaming.is_some() {
        column.recheck = true;
        return;
    }
    let (buffer, path) = (column.buffer, column.path.clone());
    let Some(editor) = column.editor.upgrade() else {
        return;
    };
    let version = editor.read(cx).model().buffer().version();
    let Some((head, found_none)) = (state(cx).blamed.get(&buffer))
        .filter(|blamed| blamed.path == path && blamed.version == version)
        .map(|blamed| (blamed.head.clone(), blamed.found.is_err()))
    else {
        return;
    };
    let at = path.clone();
    let read = cx.background_spawn(async move { repo(&at).ok()?.0.head_id() });
    let task = cx.spawn(async move |workbench, cx| {
        let now = read.await;
        if now == head && !found_none {
            return;
        }
        (workbench.update(cx, |_, cx| {
            let Some(column) = state(cx).columns.get_mut(&id) else {
                return;
            };
            column.head_check = None;
            if column.blaming.is_some() {
                column.recheck = true;
                return;
            }
            let model = editor.read(cx).model();
            let still = model.buffer().id() == buffer && model.buffer().version() == version;
            if still && !line_blaming(buffer, &path, version, cx) {
                start(id, &editor, Duration::ZERO, cx);
            }
        }))
        .ok();
    });
    if let Some(column) = state(cx).columns.get_mut(&id) {
        column.head_check = Some(task);
    }
}
