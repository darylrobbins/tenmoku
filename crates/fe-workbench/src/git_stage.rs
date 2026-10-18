//! Staging, unstaging and reverting a file or a change:
//! `git.stage_file`, `git.unstage_file`, `git.discard_file`,
//! `git.reset_buffer`, `git.stage_hunk`, `git.unstage_hunk` and
//! `git.revert_hunk`, routed here by `git_commands`, and the Changes
//! panel's buttons and keys for its rows.
//!
//! `git.stage_file`, `git.unstage_file` and `git.discard_file` act on the
//! Changes panel's selected file while the panel has focus (on nothing if
//! no row's selected), else on the active editor's file: a diff view
//! side's working file, or the file a staged diff compares. Staging an
//! editor's file saves it first, as fugitive's `:Gwrite` writes the work
//! tree and then the index, and as `:Gitsigns stage_buffer` does here too;
//! `:Gitsigns reset_buffer_index` unstages it. `git.reset_buffer`
//! (`:Gread`, `:Gitsigns reset_buffer`) replaces the editor's text with
//! the file's text in the index, as an undo step of its own, left unsaved;
//! a read-only editor answers `E21`, and a read overtaken by a later one,
//! or landing after the editor moved to another file, is dropped.
//!
//! `git.stage_hunk` (`:Gitsigns stage_hunk`) stages a change of the
//! editor's text, saved or not, as VS Code stages from the editor: the
//! file's text in the index diffed against the buffer's, the changes
//! meeting the caret's row staged whole; with a selection, a visual one or
//! an ex range (`:'<,'>Gitsigns stage_hunk`), just the rows in it, as
//! gitsigns stages part of a hunk. The buffer and the file stay as they
//! are. With nothing to stage there it answers `No change to stage at this
//! line`. Through a symlink it stages the file the link points to, whose
//! text the buffer has; a file whose buffer or blob in the index isn't
//! UTF-8 answers `Can't stage part of a file that isn't UTF-8`, as
//! rewriting the blob as UTF-8 would change its other rows. In a diff view it stages from the working side, the one with
//! the file; `dp` and `:diffput` there, in a diff against the index, stage
//! the changes in their rows whole, as fugitive's `:Gdiffsplit` puts into
//! the index, and the left side then shows the index's new text.
//!
//! `git.unstage_hunk` (`:Gitsigns undo_stage_hunk`) is gitsigns' undo:
//! each file's stages this session are a stack, and it puts back the
//! index entry from before the last one, the very blob, unless the entry
//! has changed since, or gone, from what that stage left (`Index changed
//! since; not undone`), which then can't be undone. What a stage left is
//! the blob git wrote, after the clean filters `git add` runs.
//!
//! `git.revert_hunk` (`:Gitsigns reset_hunk`) puts the editor's diff
//! base's rows back over each change meeting the caret's row, the
//! selection's or the range's: HEAD's in a tab, what its change bar shows,
//! and the left side's in a diff view. The engine reverts whole changes,
//! so a range over part of one reverts all of it. It's one undo step, so it
//! asks nothing, as VS Code's Revert Change asks nothing; discarding a
//! whole file still asks.
//!
//! The commands act on the caret's row, so a click on a change bar, which
//! moves the caret there, picks the change they act on. (There are no
//! buttons in the gutter.)
//!
//! Discarding reads the files' status first, so it knows which are
//! untracked whatever the panel last listed; files with nothing to discard
//! are left out. Then it asks, in VS Code's words: "Are you sure you want
//! to discard changes in a.rs?" (Discard File), for an untracked file "Are
//! you sure you want to DELETE a.rs?" (Delete File) and that it's
//! irreversible, and for several "Are you sure you want to discard changes
//! in N files?". Once you're sure, it reads the status again and discards
//! only the files still as you were asked about them: a tracked file goes
//! back to its text in the index, an untracked one is deleted. Open
//! editors take the file as it is on disk as any change on disk reloads
//! them.
//!
//! git acts on the file named, through symlinked folders but not a
//! symlink that is the file itself, which git tracks as a link. The work
//! that changes the repository runs on the background executor, one at a
//! time in the order asked (a save before staging starts in its turn, and
//! a save refused stages nothing), each followed by a refresh of the workbench's
//! git state, which lists the panel again. A failure is an error toast,
//! "Git: " and the first line of git's error, the rest its detail.

use std::{
    collections::{HashMap, VecDeque},
    ffi::{OsStr, OsString},
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use fe_buffer::{Buffer, BufferId, FileEncoding, Origin};
use fe_editor::text_diff::{Hunk, TextDiff};
use fe_git::{ChangeKind, GitError, Repo};
use fe_gpui::EditorView;
use gpui::{
    App, AppContext as _, Context, Entity, Global, PromptButton, PromptLevel, SharedString, Task,
    WeakEntity, Window,
};

use super::{
    Workbench,
    git_commands::{GitCommand, GitOp},
    git_diff::{self, E21, NO_FILE},
    toast::{self, Toast},
    vim_host,
};
use crate::git::{self, GitState};

/// What deleting an untracked file adds to its question.
const IRREVERSIBLE: &str = "This is IRREVERSIBLE! This file will be FOREVER LOST if you proceed.";

/// What discarding several files adds when some are untracked.
const SOME_IRREVERSIBLE: &str =
    "This is IRREVERSIBLE! The untracked files among them will be FOREVER LOST if you proceed.";

/// What discarding files with no changes answers.
const NOTHING_TO_DISCARD: &str = "No changes to discard";

/// What staging a change answers with none there.
const NO_CHANGE_TO_STAGE: &str = "No change to stage at this line";

/// What reverting a change answers with none there.
const NO_CHANGE_TO_REVERT: &str = "No change to revert at this line";

/// What staging from a diff view's git side answers.
const GIT_SIDE: &str = "Stage from the Working Tree side";

/// What undoing a stage answers with none of the file's to undo.
const NOTHING_STAGED: &str = "No staged change to undo";

/// What undoing a stage answers when the index has changed since.
const INDEX_CHANGED: &str = "Index changed since; not undone";

/// Why git work didn't happen.
enum Failed {
    /// What to answer: no repository, or nothing to do.
    Answer(String),
    /// What git said as it failed.
    Git(String),
}

/// What discarding a file does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Discard {
    /// Takes it back to its text in the index.
    Restore,
    /// Deletes it: it's untracked.
    Delete,
}

/// A save that git work waits for, started when the work's turn comes:
/// its task resolves to the error to show, if any.
type Save = Box<dyn FnOnce(&mut App) -> Task<Option<String>>>;

/// What a save answers, before the file's name, if its editor moved to
/// another buffer or file before its turn came.
const MOVED: &str = "Not saved or staged: the editor no longer edits ";

/// What a save answers if its window closed before its turn came.
const WINDOW_CLOSED: &str = "Not saved: the window closed";

/// Work that changes a repository: given the app, it starts and returns
/// its task.
type Mutation = Box<dyn FnOnce(&mut App) -> Task<()>>;

/// The work that changes repositories, run one at a time in the order
/// asked.
#[derive(Default)]
struct Mutations {
    queued: VecDeque<Mutation>,
    running: bool,
}

impl Global for Mutations {}

/// The `git.reset_buffer` reads started, the last one's number for each
/// buffer, whichever of its editors asked: only that one lands.
#[derive(Default)]
struct ResetReads {
    started: u64,
    last: HashMap<BufferId, u64>,
}

impl Global for ResetReads {}

/// What staging part of a file that isn't UTF-8 answers.
const NOT_UTF8: &str = "Can't stage part of a file that isn't UTF-8";

/// A file's entry in the index: its mode and its blob's id, as `git
/// ls-files --stage` gives them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    mode: String,
    id: String,
}

/// What a change staged did: the file's index entry before, and the one
/// git wrote, filtered as `git add` filters.
struct Staged {
    before: Entry,
    after: Entry,
}

/// Each file's changes staged this session, by its real path, the last
/// on top: what `git.unstage_hunk` undoes. Shared with the git work, which
/// runs one at a time.
#[derive(Clone, Default)]
struct StagedChanges(Arc<Mutex<HashMap<PathBuf, Vec<Staged>>>>);

impl Global for StagedChanges {}

/// Which of an editor's changes to stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Pick {
    /// Each change meeting these rows of its text, whole.
    Changes(Range<u32>),
    /// Just these rows.
    Rows(Range<u32>),
}

/// Runs `op`, a staging command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let command = op.command;
    match command {
        GitCommand::StageFile | GitCommand::UnstageFile | GitCommand::DiscardFile => {
            let changes = workbench.dock().read(cx).changes().clone();
            let (path, editor) = match changes.read(cx).focused_selection(window, cx) {
                Some(Some(path)) => (path, None),
                // The panel has focus, but no row's selected.
                Some(None) => return,
                None => {
                    let path =
                        (op.editor.as_ref()).and_then(|editor| workbench.op_path(editor, cx));
                    let Some(path) = path else {
                        return workbench.answer_git_op(&op, NO_FILE.to_owned(), cx);
                    };
                    (path, workbench.working_editor(&op, cx))
                }
            };
            match (command, editor) {
                (GitCommand::StageFile, Some(editor)) => {
                    workbench.save_and_stage(editor, path, op, window, cx)
                }
                (GitCommand::StageFile, None) => {
                    workbench.stage(vec![path], true, Some(op), None, cx)
                }
                (GitCommand::UnstageFile, _) => {
                    workbench.stage(vec![path], false, Some(op), None, cx)
                }
                _ => workbench.discard(vec![path], Some(op), window, cx),
            }
        }
        GitCommand::ResetBuffer => workbench.reset_buffer(op, cx),
        GitCommand::StageHunk => {
            let Some(editor) = op.editor.clone() else {
                return workbench.answer_git_op(&op, NO_FILE.to_owned(), cx);
            };
            let pick = picked(&op, &editor, cx);
            workbench.stage_change(&editor, pick, op, cx);
        }
        GitCommand::UnstageHunk => workbench.undo_stage(op, cx),
        GitCommand::RevertHunk => workbench.revert_change(op, cx),
        _ => {
            let text = format!("Not supported yet: {}", command.title());
            workbench.answer_git_op(&op, text, cx);
        }
    }
}

/// The rows `op` acts on in `editor`: its ex range's, else a selection's,
/// else the changes meeting the caret's row. In vim, only a visual
/// selection counts, every row from its anchor's through its cursor's.
fn picked(op: &GitOp, editor: &Entity<EditorView>, cx: &App) -> Pick {
    if let Some(range) = op.range.clone() {
        return Pick::Rows(range);
    }
    let view = editor.read(cx);
    let model = view.model();
    let text = model.text();
    let row = |offset| text.offset_to_point(offset).row;
    let selection = model.primary_selection();
    let range = selection.range.clone();
    let ends = match view.vim_state() {
        Some(vim) => (vim.visual_anchor(model)).zip(vim.visual_cursor(model)),
        // A selection ending at a row's start leaves that row out.
        None if !range.is_empty() => {
            let end = text.offset_to_point(range.end);
            let last = match end.column == 0 {
                true => end.row.saturating_sub(1).max(row(range.start)),
                false => end.row,
            };
            return Pick::Rows(row(range.start)..last + 1);
        }
        None => None,
    };
    match ends {
        Some((anchor, cursor)) => {
            let (anchor, cursor) = (row(anchor), row(cursor));
            Pick::Rows(anchor.min(cursor)..anchor.max(cursor) + 1)
        }
        None => {
            let at = row(selection.head());
            Pick::Changes(at..at + 1)
        }
    }
}

/// Whether `hunk`'s rows of the new text meet `rows`: a deletion's when
/// they hold the row it sits above or the row above that.
fn meets(hunk: &Hunk, rows: &Range<u32>) -> bool {
    let picked = |row: u32| rows.contains(&row);
    match hunk.right.is_empty() {
        true => picked(hunk.right.start) || hunk.right.start.checked_sub(1).is_some_and(picked),
        false => rows.start.max(hunk.right.start) < rows.end.min(hunk.right.end),
    }
}

/// `index` with `pick` of `text`'s changes against it staged, or `None`
/// if that stages nothing.
fn staged_text(index: &str, text: &str, pick: &Pick) -> Option<String> {
    let diff = TextDiff::new(index, text);
    let staged = match pick {
        Pick::Changes(rows) => {
            let chosen: Vec<usize> = (diff.hunks().iter().enumerate())
                .filter(|(_, hunk)| meets(hunk, rows))
                .map(|(ix, _)| ix)
                .collect();
            if chosen.is_empty() {
                return None;
            }
            diff.apply(index, text, &chosen)
        }
        Pick::Rows(rows) => diff.apply_rows(index, text, rows.clone()),
    };
    (staged != index).then_some(staged)
}

impl Workbench {
    /// Stages `paths`, or with `stage` false unstages them, for the Changes
    /// panel: see the module docs.
    pub(crate) fn stage_files(&mut self, paths: Vec<PathBuf>, stage: bool, cx: &mut Context<Self>) {
        self.stage(paths, stage, None, None, cx);
    }

    /// Stages the conflicted `paths`, for the Changes panel's Merge Changes
    /// rows: each file's text is the open buffer's, else the file's on
    /// disk, and those that still hold conflict markers are asked about
    /// once, in VS Code's words; the rest are staged without asking.
    pub(crate) fn stage_merge_files(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let open = self.file_editors(cx);
        let mut editors: Vec<Entity<EditorView>> = Vec::new();
        let texts: Vec<Option<String>> = (paths.iter())
            .map(|path| {
                let editor = open.iter().find(|editor| {
                    let view = editor.read(cx);
                    (view.model().path()).is_some_and(|file| git_diff::same_file(file, path))
                });
                if let Some(editor) = editor {
                    editors.push(editor.clone());
                }
                editor.map(|editor| editor.read(cx).model().buffer().text().to_string())
            })
            .collect();
        let read = cx.background_spawn(async move {
            let mut unresolved = Vec::new();
            for (path, text) in paths.iter().zip(texts) {
                let text = match text {
                    Some(text) => text,
                    None => String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default())
                        .into_owned(),
                };
                if !fe_git::conflict_markers::parse(&text).is_empty() {
                    unresolved.push(path.clone());
                }
            }
            (paths, unresolved)
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let (paths, unresolved) = read.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.confirm_stage_merge(paths, unresolved, editors, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// Asks whether to stage `unresolved`, the files of `paths` that still
    /// have conflict markers, and stages `paths` if so; with none, stages
    /// them without asking. The open `editors` of the files are saved first,
    /// so the text staged is the text checked; a save that fails stages
    /// nothing.
    fn confirm_stage_merge(
        &mut self,
        paths: Vec<PathBuf>,
        unresolved: Vec<PathBuf>,
        editors: Vec<Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = window.window_handle();
        let save = move || -> Option<Save> {
            if editors.is_empty() {
                return None;
            }
            Some(Box::new(move |cx: &mut App| {
                let saves: Vec<Task<Option<String>>> = (editors.iter())
                    .map(|editor| {
                        let saved = cx.update_window(handle, |_, window, cx| {
                            vim_host::save(editor, NO_FILE.to_owned(), window, cx)
                        });
                        saved.unwrap_or_else(|_| Task::ready(Some(WINDOW_CLOSED.to_owned())))
                    })
                    .collect();
                cx.spawn(async move |_| {
                    let mut failed = None;
                    for save in saves {
                        failed = failed.or(save.await);
                    }
                    failed
                })
            }))
        };
        let question = match unresolved.as_slice() {
            [] => return self.stage_checked(paths, Vec::new(), save(), cx),
            [path] => format!(
                "Are you sure you want to stage {} with merge conflicts?",
                file_name(path)
            ),
            files => format!(
                "Are you sure you want to stage {} files with merge conflicts?",
                files.len()
            ),
        };
        let buttons = [
            PromptButton::Ok(SharedString::new_static("Stage")),
            PromptButton::Cancel(SharedString::new_static("Cancel")),
        ];
        let answer = window.prompt(PromptLevel::Warning, &question, None, &buttons, cx);
        cx.spawn(async move |workbench, cx| {
            if answer.await != Ok(0) {
                return;
            }
            workbench
                .update(cx, |workbench, cx| {
                    workbench.stage_checked(paths, unresolved, save(), cx)
                })
                .ok();
        })
        .detach();
    }

    /// Stages `paths` once `save` has written the open editors, but only
    /// if no file of them but `confirmed` holds conflict markers by then:
    /// the text the save wrote, not the text that was asked about, is what
    /// `git add` stages, and the buffer may have changed in between.
    fn stage_checked(
        &mut self,
        paths: Vec<PathBuf>,
        confirmed: Vec<PathBuf>,
        save: Option<Save>,
        cx: &mut Context<Self>,
    ) {
        let confirmed: Vec<PathBuf> = confirmed
            .iter()
            .map(|path| git_diff::link_path(path))
            .collect();
        self.run_git(paths, None, save, cx, move |repo, paths| {
            for path in paths {
                if confirmed.contains(path) {
                    continue;
                }
                let text = std::fs::read(path).unwrap_or_default();
                if !fe_git::conflict_markers::parse(&String::from_utf8_lossy(&text)).is_empty() {
                    return Err(Failed::Answer(format!(
                        "{} now has merge conflicts; not staged",
                        file_name(path)
                    )));
                }
            }
            repo.stage(paths).map_err(git_failed)
        });
    }

    /// Asks whether to discard the changes to `paths`, and does if so, for
    /// the Changes panel: see the module docs.
    pub(crate) fn discard_files(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.discard(paths, None, window, cx);
    }

    /// Stages `paths`, or with `stage` false unstages them, for `op` if a
    /// command asked, once `before`, a save, has succeeded.
    fn stage(
        &mut self,
        paths: Vec<PathBuf>,
        stage: bool,
        op: Option<GitOp>,
        before: Option<Save>,
        cx: &mut Context<Self>,
    ) {
        self.run_git(paths, op, before, cx, move |repo, paths| {
            let staged = match stage {
                true => repo.stage(paths),
                false => repo.unstage(paths),
            };
            staged.map_err(git_failed)
        });
    }

    /// Reads what discarding `paths` would do, asks whether to, and does if
    /// so, for `op` if a command asked.
    fn discard(
        &mut self,
        paths: Vec<PathBuf>,
        op: Option<GitOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let read = cx.background_spawn(async move {
            let real: Vec<PathBuf> = paths.iter().map(|path| git_diff::link_path(path)).collect();
            let repo = git_diff::repo_of(&real[0]).map_err(Failed::Answer)?;
            classify(&repo, &real)
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let read = read.await;
            workbench
                .update_in(cx, |workbench, window, cx| match read {
                    Ok(files) if files.is_empty() => {
                        let nothing = Failed::Answer(NOTHING_TO_DISCARD.to_owned());
                        workbench.report(nothing, op.as_ref(), cx)
                    }
                    Ok(files) => workbench.confirm_discard(files, op, window, cx),
                    Err(failed) => workbench.report(failed, op.as_ref(), cx),
                })
                .ok();
        })
        .detach();
    }

    /// Asks whether to discard `files`, each with what that does, and does
    /// if so, for each that it still does.
    fn confirm_discard(
        &mut self,
        files: Vec<(PathBuf, Discard)>,
        op: Option<GitOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (question, detail, button) = match files.as_slice() {
            [] => return,
            [(path, Discard::Restore)] => (
                format!(
                    "Are you sure you want to discard changes in {}?",
                    file_name(path)
                ),
                None,
                "Discard File".to_owned(),
            ),
            [(path, Discard::Delete)] => (
                format!("Are you sure you want to DELETE {}?", file_name(path)),
                Some(IRREVERSIBLE),
                "Delete File".to_owned(),
            ),
            files => (
                format!(
                    "Are you sure you want to discard changes in {} files?",
                    files.len()
                ),
                (files.iter().any(|(_, discard)| *discard == Discard::Delete))
                    .then_some(SOME_IRREVERSIBLE),
                format!("Discard {} Files", files.len()),
            ),
        };
        let buttons = [
            PromptButton::Ok(SharedString::from(button)),
            PromptButton::Cancel(SharedString::new_static("Cancel")),
        ];
        let answer = window.prompt(PromptLevel::Warning, &question, detail, &buttons, cx);
        cx.spawn(async move |workbench, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let (paths, asked): (Vec<PathBuf>, Vec<Discard>) = files.into_iter().unzip();
            workbench
                .update(cx, |workbench, cx| {
                    workbench.run_git(paths, op, None, cx, move |repo, paths| {
                        discard_as_asked(repo, paths, &asked)
                    })
                })
                .ok();
        })
        .detach();
    }

    /// Runs `run` on `paths`, each through its folder's symlinks, in the
    /// repository of the first, on the background executor once the work
    /// asked before is done and `before`, a save, has succeeded (a save
    /// that failed is the answer, and nothing runs); then shows a failure,
    /// and refreshes. Its place in the queue is taken now, whatever it
    /// waits for.
    fn run_git(
        &mut self,
        paths: Vec<PathBuf>,
        op: Option<GitOp>,
        before: Option<Save>,
        cx: &mut Context<Self>,
        run: impl FnOnce(&Repo, &[PathBuf]) -> Result<(), Failed> + Send + 'static,
    ) {
        if paths.is_empty() {
            return;
        }
        let workbench = cx.weak_entity();
        mutate(cx, move |cx| {
            // The save starts only now, once the work before it is done.
            let saving = before.map(|save| save(cx));
            cx.spawn(async move |cx| {
                let saved = match saving {
                    Some(saving) => saving.await,
                    None => None,
                };
                if let Some(failed) = saved {
                    let failed = Failed::Answer(failed);
                    (workbench.update(cx, |workbench, cx| {
                        workbench.report(failed, op.as_ref(), cx)
                    }))
                    .ok();
                    return;
                }
                #[cfg(test)]
                let gate = cx.update(test_gate);
                let ran = cx.background_spawn(async move {
                    #[cfg(test)]
                    gate.await;
                    let real: Vec<PathBuf> =
                        paths.iter().map(|path| git_diff::link_path(path)).collect();
                    let repo = git_diff::repo_of(&real[0]).map_err(Failed::Answer)?;
                    run(&repo, &real)
                });
                let ran = ran.await;
                (workbench.update(cx, |workbench, cx| {
                    if let Err(failed) = ran {
                        workbench.report(failed, op.as_ref(), cx);
                    }
                    workbench.git.update(cx, GitState::refresh);
                }))
                .ok();
            })
        });
    }

    /// Shows `failed`: git's failure in an error toast, else the answer to
    /// `op`, or a warning if no command asked.
    fn report(&self, failed: Failed, op: Option<&GitOp>, cx: &mut App) {
        match (failed, op) {
            (Failed::Git(error), _) => toast::show(cx, failure(&error)),
            (Failed::Answer(answer), Some(op)) => self.answer_git_op(op, answer, cx),
            (Failed::Answer(answer), None) => toast::show(cx, Toast::warning(answer)),
        }
    }

    /// The editor of `op`'s file that saving writes: the editor itself, or
    /// a diff view side's working side; for a staged diff's sides, which
    /// are git's, an open editor of the file it compares, if any, found by
    /// the file's real path as [`git_diff::same_file`] finds it, however
    /// it was opened.
    fn working_editor(&self, op: &GitOp, cx: &App) -> Option<Entity<EditorView>> {
        let editor = op.editor.as_ref()?;
        let edits = |editor: &Entity<EditorView>, path: Option<&Path>| {
            let view = editor.read(cx);
            let Some(file) = view.model().path() else {
                return false;
            };
            path.is_none_or(|path| git_diff::same_file(file, path)) && !view.is_read_only()
        };
        let file = self.file_editor(editor, cx);
        if edits(&file, None) {
            return Some(file);
        }
        let path = self.op_path(editor, cx)?;
        (self.file_editors(cx).into_iter()).find(|open| edits(open, Some(&path)))
    }

    /// Saves `editor`, then stages its file at `path`, both in the queue's
    /// order as of now: the save starts once the work asked before is
    /// done. A save that fails is `op`'s answer.
    fn save_and_stage(
        &mut self,
        editor: Entity<EditorView>,
        path: PathBuf,
        op: GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let window = window.window_handle();
        // The buffer and file asked about: the save checks they're still
        // the editor's as its turn comes.
        let (buffer, file) = {
            let model = editor.read(cx).model();
            (model.buffer().id(), model.path().map(Path::to_owned))
        };
        let save: Save = Box::new(move |cx| {
            let model = editor.read(cx).model();
            if model.buffer().id() != buffer || model.path() != file.as_deref() {
                let name = file.as_deref().map(file_name).unwrap_or_default();
                return Task::ready(Some(format!("{MOVED}{name}")));
            }
            let saved = cx.update_window(window, |_, window, cx| {
                vim_host::save(&editor, NO_FILE.to_owned(), window, cx)
            });
            saved.unwrap_or_else(|_| Task::ready(Some(WINDOW_CLOSED.to_owned())))
        });
        self.stage(vec![path], true, Some(op), Some(save), cx);
    }

    /// `git.reset_buffer`: replaces the editor's text with its file's text
    /// in the index, read on the background executor: see the module docs.
    fn reset_buffer(&mut self, op: GitOp, cx: &mut Context<Self>) {
        if (op.editor.as_ref()).is_some_and(|editor| editor.read(cx).is_read_only()) {
            return self.answer_git_op(&op, E21.to_owned(), cx);
        }
        let Some(editor) = self.working_editor(&op, cx) else {
            return self.answer_git_op(&op, NO_FILE.to_owned(), cx);
        };
        let Some(path) = editor.read(cx).model().path().map(Path::to_owned) else {
            return self.answer_git_op(&op, NO_FILE.to_owned(), cx);
        };
        let buffer = editor.read(cx).model().buffer().id();
        let reads = cx.default_global::<ResetReads>();
        reads.started += 1;
        let read_number = reads.started;
        reads.last.insert(buffer, read_number);
        #[cfg(test)]
        let gate = test_gate(cx);
        let at = path.clone();
        let read = cx.background_spawn(async move {
            let real = git_diff::link_path(&at);
            let repo = git_diff::repo_of(&real).map_err(Failed::Answer)?;
            let text = repo.index_text(&real).map_err(git_failed)?;
            #[cfg(test)]
            gate.await;
            let not_in_index = || Failed::Answer(format!("{} is not in the index", file_name(&at)));
            text.ok_or_else(not_in_index)
        });
        let editor = editor.downgrade();
        cx.spawn(async move |workbench, cx| {
            let read = read.await;
            workbench
                .update(cx, |workbench, cx| {
                    let Some(editor) = still_for(&editor, buffer, read_number, &path, cx) else {
                        return;
                    };
                    match read {
                        Ok(text) => replace_text(&editor, &text, cx),
                        Err(failed) => workbench.report(failed, Some(&op), cx),
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Stages `pick` of `editor`'s text into the index, for `op`: see the
    /// module docs. The text is taken now; the index's is read as the
    /// work's turn comes.
    pub(super) fn stage_change(
        &mut self,
        editor: &Entity<EditorView>,
        pick: Pick,
        op: GitOp,
        cx: &mut Context<Self>,
    ) {
        let view = editor.read(cx);
        let Some(path) = view.model().path().map(Path::to_owned) else {
            // A diff view's git side, or an editor with no file.
            let answer = match self.diff_of(editor) {
                Some(_) => GIT_SIDE,
                None => NO_FILE,
            };
            return self.answer_git_op(&op, answer.to_owned(), cx);
        };
        // The text as the file has it: a UTF-8 BOM is the buffer's, not
        // its text's. Another encoding's bytes aren't the text's.
        let encoding = view.model().buffer().encoding();
        if encoding.encoding != FileEncoding::UTF8.encoding {
            return self.answer_git_op(&op, NOT_UTF8.to_owned(), cx);
        }
        let bom = if encoding.bom { "\u{feff}" } else { "" };
        let text = format!("{bom}{}", view.model().text());
        let staged = cx.default_global::<StagedChanges>().clone();
        // The file a symlink points to is the one the buffer has.
        let real = git::resolve(&path);
        self.run_git(vec![real], Some(op), None, cx, move |repo, real| {
            let (real, rela) = (&real[0], relative(repo, &real[0])?);
            let before = file_entry(repo, real, &rela)?;
            let index = blob(repo, &before.id)?;
            let index = String::from_utf8(index).map_err(|_| Failed::Answer(NOT_UTF8.into()))?;
            let Some(text) = staged_text(&index, &text, &pick) else {
                return Err(Failed::Answer(NO_CHANGE_TO_STAGE.to_owned()));
            };
            repo.set_index_text(real, &text).map_err(git_failed)?;
            let Some(after) = index_entry(repo, &rela)? else {
                return Err(not_in_index(real));
            };
            // Filtered to what was there already, it staged nothing.
            if after != before {
                let mut stacks = staged.0.lock().unwrap_or_else(PoisonError::into_inner);
                let staged = Staged { before, after };
                stacks.entry(real.clone()).or_default().push(staged);
            }
            Ok(())
        });
    }

    /// `git.unstage_hunk`: puts back the index's text from before the last
    /// change of the file staged this session, if the index still has what
    /// that wrote: see the module docs.
    fn undo_stage(&mut self, op: GitOp, cx: &mut Context<Self>) {
        let path = (op.editor.as_ref()).and_then(|editor| self.op_path(editor, cx));
        let Some(path) = path else {
            return self.answer_git_op(&op, NO_FILE.to_owned(), cx);
        };
        let staged = cx.default_global::<StagedChanges>().clone();
        let real = git::resolve(&path);
        self.run_git(vec![real], Some(op), None, cx, move |repo, real| {
            let (real, rela) = (&real[0], relative(repo, &real[0])?);
            let mut stacks = staged.0.lock().unwrap_or_else(PoisonError::into_inner);
            let stack = stacks.entry(real.clone()).or_default();
            let Some(last) = stack.last() else {
                return Err(Failed::Answer(NOTHING_STAGED.to_owned()));
            };
            // Changed, or gone, since: it can never be undone now.
            if index_entry(repo, &rela)?.as_ref() != Some(&last.after) {
                stack.pop();
                return Err(Failed::Answer(INDEX_CHANGED.to_owned()));
            }
            // The blob that was there, as it was: not filtered again.
            let Entry { mode, id } = &last.before;
            let mut entry = OsString::from(format!("{mode},{id},"));
            entry.push(&rela);
            git_output(
                repo,
                &["update-index".as_ref(), "--cacheinfo".as_ref(), &entry],
            )?;
            stack.pop();
            Ok(())
        });
    }

    /// `git.revert_hunk`: puts the editor's diff base's rows back over the
    /// changes meeting `op`'s rows, as one undo step: see the module docs.
    fn revert_change(&mut self, op: GitOp, cx: &mut Context<Self>) {
        let Some(editor) = op.editor.clone() else {
            return self.answer_git_op(&op, NO_FILE.to_owned(), cx);
        };
        if editor.read(cx).is_read_only() {
            return self.answer_git_op(&op, E21.to_owned(), cx);
        }
        let rows = match picked(&op, &editor, cx) {
            Pick::Changes(rows) | Pick::Rows(rows) => rows,
        };
        let reverted = editor.update(cx, |view, cx| {
            let reverted = view.model_mut().revert_changes(rows);
            cx.notify();
            reverted
        });
        if reverted == 0 {
            self.answer_git_op(&op, NO_CHANGE_TO_REVERT.to_owned(), cx);
        }
    }
}

/// What a file not in the index answers.
fn not_in_index(real: &Path) -> Failed {
    Failed::Answer(format!("{} is not in the index", file_name(real)))
}

/// `real`'s path in `repo`'s working tree.
fn relative(repo: &Repo, real: &Path) -> Result<PathBuf, Failed> {
    match real.strip_prefix(repo.workdir()) {
        Ok(rela) if !rela.as_os_str().is_empty() => Ok(rela.to_owned()),
        _ => Err(not_in_index(real)),
    }
}

/// What `git args` prints in `repo`'s working tree, its pathspecs taken
/// literally, or what it said as it failed. Blocks until `git` exits.
fn git_output(repo: &Repo, args: &[&OsStr]) -> Result<Vec<u8>, Failed> {
    let output = std::process::Command::new("git")
        .current_dir(repo.workdir())
        .arg("--literal-pathspecs")
        .args(args)
        .output()
        .map_err(|error| Failed::Git(format!("git: {error}")))?;
    match output.status.success() {
        true => Ok(output.stdout),
        false => Err(Failed::Git(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )),
    }
}

/// The stage-0 index entry at `rela`, if there is one; an answer if the
/// path has conflicts instead.
fn index_entry(repo: &Repo, rela: &Path) -> Result<Option<Entry>, Failed> {
    let args = [
        "ls-files".as_ref(),
        "--stage".as_ref(),
        "-z".as_ref(),
        "--".as_ref(),
    ];
    let listed = git_output(repo, &[&args[..], &[rela.as_os_str()]].concat())?;
    let mut conflicted = false;
    // Each `<mode> <id> <stage>\t<path>`, NUL-ended.
    for record in listed
        .split(|&byte| byte == 0)
        .filter(|record| !record.is_empty())
    {
        let record = String::from_utf8_lossy(record);
        let fields = record.split('\t').next().unwrap_or_default();
        match fields.split(' ').collect::<Vec<_>>()[..] {
            [mode, id, "0"] => {
                let (mode, id) = (mode.to_owned(), id.to_owned());
                return Ok(Some(Entry { mode, id }));
            }
            _ => conflicted = true,
        }
    }
    match conflicted {
        true => Err(Failed::Answer(format!(
            "{} has unresolved conflicts",
            file_name(rela)
        ))),
        false => Ok(None),
    }
}

/// The index entry of `real`, at `rela`, a file's: an answer if it's
/// missing, or a symlink's or a submodule's.
fn file_entry(repo: &Repo, real: &Path, rela: &Path) -> Result<Entry, Failed> {
    let entry = index_entry(repo, rela)?.ok_or_else(|| not_in_index(real))?;
    match entry.mode.as_str() {
        "100644" | "100755" => Ok(entry),
        _ => Err(Failed::Answer(format!(
            "{} is not a file in the index",
            file_name(real)
        ))),
    }
}

/// The bytes of blob `id`.
fn blob(repo: &Repo, id: &str) -> Result<Vec<u8>, Failed> {
    git_output(repo, &["cat-file".as_ref(), "blob".as_ref(), id.as_ref()])
}

/// Queues `start`, work that changes a repository, to start once the work
/// queued before it is done: with none, once the current update is over,
/// so that its save can update the window it was asked in. Commits
/// (`git_commit`) queue here too.
pub(super) fn mutate(cx: &mut App, start: impl FnOnce(&mut App) -> Task<()> + 'static) {
    let mutations = cx.default_global::<Mutations>();
    mutations.queued.push_back(Box::new(start));
    if !mutations.running {
        mutations.running = true;
        cx.defer(start_next);
    }
}

/// Starts the next work queued, if any.
fn start_next(cx: &mut App) {
    let mutations = cx.default_global::<Mutations>();
    let Some(start) = mutations.queued.pop_front() else {
        mutations.running = false;
        return;
    };
    mutations.running = true;
    let task = start(cx);
    cx.spawn(async move |cx| {
        task.await;
        cx.update(start_next);
    })
    .detach();
}

/// `editor`, if it still edits `buffer`, the file at `path`, and read
/// `read_number` of its index text is the last started for that buffer.
fn still_for(
    editor: &WeakEntity<EditorView>,
    buffer: BufferId,
    read_number: u64,
    path: &Path,
    cx: &App,
) -> Option<Entity<EditorView>> {
    let editor = editor.upgrade()?;
    let last = cx.try_global::<ResetReads>()?.last.get(&buffer);
    let model = editor.read(cx).model();
    let same = model.buffer().id() == buffer && model.path() == Some(path);
    (last == Some(&read_number) && same).then_some(editor)
}

/// What discarding each of `real` does, as `repo`'s status says now: those
/// with nothing to discard left out. Runs on the background executor.
fn classify(repo: &Repo, real: &[PathBuf]) -> Result<Vec<(PathBuf, Discard)>, Failed> {
    let unstaged: HashMap<PathBuf, ChangeKind> = (repo.status_split().map_err(git_failed)?)
        .into_iter()
        .filter_map(|status| Some((status.path, status.unstaged?)))
        .collect();
    let discard = |path: &PathBuf| match unstaged.get(path)? {
        ChangeKind::Untracked => Some(Discard::Delete),
        _ => Some(Discard::Restore),
    };
    Ok((real.iter())
        .filter_map(|path| Some((path.clone(), discard(path)?)))
        .collect())
}

/// Discards each of `paths` whose discarding still does what `asked`
/// says; those that changed meanwhile are the answer. Runs on the
/// background executor.
fn discard_as_asked(repo: &Repo, paths: &[PathBuf], asked: &[Discard]) -> Result<(), Failed> {
    let now: HashMap<PathBuf, Discard> = classify(repo, paths)?.into_iter().collect();
    let (same, changed): (Vec<_>, Vec<_>) =
        (paths.iter().zip(asked)).partition(|(path, asked)| now.get(*path) == Some(*asked));
    let same: Vec<PathBuf> = same.into_iter().map(|(path, _)| path.clone()).collect();
    if !same.is_empty() {
        repo.discard(&same).map_err(git_failed)?;
    }
    match changed.as_slice() {
        [] => Ok(()),
        changed => {
            let names: Vec<String> = changed.iter().map(|(path, _)| file_name(path)).collect();
            Err(Failed::Answer(format!(
                "Changed since you were asked, so not discarded: {}",
                names.join(", ")
            )))
        }
    }
}

/// Replaces `editor`'s text with `text`, a file's contents, as one edit of
/// what differs, an undo step of its own, which leaves it unsaved.
fn replace_text(editor: &Entity<EditorView>, text: &str, cx: &mut App) {
    let text = Buffer::from_file_contents(text).text().to_string();
    editor.update(cx, |view, cx| {
        let old = view.model().text().to_string();
        if old == text {
            return;
        }
        let (span, replacement) = git_diff::changed_span(&old, &text);
        let mut buffer = view.model_mut().buffer_mut();
        buffer.with_transaction_aside(|buffer| buffer.edit([(span, replacement)], Origin::User));
        drop(buffer);
        cx.notify();
    });
}

/// `error` as git work that failed.
fn git_failed(error: GitError) -> Failed {
    Failed::Git(error_text(error))
}

/// What `error` says: git's own words if it ran and failed.
fn error_text(error: GitError) -> String {
    match error {
        GitError::Command { stderr, .. } if !stderr.trim().is_empty() => stderr,
        error => {
            let text = error.to_string();
            text.strip_prefix("git: ")
                .map_or(text.clone(), str::to_owned)
        }
    }
}

/// The error toast for `error`: its first line after "Git: ", the rest
/// its detail.
fn failure(error: &str) -> Toast {
    let mut lines = error.trim().lines();
    let first = lines.next().unwrap_or_default();
    let rest = lines.collect::<Vec<_>>().join("\n");
    let toast = Toast::error(format!("Git: {first}"));
    match rest.trim() {
        "" => toast,
        rest => toast.detail(rest),
    }
}

/// `path`'s name, as the question names it.
fn file_name(path: &Path) -> String {
    (path.file_name()).map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
impl Workbench {
    /// Holds the git work started from now: see [`Holds`].
    pub(crate) fn hold_git_work(cx: &mut App) {
        Holds::hold(cx);
    }

    /// How many pieces of git work are held.
    pub(crate) fn git_work_held(cx: &mut App) -> usize {
        Holds::held(cx)
    }

    /// Lets held piece `ix`, in the order started, go on.
    pub(crate) fn release_git_work(ix: usize, cx: &mut App) {
        Holds::release(ix, cx);
    }
}

/// What the tests hold: while holding, each piece of git work started
/// waits, the queued work before it runs and a `git.reset_buffer` read
/// before it lands, until released.
#[cfg(test)]
#[derive(Default)]
struct Holds {
    holding: bool,
    held: Vec<futures::channel::oneshot::Sender<()>>,
}

#[cfg(test)]
impl Global for Holds {}

#[cfg(test)]
impl Holds {
    /// Holds the work started from now.
    fn hold(cx: &mut App) {
        cx.default_global::<Self>().holding = true;
    }

    /// How many are held.
    fn held(cx: &mut App) -> usize {
        cx.default_global::<Self>().held.len()
    }

    /// Lets held piece `ix`, in the order started, go on.
    fn release(ix: usize, cx: &mut App) {
        cx.default_global::<Self>().held.remove(ix).send(()).ok();
    }
}

/// What a piece of git work awaits: a held one's release.
#[cfg(test)]
pub(super) fn test_gate(cx: &mut App) -> impl std::future::Future<Output = ()> + Send + use<> {
    let (release, gate) = futures::channel::oneshot::channel();
    let holds = cx.default_global::<Holds>();
    match holds.holding {
        true => holds.held.push(release),
        false => drop(release.send(())),
    }
    async move {
        gate.await.ok();
    }
}

#[cfg(test)]
#[path = "git_stage_hunk_tests.rs"]
mod git_stage_hunk_tests;
