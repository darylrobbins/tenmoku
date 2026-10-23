//! Comparing a file with the index, with HEAD or with another file, in a
//! [`DiffView`] tab, and moving between its changes: `git.diff_index`,
//! `git.diff_head`, `diff.compare_file`, `git.next_change` and
//! `git.previous_change`, and the host commands typed in a diff view's
//! sides, routed here by `git_commands`.
//!
//! A git diff's left side is the file's text in the index (`:Gdiffsplit`)
//! or in HEAD (`:Gdiffsplit HEAD`), read on the background executor, empty
//! for a file git doesn't have; its right side, `Working Tree`, is a new
//! model over the file's open buffer, so edits show in both and save as the
//! file: a tab's, or another diff view's right side's ([`Workbench::file_editors`],
//! found by the file's real path), or else the file read from disk, unless
//! it's too large to edit ([`loader::is_large`]). That working side is one
//! of the file's open editors from then on: `:wa` and `:qa` count it, disk
//! changes reload it, and a tab opened on the file shares its buffer.
//!
//! The tab opens in the pane active when it was asked for, titled `a.rs
//! (Index ↔ Working Tree)`, and is shown if it's still the last diff asked
//! for and nothing was navigated to meanwhile; asking again for the same
//! file and base shows that tab, unless the file was deleted or restored
//! since and the working side has no unsaved changes, when a diff of the
//! file as it is now replaces it.
//!
//! The Changes panel opens a Changes row's file against the index (a
//! deleted file against nothing, both sides read-only), and a Staged row's
//! HEAD against the index, `a.rs (HEAD ↔ Index)`, both sides git's and
//! read-only, as VS Code's Source Control does: the entry git tracks at
//! that name, a symlink's being its target's path. A click on an editor's
//! change bar opens its file against HEAD, side by side, with the right
//! side's caret on the change's first row, centred. (VS Code peeks at the
//! change inline instead.) `diff.compare_file` (`:diffsplit {file}`)
//! compares a file the quick-open picker, titled "Compare with…", picks
//! (or `:diffsplit` names, resolved as `:e` resolves it) on the read-only
//! left with the active file on the right.
//!
//! Each git diff's git sides are read again after a git refresh
//! ([`Workbench::refresh_git_diffs`]), a staged diff's both. Only a view's
//! last read lands, and a diff opened while a refresh ran is read again. A
//! refresh that finds the file deleted or restored rebuilds a clean diff,
//! as asking again does (a staged diff is never rebuilt: its sides are
//! git's).
//!
//! In a tab, `git.next_change` remembers the change it went to, as the
//! diff view's navigation does, so a deletion past the last row, whose
//! caret is on the row above, doesn't hold it.
//!
//! In a side, vim's `do` (`:diffget`) on the right side reverts the
//! changes in its rows to the left text, as one undo step, and `dp`
//! (`:diffput`) on the left side does the same to the right side's
//! changes there. On the right side of a diff against the index, `dp`
//! stages the changes in its rows, as `git_stage` does; a read-only side
//! answers `E21` to `do` and `:w`, and putting into a read-only left side,
//! or into HEAD, `E793`. `:w` on the right side saves the file; `:q`
//! closes the diff tab, refused with `E37` while the right side is dirty
//! and no other editor shows its buffer. `:w {path}` and `:sav[eas]` in
//! either side answer `Not supported yet` for now.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{Bias, Buffer, Origin};
use fe_editor::{
    EditorModel,
    row_changes::RowChange,
    text_diff::{Side, TextDiff},
};
use fe_git::Repo;
use fe_gpui::{
    DiffSide, DiffView, EditorView,
    commands::EditorViewEvent,
    diff_view::{NO_CHANGES, WRAPPED_TO_FIRST, WRAPPED_TO_LAST},
};
use fe_vim::HostCommand;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Focusable as _, Global, Subscription,
    WeakEntity, Window,
};
use kiln_shell::{ItemHandle as _, PaneView as _};

use super::{
    Palette, Workbench,
    git_commands::{self, GitCommand, GitOp, Owner},
    git_stage,
    vim_host::{self, E37, NO_SHELL, RELOAD, message},
};
use crate::{Pane, Picker, QuickOpen, git, loader, quick_open::Pick};

/// What a git diff answers outside a repository.
const NO_REPO: &str = "Not in a git repository";

/// What a command for the active file answers when it has none.
pub(super) const NO_FILE: &str = "E32: No file name";

/// What a read-only side answers to an edit.
pub(super) const E21: &str = "E21: Cannot make changes, 'modifiable' is off";

/// What putting into a read-only side answers.
const E793: &str = "E793: No other buffer in diff mode is modifiable";

/// The compare picker's placeholder.
const COMPARE_WITH: &str = "Compare with…";

/// The right side's title in a git diff.
const WORKING_TREE: &str = "Working Tree";

/// What a file too large to edit answers, before its name.
const TOO_LARGE: &str = "Too large to compare";

/// What `:w {path}` and `:sav[eas]` answer in a diff view's side.
const NOT_IN_DIFF: &str = "Not supported yet: writing a diff side to another file";

/// What a git diff's left side is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    Index,
    Head,
    /// HEAD, against the index on the right: what's staged, both sides
    /// git's and read-only.
    Staged,
}

impl Base {
    fn title(self) -> &'static str {
        match self {
            Base::Index => "Index",
            Base::Head | Base::Staged => "HEAD",
        }
    }
}

/// An open diff view, the file it compares and its git base: `None` for
/// one comparing two files.
struct Compared {
    view: WeakEntity<DiffView>,
    path: PathBuf,
    base: Option<Base>,
    /// Whether it compares with nothing, the file gone.
    deleted: bool,
    /// The last read of its git side started: only that one lands.
    read: u64,
    /// A model over its working side's buffer that nothing shows: what
    /// another diff of the file, or a tab opened on it, splits, sharing the
    /// buffer but none of this view's comparison or tints. `None` for a
    /// read-only side.
    source: Option<EditorModel>,
    /// A change-bar click's row to reveal in the diff that replaces this
    /// one, if a read finds it out of date.
    reveal: Option<Reveal>,
    /// Hears the working side's change bar clicked, and keeps `source`
    /// caught up with the buffer as the view moves on.
    _subscriptions: Vec<Subscription>,
}

/// The diff views opened, in any window, and the counters that order
/// their reads and opens.
#[derive(Default)]
struct DiffState {
    views: Vec<Compared>,
    /// Bumped by each diff asked for: only the last one asked for is shown
    /// as it lands.
    opens: u64,
    /// Bumped by each git-side read started.
    reads: u64,
    /// Bumped by each refresh, so an open that lands after one reads its
    /// git side again.
    refreshes: u64,
    /// The change `git.next_change` last went to in a tab: the editor, the
    /// change's index, where it left the caret, and the changes it was
    /// among, which an edit or a new base replaces.
    visited: Option<(EntityId, usize, usize, std::sync::Arc<[RowChange]>)>,
}

impl Global for DiffState {}

fn state(cx: &mut App) -> &mut DiffState {
    cx.default_global::<DiffState>()
}

/// The open diff views, the closed ones forgotten.
fn diff_views(cx: &mut App) -> &mut Vec<Compared> {
    let views = &mut state(cx).views;
    views.retain(|compared| compared.view.upgrade().is_some());
    views
}

/// The entry of `view`, if it's open.
fn compared<'a>(view: &WeakEntity<DiffView>, cx: &'a mut App) -> Option<&'a mut Compared> {
    diff_views(cx)
        .iter_mut()
        .find(|compared| compared.view == *view)
}

/// Where to put the right side's caret once a diff opens: on `row`,
/// centred in `shown` screen rows if known.
#[derive(Clone, Copy)]
struct Reveal {
    row: u32,
    shown: Option<u32>,
}

/// A git diff asked for, or rebuilt.
struct Request {
    path: PathBuf,
    base: Base,
    /// Whether the file is gone from the working tree, as a refresh found;
    /// looked at as it's read if `None`.
    deleted: Option<bool>,
    then: Option<Reveal>,
    /// What to answer if it can't open.
    op: GitOp,
    /// The diff of the file it's to replace in its tab, out of date, and
    /// the read of its git side the rebuild lands as: a later read, or the
    /// tab closing, supersedes it.
    replaces: Option<(WeakEntity<DiffView>, u64)>,
    /// Whether it was asked for, to be shown, rather than a refresh's
    /// rebuild.
    asked: bool,
    /// Whether it compares an editor's buffer, kept whether or not the file
    /// is on disk (a new file not yet saved, say), rather than a file the
    /// Changes panel lists, which may be deleted.
    live: bool,
}

/// Tells whether an ask still stands as its diff lands, else nothing opens:
/// see [`Landing`].
pub(super) type Guard = Rc<dyn Fn(&Workbench, &mut App) -> bool>;

impl Request {
    /// The diff of `path` against `base` asked for by `op`.
    fn asked(path: PathBuf, base: Base, then: Option<Reveal>, op: GitOp) -> Self {
        Self {
            path,
            base,
            deleted: None,
            then,
            op,
            replaces: None,
            asked: true,
            live: false,
        }
    }

    /// The same, comparing an editor's buffer.
    fn live(self) -> Self {
        Self { live: true, ..self }
    }
}

/// Whether `a` and `b` name the same file, through symlinks and `..`.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    a == b || git::resolve(a) == git::resolve(b)
}

/// `path`'s name, and the answer when it's too large to compare.
fn too_large(path: &Path) -> String {
    format!("{TOO_LARGE}: {}", file_name(path))
}

/// The file at `path`, read for a diff's side: none if it's missing, an
/// answer if it can't be read or is too large to edit. Runs on the
/// background executor.
fn read_file(path: &Path) -> Result<Option<Buffer>, String> {
    if loader::is_large(path) {
        return Err(too_large(path));
    }
    let cant = |error: std::io::Error| format!("Can't open {}: {error}", path.display());
    loader::read(path.to_owned()).map_err(cant)
}

/// Runs `op`, a diff command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    match op.command {
        GitCommand::DiffIndex => workbench.diff_with_git(op, Base::Index, None, window, cx),
        GitCommand::DiffHead => workbench.diff_with_git(op, Base::Head, None, window, cx),
        GitCommand::CompareFile => workbench.compare_file(op, window, cx),
        GitCommand::NextChange => workbench.go_to_change(op, true, window, cx),
        GitCommand::PreviousChange => workbench.go_to_change(op, false, window, cx),
        _ => {
            let text = format!("Not supported yet: {}", op.command.title());
            workbench.answer_git_op(&op, text, cx);
        }
    }
}

/// Opens `op`'s file against HEAD, shown only if `guard` says its ask still
/// stands as the diff lands.
pub(super) fn diff_head_guarded(
    workbench: &mut Workbench,
    op: GitOp,
    guard: Guard,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    // As `run_git_op` would route it.
    #[cfg(test)]
    workbench
        .git_routed
        .push(super::git_commands::Routed::Op(op.clone()));
    workbench.diff_with_git(op, Base::Head, Some(guard), window, cx);
}

/// Runs `command`, typed in `editor`, a side of `diff`: see the module
/// docs.
pub(super) fn host_command(
    workbench: &mut Workbench,
    diff: WeakEntity<DiffView>,
    editor: Entity<EditorView>,
    command: HostCommand,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let Some(diff) = diff.upgrade() else {
        return;
    };
    let read_only = editor.read(cx).is_read_only();
    match command {
        HostCommand::Write { path: Some(_), .. } | HostCommand::SaveAs { .. } => {
            message(&editor, NOT_IN_DIFF, cx)
        }
        HostCommand::Write { .. }
        | HostCommand::WriteQuit {
            only_if_modified: false,
            ..
        } if read_only => message(&editor, E21, cx),
        HostCommand::Write { .. } => workbench.write_side(editor, None, window, cx),
        HostCommand::WriteQuit { .. } if read_only || !editor.is_dirty(cx) => {
            workbench.close_diff(&diff, &editor, false, window, cx)
        }
        HostCommand::WriteQuit { .. } => workbench.write_side(editor, Some(diff), window, cx),
        HostCommand::Quit { force } => workbench.close_diff(&diff, &editor, force, window, cx),
        HostCommand::DiffGet { .. } if read_only => message(&editor, E21, cx),
        HostCommand::DiffGet { rows } => revert(&editor, rows, cx),
        HostCommand::DiffPut { rows } if puts_into_index(&diff, &editor, cx) => {
            let op = typed(
                GitCommand::StageHunk,
                String::new(),
                Some(rows.clone()),
                &editor,
            );
            workbench.stage_change(&editor, git_stage::Pick::Changes(rows), op, cx);
        }
        HostCommand::DiffPut { rows } => put(&diff, &editor, rows, cx),
        HostCommand::DiffSplit { path } => {
            let op = typed(GitCommand::CompareFile, path, None, &editor);
            workbench.run_git_op(op, window, cx);
        }
        HostCommand::User {
            name,
            bang,
            range,
            args,
        } => match git_commands::ex_command(&name, bang, range.is_some(), &args) {
            Ok((command, args)) => {
                let op = typed(command, args, range, &editor);
                workbench.run_git_op(op, window, cx);
            }
            Err(answer) => message(&editor, answer, cx),
        },
        HostCommand::QuitAll { force } => workbench.vim_quit_all(&editor, force, window, cx),
        HostCommand::WriteAll => workbench.vim_write_all(&editor, window, cx),
        HostCommand::Edit {
            path: Some(path), ..
        } => workbench.edit_from_diff(&diff, &editor, &path, window, cx),
        HostCommand::Edit { path: None, .. } => message(&editor, RELOAD, cx),
        HostCommand::Read { path, below } => workbench.vim_read(&editor, path, below, cx),
        HostCommand::Filter { .. } | HostCommand::ReadCommand { .. } => {
            message(&editor, NO_SHELL, cx)
        }
        HostCommand::Explore
        | HostCommand::OldFiles
        | HostCommand::OpenProblems
        | HostCommand::CloseProblems
        | HostCommand::ReplaceInFiles { .. }
        | HostCommand::Pick(_) => workbench.run_window_command(&editor, command, window, cx),
    }
}

/// The op of `command` typed in `editor`'s vim line.
fn typed(
    command: GitCommand,
    args: String,
    range: Option<Range<u32>>,
    editor: &Entity<EditorView>,
) -> GitOp {
    GitOp {
        command,
        args,
        range,
        editor: Some(editor.clone()),
        typed: true,
    }
}

/// The op a click asks for, answered in a toast.
fn clicked(command: GitCommand) -> GitOp {
    GitOp {
        command,
        args: String::new(),
        range: None,
        editor: None,
        typed: false,
    }
}

/// `do`: reverts the changes of `editor`, the right side, in `rows` to the
/// left side's text, which its row changes compare against.
fn revert(editor: &Entity<EditorView>, rows: Range<u32>, cx: &mut App) {
    editor.update(cx, |editor, cx| {
        editor.model_mut().revert_changes(rows);
        cx.notify();
    });
}

/// Whether `dp` in `editor`, a side of `diff`, puts into the index: it's
/// the right side, the file's, of a diff against the index.
fn puts_into_index(diff: &Entity<DiffView>, editor: &Entity<EditorView>, cx: &mut App) -> bool {
    let working = editor == diff.read(cx).right() && !editor.read(cx).is_read_only();
    working
        && compared(&diff.downgrade(), cx)
            .is_some_and(|compared| compared.base == Some(Base::Index))
}

/// `dp`: from the left side, reverts the right side's changes in the
/// hunks meeting `rows` (left rows, as `do` picks them); from the right
/// side of a diff not against the index, the left one takes nothing.
fn put(diff: &Entity<DiffView>, editor: &Entity<EditorView>, rows: Range<u32>, cx: &mut App) {
    let right = diff.read(cx).right().clone();
    if editor != diff.read(cx).left() {
        return message(editor, E793, cx);
    }
    if right.read(cx).is_read_only() {
        return message(editor, E793, cx);
    }
    // Diffed afresh, so the hunks are those of the texts as they are.
    let texts = [diff.read(cx).left(), &right]
        .map(|side| side.read(cx).model().buffer().text().to_string());
    let [left_text, right_text] = &texts;
    let picked = |row: u32| rows.contains(&row);
    let hunks: Vec<_> = (TextDiff::new(left_text, right_text).hunks().iter())
        .filter(|hunk| match hunk.left.is_empty() {
            true => picked(hunk.left.start) || hunk.left.start.checked_sub(1).is_some_and(picked),
            false => rows.start.max(hunk.left.start) < rows.end.min(hunk.left.end),
        })
        .cloned()
        .collect();
    let Some(first) = hunks.first().map(|hunk| hunk.right.start) else {
        return;
    };
    // Exactly the hunks picked, each right side's rows replaced by the
    // left's. Rows are split at their newlines, so a last row without one,
    // or a newline alone, is a row as the diff counts it; joined again,
    // the separators are right at the end of the text. The change is one
    // edit, and so one undo step.
    let theirs: Vec<&str> = left_text.split('\n').collect();
    let mut ours: Vec<&str> = right_text.split('\n').collect();
    for hunk in hunks.iter().rev() {
        let (left_rows, right_rows) = (hunk.left.clone(), hunk.right.clone());
        let taken = &theirs[left_rows.start as usize..left_rows.end as usize];
        ours.splice(
            right_rows.start as usize..right_rows.end as usize,
            taken.iter().copied(),
        );
    }
    let new_text = ours.join("\n");
    let (span, replacement) = changed_span(right_text, &new_text);
    right.update(cx, |view, cx| {
        let model = view.model_mut();
        model.buffer_mut().edit([(span, replacement)], Origin::User);
        let text = model.text();
        let at = text.row_start(first.min(text.row_count().saturating_sub(1)));
        model.set_cursor(at);
        cx.notify();
    });
}

/// The bytes of `old` that `new` changes, and what replaces them: all but
/// the prefix and suffix the two share, cut at char boundaries.
pub(super) fn changed_span(old: &str, new: &str) -> (Range<usize>, String) {
    let prefix = (old.char_indices().zip(new.chars()))
        .find(|((_, a), b)| a != b)
        .map_or(old.len().min(new.len()), |((at, _), _)| at);
    let suffix = (old[prefix..].chars().rev().zip(new[prefix..].chars().rev()))
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    (
        prefix..old.len() - suffix,
        new[prefix..new.len() - suffix].to_owned(),
    )
}

/// The text of `path` in `base`, or the answer: empty for a file git
/// doesn't have there, too large if its blob is as large as a file too
/// large to edit, which is refused before it's read. The repository is found from the nearest folder of it that
/// exists, so a file whose folder was deleted has one. Runs on the
/// background executor.
fn git_text(base: Base, path: &Path) -> Result<String, String> {
    git_text_at(base, path, &git::resolve(path))
}

/// The text of `path` in `base` for a staged diff, whose sides are both
/// git's: the entry git tracks at that name, a symlink's being its target's
/// path, read as [`git_text`] reads. Runs on the background executor.
fn staged_text(base: Base, path: &Path) -> Result<String, String> {
    git_text_at(base, path, &link_path(path))
}

/// [`git_text`] of `path`, found in git at `real`.
fn git_text_at(base: Base, path: &Path, real: &Path) -> Result<String, String> {
    let repo = repo_of(real)?;
    let size = match base {
        Base::Index => repo.index_blob_size(real),
        Base::Head | Base::Staged => repo.head_blob_size(real),
    };
    let size = size.map_err(|error| error.to_string())?;
    if size.is_some_and(|size| size >= loader::large_bytes(path)) {
        return Err(too_large(path));
    }
    let text = match base {
        Base::Index => repo.index_text(real),
        Base::Head | Base::Staged => repo.head_text(real),
    };
    text.map(Option::unwrap_or_default)
        .map_err(|error| error.to_string())
}

/// `path` with its folder through symlinks and `..`, its own name kept: a
/// symlink is the link git tracks, not what it points to.
pub(super) fn link_path(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) if !folder.as_os_str().is_empty() => {
            git::resolve(folder).join(name)
        }
        _ => path.to_owned(),
    }
}

/// The repository of the file at `real`, through symlinks, found from the
/// nearest folder of it that exists, or the answer. Runs on the background
/// executor.
pub(super) fn repo_of(real: &Path) -> Result<Repo, String> {
    let Some(folder) = real.ancestors().skip(1).find(|folder| folder.is_dir()) else {
        return Err(NO_REPO.to_owned());
    };
    match Repo::discover(folder) {
        Ok(Some(repo)) => Ok(repo),
        Ok(None) => Err(NO_REPO.to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// A read-only side titled `title` over `text`, a file's contents, its
/// line endings normalized as a file's are read.
fn fixed_side(title: impl Into<gpui::SharedString>, text: &str) -> DiffSide {
    let buffer = Buffer::from_file_contents(text);
    DiffSide::new(title, EditorModel::new(buffer, None)).read_only()
}

/// The name a tab gives `path`.
fn file_name(path: &Path) -> String {
    (path.file_name()).map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Shows `diff` side by side, then reveals `then`'s row.
fn reveal_change(diff: &Entity<DiffView>, then: Reveal, window: &mut Window, cx: &mut App) {
    diff.update(cx, |diff, cx| diff.set_inline(false, window, cx));
    reveal(diff, then, cx);
}

/// Puts `diff`'s right caret at the start of `reveal`'s row, centring it
/// when the rows shown are known. Both sides scroll there: fillers line
/// their screen rows up.
fn reveal(diff: &Entity<DiffView>, reveal: Reveal, cx: &mut App) {
    let (left, right) = {
        let diff = diff.read(cx);
        (diff.left().clone(), diff.right().clone())
    };
    right.update(cx, |view, cx| {
        let text = view.model().text();
        let at = text.row_start(reveal.row.min(text.row_count().saturating_sub(1)));
        view.select_and_reveal(at..at, cx);
    });
    let Some(shown) = reveal.shown else {
        return;
    };
    let fillers: u32 = (diff.read(cx).diff().fillers(Side::Right).into_iter())
        .filter(|(above, _)| *above <= reveal.row)
        .map(|(_, rows)| rows)
        .sum();
    let top = (reveal.row + fillers) as f32 - (shown / 2) as f32;
    for side in [left, right] {
        side.update(cx, |view, cx| view.set_scroll_top(top, cx));
    }
}

/// Replaces `side`'s text, a diff's git side's, with `text`, if it
/// differs. It's git's text, not an edit, so the side stays clean.
fn set_side_text(side: &Entity<EditorView>, text: &str, cx: &mut App) {
    let text = Buffer::from_file_contents(text).text().to_string();
    let text = text.as_str();
    side.update(cx, |view, cx| {
        if view.model().text().to_string() == text {
            return;
        }
        let len = view.model().text().len();
        let edits = [(0..len, text)];
        let mut buffer = view.model_mut().buffer_mut();
        buffer.edit(edits, Origin::User);
        buffer.did_save();
        drop(buffer);
        cx.notify();
    });
}

/// Starts a read of `view`'s git side, superseding any running: its
/// number, if the view is open.
fn start_read(view: &WeakEntity<DiffView>, cx: &mut App) -> Option<u64> {
    let state = state(cx);
    state.reads += 1;
    let read = state.reads;
    compared(view, cx)?.read = read;
    Some(read)
}

/// Whether read `read` of `view`'s git side is its last, which alone may
/// land.
fn is_last_read(view: &WeakEntity<DiffView>, read: u64, cx: &mut App) -> bool {
    compared(view, cx).is_some_and(|compared| compared.read == read)
}

/// Lands read `read` of `view`'s git side, `text`, and of the index's
/// text on the right of a [`Base::Staged`] diff, unless a later read
/// started: whether it landed.
fn land_read(
    view: &WeakEntity<DiffView>,
    read: u64,
    text: &str,
    index: Option<&str>,
    cx: &mut App,
) -> bool {
    let Some(diff) = view.upgrade().filter(|_| is_last_read(view, read, cx)) else {
        return false;
    };
    let (left, right) = {
        let diff = diff.read(cx);
        (diff.left().clone(), diff.right().clone())
    };
    set_side_text(&left, text, cx);
    if let Some(index) = index {
        set_side_text(&right, index, cx);
    }
    true
}

/// Runs `run` on `workbench` in the window it's the root of, once the
/// current update is over.
fn in_window(
    workbench: WeakEntity<Workbench>,
    cx: &mut App,
    run: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
) {
    cx.defer(move |cx| {
        let mut run = Some(run);
        for window in cx.windows() {
            (cx.update_window(window, |root, window, cx| {
                if root.entity_id() != workbench.entity_id() {
                    return;
                }
                if let Some(run) = run.take() {
                    (workbench.update(cx, |workbench, cx| run(workbench, window, cx))).ok();
                }
            }))
            .ok();
            if run.is_none() {
                break;
            }
        }
    });
}

/// Where a diff asked for opens, and what decides whether it's shown as it
/// lands.
struct Landing {
    /// The open's number, if it was asked for.
    serial: Option<u64>,
    pane: WeakEntity<Pane>,
    /// The pane's navigations when asked.
    navigations: u64,
    /// The pane's file opens when asked.
    opens: u64,
    /// The refreshes when asked.
    refreshes: u64,
    /// Whether what asked for it still stands, if that matters.
    guard: Option<Guard>,
}

/// What a git diff's sides were read from: the git text, whether the file
/// is there, the file from disk if no editor had it open, and the index's
/// text for a [`Base::Staged`] diff's right side.
type GitRead = Result<(String, bool, Option<Option<Buffer>>, Option<String>), String>;

impl Workbench {
    /// The open editors of files: the tabs', and each diff view's working
    /// side that has a file. A buffer may show in several.
    pub(crate) fn file_editors(&self, cx: &App) -> Vec<Entity<EditorView>> {
        let mut editors = self.open_editors(cx);
        for pane in self.panes(cx) {
            for tab in pane.read(cx).tabs() {
                let Some(diff) = tab.item().downcast_ref::<DiffView>() else {
                    continue;
                };
                let right = diff.read(cx).right();
                let view = right.read(cx);
                if view.model().path().is_some() && !view.is_read_only() {
                    editors.push(right.clone());
                }
            }
        }
        editors
    }

    /// A new model over the working buffer of the file at `path` that a
    /// diff view in any window keeps (see [`Compared::source`]), found by
    /// the file's real path: what a tab opened on the file shares.
    pub(crate) fn diff_source(path: &Path, cx: &App) -> Option<EditorModel> {
        let state = cx.try_global::<DiffState>()?;
        let sources = || {
            (state.views.iter())
                .filter(|compared| compared.view.upgrade().is_some())
                .filter_map(|compared| Some((&compared.path, compared.source.as_ref()?)))
        };
        let (_, source) = (sources().find(|(open, _)| *open == path))
            .or_else(|| sources().find(|(open, _)| same_file(open, path)))?;
        Some(source.split())
    }

    /// A new model over the open buffer of the file at `path`, found by its
    /// real path: a tab's, else a diff view's working side's.
    fn file_model(&self, path: &Path, cx: &App) -> Option<EditorModel> {
        let editors = self.open_editors(cx);
        let path_of =
            |editor: &Entity<EditorView>| editor.read(cx).model().path().map(Path::to_owned);
        let editor = (editors.iter())
            .find(|editor| path_of(editor).as_deref() == Some(path))
            .or_else(|| {
                (editors.iter())
                    .find(|editor| path_of(editor).is_some_and(|open| same_file(&open, path)))
            });
        match editor {
            Some(editor) => Some(editor.read(cx).model().split()),
            None => Self::diff_source(path, cx),
        }
    }

    /// The diff view `editor` is a side of, if it's one.
    pub(super) fn diff_of(&self, editor: &Entity<EditorView>) -> Option<Entity<DiffView>> {
        let editor = editor.downgrade();
        (self.owned_editors.iter()).find_map(|(owned, owner)| match owner {
            Owner::DiffView(diff) if *owned == editor => diff.upgrade(),
            _ => None,
        })
    }

    /// The editor of the file `editor` shows: itself, or a diff view's right
    /// side.
    pub(super) fn file_editor(&self, editor: &Entity<EditorView>, cx: &App) -> Entity<EditorView> {
        match self.diff_of(editor) {
            Some(diff) => diff.read(cx).right().clone(),
            None => editor.clone(),
        }
    }

    /// The file a command typed or run in `editor` is for: its own, a diff
    /// view side's working file, or the file a staged diff, whose sides are
    /// git's, compares.
    pub(super) fn op_path(&self, editor: &Entity<EditorView>, cx: &App) -> Option<PathBuf> {
        let file = self.file_editor(editor, cx);
        if let Some(path) = file.read(cx).model().path() {
            return Some(path.to_owned());
        }
        let diff = self.diff_of(editor)?.downgrade();
        let state = cx.try_global::<DiffState>()?;
        let compared = state.views.iter().find(|compared| compared.view == diff)?;
        Some(compared.path.clone())
    }

    /// The pane showing `diff`, and its tab's index there.
    fn diff_tab(&self, diff: &Entity<DiffView>, cx: &App) -> Option<(Entity<Pane>, usize)> {
        self.panes(cx).into_iter().find_map(|pane| {
            let tabs = pane.read(cx).tabs();
            let ix = (tabs.iter()).position(|tab| tab.item().downcast_ref() == Some(diff))?;
            Some((pane.clone(), ix))
        })
    }

    /// This window's diff view of `path` against `base`, if one is open, and
    /// whether it compares with the file deleted.
    /// A staged diff is of the entry git tracks at the path, a symlink
    /// itself, so it's found by the path with only its folder resolved;
    /// another, of a working file, by the file's real path.
    fn open_diff(&self, path: &Path, base: Base, cx: &mut App) -> Option<(Entity<DiffView>, bool)> {
        let same = |open: &Path| match base {
            Base::Staged => open == path || link_path(open) == link_path(path),
            _ => same_file(open, path),
        };
        let views: Vec<(Entity<DiffView>, bool)> = (diff_views(cx).iter())
            .filter(|compared| compared.base == Some(base) && same(&compared.path))
            .filter_map(|compared| Some((compared.view.upgrade()?, compared.deleted)))
            .collect();
        (views.into_iter()).find(|(view, _)| self.diff_tab(view, cx).is_some())
    }

    /// Shows `diff`'s tab and focuses it, superseding any diff asked for
    /// before.
    fn show_diff(&mut self, diff: &Entity<DiffView>, window: &mut Window, cx: &mut Context<Self>) {
        state(cx).opens += 1;
        if let Some((pane, ix)) = self.diff_tab(diff, cx) {
            self.activate(pane.clone(), window, cx);
            pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
        }
    }

    /// A diff asked for now, opening in `pane`: see [`Landing`].
    fn landing(&self, pane: &Entity<Pane>, asked: bool, cx: &mut App) -> Landing {
        let state = state(cx);
        let serial = asked.then(|| {
            state.opens += 1;
            state.opens
        });
        let refreshes = state.refreshes;
        let pane_state = pane.read(cx);
        Landing {
            serial,
            pane: pane.downgrade(),
            navigations: pane_state.navigations(),
            opens: pane_state.open_serial(),
            refreshes,
            guard: None,
        }
    }

    /// Whether a diff landing as `landing` says is to be shown: it's the
    /// last diff asked for, its pane is still the active one, and nothing
    /// was navigated to or opened there meanwhile.
    fn may_show(&self, landing: &Landing, cx: &mut App) -> bool {
        let last = (landing.serial).is_some_and(|serial| serial == state(cx).opens);
        let stands = (landing.guard.clone()).is_none_or(|guard| guard(self, cx));
        let pane = landing.pane.upgrade();
        last && stands
            && pane.is_some_and(|pane| {
                let state = pane.read(cx);
                pane == self.active
                    && state.navigations() == landing.navigations
                    && state.open_serial() == landing.opens
            })
    }

    /// A diff view of `left` and the working side over `source`, a model of
    /// the file's buffer, or else `right`, its sides' host commands taken
    /// here, its working side's change bar opening the file against HEAD.
    fn new_diff(
        &mut self,
        compared: (PathBuf, Option<Base>, bool),
        left: DiffSide,
        right: Result<(gpui::SharedString, EditorModel), DiffSide>,
        cx: &mut Context<Self>,
    ) -> Entity<DiffView> {
        let (path, base, deleted) = compared;
        let (right, source) = match right {
            Ok((title, source)) => (DiffSide::new(title, source.split()), Some(source)),
            Err(right) => (right, None),
        };
        let name = file_name(&path);
        let compared = path.clone();
        let diff = cx.new(|cx| DiffView::new(name, left, right, cx).with_compared_path(compared));
        let sides = {
            let view = diff.read(cx);
            [view.left().clone(), view.right().clone()]
        };
        for side in &sides {
            self.own_editor(side, Owner::DiffView(diff.downgrade()));
        }
        let mut subscriptions = Vec::new();
        if source.is_some() {
            subscriptions.push(
                cx.subscribe(&sides[1], |_, right, event: &EditorViewEvent, cx| {
                    if let EditorViewEvent::ChangeBarClicked { rows } = event {
                        Workbench::change_bar_clicked(cx.weak_entity(), right, rows.clone(), cx);
                    }
                }),
            );
            // The view notifies as either side's buffer moves on: the source
            // catches up, so it holds no events or history back.
            let view = diff.downgrade();
            subscriptions.push(cx.observe(&diff, move |_, _, cx| {
                let source = self::compared(&view, cx).and_then(|entry| entry.source.as_mut());
                if let Some(source) = source {
                    source.sync_display();
                }
            }));
        }
        // Forgotten, source and all, as it closes, whether or not the
        // workbench is still there (its window closing releases it first).
        let id = diff.entity_id();
        let app: &mut App = cx;
        app.observe_release(&diff, move |_, cx| {
            (state(cx).views).retain(|compared| compared.view.entity_id() != id);
        })
        .detach();
        diff_views(cx).push(Compared {
            view: diff.downgrade(),
            path,
            base,
            deleted,
            read: 0,
            source,
            reveal: None,
            _subscriptions: subscriptions,
        });
        diff
    }

    /// Adds `diff` as the last tab of `pane`, or of the active pane if that's
    /// gone, without showing it.
    fn place_diff(&mut self, diff: &Entity<DiffView>, pane: &WeakEntity<Pane>, cx: &mut App) {
        let pane = (pane.upgrade())
            .filter(|pane| self.panes(cx).contains(pane))
            .unwrap_or_else(|| self.active.clone());
        pane.update(cx, |pane, cx| {
            pane.pane_mut().push(Box::new(diff.clone()), cx);
            pane.items_changed(cx);
            cx.notify();
        });
    }

    /// Puts `new` in `old`'s tab, shown and focused if `old` was.
    fn replace_diff(
        &mut self,
        old: &Entity<DiffView>,
        new: &Entity<DiffView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pane, ix)) = self.diff_tab(old, cx) else {
            return;
        };
        let shown = pane.read(cx).active() == Some(ix);
        let focused = {
            let old = old.read(cx);
            (old.left().focus_handle(cx).contains_focused(window, cx))
                || old.right().focus_handle(cx).contains_focused(window, cx)
        };
        let focus = new.read(cx).right().focus_handle(cx);
        pane.update(cx, |pane, cx| {
            let at = pane.pane_mut().insert(ix + 1, Box::new(new.clone()), cx);
            if shown {
                pane.pane_mut().set_active(at);
            }
            pane.take_item(ix, window, cx);
            cx.notify();
        });
        if focused {
            window.focus(&focus, cx);
        }
    }

    /// `git.diff_index` and `git.diff_head`: the active file against
    /// `base`.
    fn diff_with_git(
        &mut self,
        op: GitOp,
        base: Base,
        guard: Option<Guard>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = (op.editor.as_ref()).map(|editor| self.file_editor(editor, cx));
        // An editor's buffer, else a staged diff's file.
        let live = (editor.as_ref()).is_some_and(|editor| editor.read(cx).model().path().is_some());
        match (op.editor.as_ref()).and_then(|editor| self.op_path(editor, cx)) {
            Some(path) => {
                let request = Request::asked(path, base, None, op);
                let request = if live { request.live() } else { request };
                self.request_guarded_diff(request, guard, window, cx)
            }
            None => self.answer_git_op(&op, NO_FILE.to_owned(), cx),
        }
    }

    /// Opens `request`'s diff, or shows the one open, whose git side and
    /// file are then looked at again: see the module docs.
    fn request_git_diff(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        self.request_guarded_diff(request, None, window, cx)
    }

    /// The same, shown only if `guard` says its ask still stands as it lands.
    fn request_guarded_diff(
        &mut self,
        mut request: Request,
        guard: Option<Guard>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = (request.replaces.is_none())
            .then(|| self.open_diff(&request.path, request.base, cx))
            .flatten();
        // One comparing with nothing, asked for from an editor whose buffer
        // is the file's: rebuilt around that buffer.
        if let Some((diff, true)) = &open
            && request.live
            && self.file_model(&request.path, cx).is_some()
            && let Some(read) = start_read(&diff.downgrade(), cx)
        {
            request.replaces = Some((diff.downgrade(), read));
        } else if let Some((diff, _)) = open {
            self.show_diff(&diff, window, cx);
            if let Some(then) = request.then {
                reveal_change(&diff, then, window, cx);
                // And in the diff that replaces it, if it's out of date.
                if let Some(compared) = compared(&diff.downgrade(), cx) {
                    compared.reveal = Some(then);
                }
            }
            // Rebuilt if the file was deleted or restored since.
            return self.read_git_sides(vec![diff.downgrade()], cx);
        }
        let replaced = (request.replaces.as_ref())
            .and_then(|(old, _)| old.upgrade())
            .and_then(|old| self.diff_tab(&old, cx));
        let pane = replaced.map_or_else(|| self.active.clone(), |(pane, _)| pane);
        let mut landing = self.landing(&pane, request.asked, cx);
        landing.guard = guard;
        let staged = request.base == Base::Staged;
        // The open buffer, else the file as it is on disk; neither for the
        // staged text, whose sides are both git's.
        let model = (!staged)
            .then(|| self.file_model(&request.path, cx))
            .flatten();
        let read_disk = model.is_none() && !staged;
        let (base, at, deleted) = (request.base, request.path.clone(), request.deleted);
        // An editor's buffer is there whether or not its file is.
        let live = request.live && model.is_some();
        let read = cx.background_spawn(async move {
            let exists = staged || live || deleted.map_or_else(|| at.exists(), |deleted| !deleted);
            let disk = match exists && read_disk {
                true => Some(read_file(&at)?),
                false => None,
            };
            let index = match staged {
                true => Some(staged_text(Base::Index, &at)?),
                false => None,
            };
            let text = match staged {
                true => staged_text(base, &at)?,
                false => git_text(base, &at)?,
            };
            Ok((text, exists, disk, index))
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let read = read.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.land_git_diff(request, read, model, landing, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// Opens the diff `request` asked for, or rebuilds the one it replaces,
    /// its sides as read. A rebuild superseded by a later read, or whose
    /// tab closed meanwhile, lands nowhere; one whose working side has
    /// unsaved changes by now keeps it, taking the git text read.
    fn land_git_diff(
        &mut self,
        request: Request,
        read: GitRead,
        model: Option<EditorModel>,
        landing: Landing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Asked for by something that has moved on: nothing opens.
        if (landing.guard.clone()).is_some_and(|guard| !guard(self, cx)) {
            return;
        }
        let (text, exists, disk, index) = match read {
            Ok(read) => read,
            Err(answer) if request.asked => return self.answer_git_op(&request.op, answer, cx),
            Err(_) => return,
        };
        let deleted = !exists;
        let mut then = request.then;
        // Before a replaced tab's removal counts as a navigation.
        let show = self.may_show(&landing, cx);
        let old = match &request.replaces {
            Some((old, read)) => {
                let Some(old) = old.upgrade().filter(|old| self.diff_tab(old, cx).is_some()) else {
                    return;
                };
                if !is_last_read(&old.downgrade(), *read, cx) {
                    return;
                }
                if old.is_dirty(cx) {
                    land_read(&old.downgrade(), *read, &text, index.as_deref(), cx);
                    return;
                }
                then = then.or_else(|| compared(&old.downgrade(), cx)?.reveal.take());
                Some(old)
            }
            // Opened meanwhile: kept if it's still right or has unsaved
            // changes.
            None => match self.open_diff(&request.path, request.base, cx) {
                Some((diff, was)) if was == deleted || diff.is_dirty(cx) => {
                    // The last asked for wins the caret.
                    let last = (landing.serial).is_some_and(|serial| serial == state(cx).opens);
                    if show {
                        self.show_diff(&diff, window, cx);
                    }
                    if let Some(then) = then.filter(|_| last) {
                        reveal_change(&diff, then, window, cx);
                    }
                    return;
                }
                opened => opened.map(|(diff, _)| diff),
            },
        };
        let source = match exists && index.is_none() {
            true => (model.or_else(|| self.file_model(&request.path, cx))).or_else(|| {
                let buffer = loader::buffer_or_new(disk.flatten());
                Some(EditorModel::new(buffer, Some(request.path.clone())))
            }),
            false => None,
        };
        let right = match (index, source) {
            (Some(index), _) => Err(fixed_side(Base::Index.title(), &index)),
            (None, Some(source)) => Ok((WORKING_TREE.into(), source)),
            (None, None) => Err(fixed_side(WORKING_TREE, "")),
        };
        let left = fixed_side(request.base.title(), &text);
        let compared = (request.path.clone(), Some(request.base), deleted);
        let diff = self.new_diff(compared, left, right, cx);
        match &old {
            Some(old) => self.replace_diff(old, &diff, window, cx),
            None => self.place_diff(&diff, &landing.pane, cx),
        }
        if show {
            self.show_diff(&diff, window, cx);
        }
        if let Some(then) = then {
            reveal_change(&diff, then, window, cx);
        }
        // A refresh meanwhile didn't read it.
        if state(cx).refreshes != landing.refreshes {
            self.read_git_sides(vec![diff.downgrade()], cx);
        }
    }

    /// Opens a Changes panel row's diff: a staged file's HEAD against the
    /// index, else the file against the index. See the module docs.
    pub(crate) fn open_change_diff(
        &mut self,
        path: PathBuf,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let base = if staged { Base::Staged } else { Base::Index };
        let request = Request::asked(path, base, None, clicked(GitCommand::DiffIndex));
        self.request_git_diff(request, window, cx);
    }

    /// Opens `path`'s diff against HEAD as the Changes panel did before it
    /// had groups, for the tests of the HEAD diff.
    #[cfg(test)]
    pub(crate) fn open_head_diff(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = Request::asked(path, Base::Head, None, clicked(GitCommand::DiffHead));
        self.request_git_diff(request, window, cx);
    }

    /// `editor`'s change bar was clicked on the change at `rows`: opens its
    /// file against HEAD in the window `workbench` is the root of, the
    /// right caret on the change.
    pub(crate) fn change_bar_clicked(
        workbench: WeakEntity<Self>,
        editor: Entity<EditorView>,
        rows: Range<u32>,
        cx: &mut App,
    ) {
        in_window(workbench, cx, move |workbench, window, cx| {
            workbench.open_change(&editor, rows, window, cx)
        });
    }

    fn open_change(
        &mut self,
        editor: &Entity<EditorView>,
        rows: Range<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = editor.read(cx);
        let Some(path) = view.model().path().map(Path::to_owned) else {
            return;
        };
        let then = Reveal {
            row: rows.start,
            shown: view.viewport_rows().map(|(_, shown)| shown),
        };
        let op = clicked(GitCommand::DiffHead);
        let request = Request::asked(path, Base::Head, Some(then), op).live();
        self.request_git_diff(request, window, cx);
    }

    /// Reads each open git diff's git side again, and looks whether its file
    /// is there: what a git refresh calls. (Every one: staging changes no
    /// file's status, and a file deleted and restored between refreshes
    /// changes none either.)
    pub(crate) fn refresh_git_diffs(&mut self, cx: &mut Context<Self>) {
        state(cx).refreshes += 1;
        let views: Vec<WeakEntity<DiffView>> = (diff_views(cx).iter())
            .filter(|compared| compared.base.is_some())
            .map(|compared| compared.view.clone())
            .collect();
        let views = (views.into_iter())
            .filter(|view| (view.upgrade()).is_some_and(|view| self.diff_tab(&view, cx).is_some()))
            .collect();
        self.read_git_sides(views, cx);
    }

    /// Reads `views`' git sides again in the background, and looks whether
    /// each one's file is there, as one read numbered per view: only a
    /// view's last read lands. A diff whose file was deleted or restored
    /// since, its working side clean, is rebuilt; another takes the text.
    fn read_git_sides(&mut self, views: Vec<WeakEntity<DiffView>>, cx: &mut Context<Self>) {
        // Each view, its read, its file and base, and whether it was deleted.
        let reads: Vec<(WeakEntity<DiffView>, u64, PathBuf, Base, bool)> = (views.into_iter())
            .filter_map(|view| {
                let read = start_read(&view, cx)?;
                let compared = compared(&view, cx)?;
                let (path, deleted) = (compared.path.clone(), compared.deleted);
                Some((view, read, path, compared.base?, deleted))
            })
            .collect();
        if reads.is_empty() {
            return;
        }
        let files: Vec<(PathBuf, Base)> = (reads.iter())
            .map(|(_, _, path, base, _)| (path.clone(), *base))
            .collect();
        // A staged diff's sides are both git's: its file's always there.
        let found = cx.background_spawn(async move {
            (files.iter())
                .map(|(path, base)| match base {
                    Base::Staged => {
                        let index = staged_text(Base::Index, path);
                        (staged_text(*base, path), Some(index), true)
                    }
                    _ => (git_text(*base, path), None, path.exists()),
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |workbench, cx| {
            let found = found.await;
            cx.update(|cx| {
                for ((view, read, path, base, deleted), (text, index, exists)) in
                    reads.into_iter().zip(found)
                {
                    if !is_last_read(&view, read, cx) {
                        continue;
                    }
                    let clean = view.upgrade().is_some_and(|view| !view.is_dirty(cx));
                    if exists == deleted && clean {
                        let request = Request {
                            path,
                            base,
                            deleted: Some(!exists),
                            then: None,
                            op: clicked(GitCommand::DiffHead),
                            replaces: Some((view, read)),
                            asked: false,
                            live: false,
                        };
                        in_window(workbench.clone(), cx, move |workbench, window, cx| {
                            workbench.request_git_diff(request, window, cx)
                        });
                    } else {
                        // Up to date: the reveal was made in it.
                        if let Some(compared) = compared(&view, cx) {
                            compared.reveal = None;
                        }
                        let index = match index {
                            Some(Ok(index)) => Some(index),
                            Some(Err(_)) => continue,
                            None => None,
                        };
                        if let Ok(text) = text {
                            land_read(&view, read, &text, index.as_deref(), cx);
                        }
                    }
                }
            });
        })
        .detach();
    }

    /// `diff.compare_file`: the file `op` names, or else one picked, against
    /// the active file.
    fn compare_file(&mut self, op: GitOp, window: &mut Window, cx: &mut Context<Self>) {
        let path = (op.editor.as_ref()).and_then(|editor| self.op_path(editor, cx));
        let Some(path) = path else {
            return self.answer_git_op(&op, NO_FILE.to_owned(), cx);
        };
        if !op.args.is_empty() {
            let folder = (self.root.clone()).or_else(|| path.parent().map(Path::to_owned));
            return match vim_host::resolve(&op.args, folder.as_deref()) {
                Ok(other) => self.compare_with(other, path, op, window, cx),
                Err(answer) => self.answer_git_op(&op, answer, cx),
            };
        }
        let workbench = cx.weak_entity();
        let pick: Pick = Box::new(move |other, window, cx| {
            (workbench.update(cx, |workbench, cx| {
                workbench.compare_with(other, path, op, window, cx)
            }))
            .ok();
        });
        let (root, pane) = (self.root.clone(), self.active.downgrade());
        let picker = cx.new(|cx| {
            Picker::new(
                QuickOpen::new(root, pane),
                COMPARE_WITH,
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        self.dismiss_palette(window, cx);
        let previous = window.focused(cx);
        picker.update(cx, |picker, cx| {
            picker.delegate.set_pick(pick);
            picker.delegate.opened(previous, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::QuickOpen, picker, window, cx);
    }

    /// Opens `other`, read-only, against the file at `path`: its open
    /// buffer as the read lands, else the file read from disk, which is
    /// read whether or not a buffer's open as it's asked for, as that may
    /// close or move to another file meanwhile.
    fn compare_with(
        &mut self,
        other: PathBuf,
        path: PathBuf,
        op: GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let landing = self.landing(&self.active.clone(), true, cx);
        let at = other.clone();
        let source = path.clone();
        #[cfg(test)]
        let gate = super::git_stage::test_gate(cx);
        let read = cx.background_spawn(async move {
            // The source's read matters only if no buffer's open as it lands.
            let read = (read_file(&at)?, read_file(&source));
            #[cfg(test)]
            gate.await;
            Ok(read)
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let read: Result<_, String> = read.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let missing =
                        |path: &Path| format!("Can't open {}: No such file", path.display());
                    let (buffer, disk) = match read {
                        Ok((Some(buffer), disk)) => (buffer, disk),
                        Ok((None, _)) => return workbench.answer_git_op(&op, missing(&other), cx),
                        Err(answer) => return workbench.answer_git_op(&op, answer, cx),
                    };
                    // Not a split of a diff's side, whose comparison it'd share.
                    let model = match (workbench.file_model(&path, cx), disk) {
                        (Some(model), _) => model,
                        (None, Ok(Some(disk))) => EditorModel::new(disk, Some(path.clone())),
                        (None, Ok(None)) => {
                            return workbench.answer_git_op(&op, missing(&path), cx);
                        }
                        (None, Err(answer)) => return workbench.answer_git_op(&op, answer, cx),
                    };
                    let right = Ok((workbench.relative(&path).into(), model));
                    // The left keeps its own file's language, not the right's.
                    let text = buffer.text();
                    let start = text.slice_to_string(0..text.clip_offset(512, Bias::Left));
                    let language = fe_syntax::detect_language_name(Some(&other), &start);
                    let left = EditorModel::new(buffer, None);
                    let left = DiffSide::new(workbench.relative(&other), left).read_only();
                    let diff = workbench.new_diff((path, None, false), left, right, cx);
                    let left = diff.read(cx).left().clone();
                    left.update(cx, |left, cx| left.set_language(language, cx));
                    workbench.place_diff(&diff, &landing.pane, cx);
                    if workbench.may_show(&landing, cx) {
                        workbench.show_diff(&diff, window, cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// A diff view of two in-memory texts, `left` and `right` as (title,
    /// text), both read-only and highlighted in the language of
    /// `language_of`, whose name the tab bears; opened and focused in the
    /// active pane. Nothing is written and no git command runs.
    pub(crate) fn open_text_compare(
        &mut self,
        left: (gpui::SharedString, String),
        right: (gpui::SharedString, String),
        language_of: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (left, right) = (fixed_side(left.0, &left.1), fixed_side(right.0, &right.1));
        let pane = self.active.downgrade();
        let compared = (language_of.to_owned(), None, false);
        let diff = self.new_diff(compared, left, Err(right), cx);
        self.place_diff(&diff, &pane, cx);
        self.show_diff(&diff, window, cx);
    }

    /// `path` from the root, or whole if it's outside it.
    fn relative(&self, path: &Path) -> String {
        let relative = (self.root.as_deref()).and_then(|root| path.strip_prefix(root).ok());
        relative.unwrap_or(path).display().to_string()
    }

    /// `git.next_change` and `git.previous_change`: in a diff view, its
    /// next or previous hunk; in a tab, the editor's next or previous
    /// change against git, wrapping at the ends, counting from the change
    /// it went to last while the caret's still where it left it.
    fn go_to_change(
        &mut self,
        op: GitOp,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = op.editor.clone() else {
            return;
        };
        if let Some(diff) = self.diff_of(&editor) {
            return diff.update(cx, |diff, cx| match forward {
                true => diff.next_change(window, cx),
                false => diff.previous_change(window, cx),
            });
        }
        let model = editor.read(cx).model();
        let changes = model.row_changes();
        let count = changes.len();
        if count == 0 {
            return self.answer_git_op(&op, NO_CHANGES.to_owned(), cx);
        }
        let head = model.primary_selection().head();
        let row = model.text().offset_to_point(head).row;
        let id = editor.entity_id();
        let visited = (state(cx).visited.as_ref())
            .filter(|(editor, _, at, among)| *editor == id && *at == head && **among == *changes)
            .map(|(_, index, ..)| *index);
        let found = match (visited, forward) {
            (Some(index), true) => Some(index + 1).filter(|&next| next < count),
            (Some(index), false) => index.checked_sub(1),
            (None, true) => changes.iter().position(|change| change.rows.start > row),
            (None, false) => changes.iter().rposition(|change| change.rows.start < row),
        };
        let (index, wrapped) = match found {
            Some(index) => (index, None),
            None if forward => (0, Some(WRAPPED_TO_FIRST)),
            None => (count - 1, Some(WRAPPED_TO_LAST)),
        };
        let row = changes[index].rows.start;
        let head = editor.update(cx, |editor, cx| {
            let text = editor.model().text();
            let at = text.row_start(row.min(text.row_count().saturating_sub(1)));
            editor.select_and_reveal(at..at, cx);
            editor.model().primary_selection().head()
        });
        state(cx).visited = Some((id, index, head, changes.clone()));
        if let Some(wrapped) = wrapped {
            self.answer_git_op(&op, wrapped.to_owned(), cx);
        }
    }

    /// `:w` in a diff view's right side: saves the file, then with `close`
    /// closes the diff tab as `:q` does.
    fn write_side(
        &mut self,
        editor: Entity<EditorView>,
        close: Option<Entity<DiffView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A map waiting for the write goes on, or ends, once the diff is
        // closed too, with `close`, which ends it either way.
        let hold = editor.update(cx, |editor, cx| editor.hold_vim_host(cx));
        let saved = vim_host::save(&editor, NO_FILE.to_owned(), window, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            let failed = saved.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let ok = failed.is_none();
                    let closes = close.clone();
                    match (failed, close) {
                        (Some(failed), _) => message(&editor, failed, cx),
                        (None, Some(diff)) => {
                            workbench.close_diff(&diff, &editor, false, window, cx)
                        }
                        (None, None) => {}
                    }
                    // A close ends the map, as a quit ends vim.
                    if let Some(hold) = hold {
                        editor.update(cx, |editor, cx| {
                            editor.finish_vim_host(hold, ok && closes.is_none(), cx)
                        });
                    }
                })
                .ok();
        })
        .detach();
    }

    /// `:q` in a side of `diff`: closes its tab, unless the right side is
    /// dirty and no other editor shows its buffer, which gives `E37` in
    /// `editor`; `force` closes it whatever its state. A window left with
    /// no tab closes.
    fn close_diff(
        &mut self,
        diff: &Entity<DiffView>,
        editor: &Entity<EditorView>,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pane, ix)) = self.diff_tab(diff, cx) else {
            return;
        };
        if !force && diff.is_dirty(cx) && !pane.read(cx).pane().shown_elsewhere(diff, cx) {
            return message(editor, E37, cx);
        }
        pane.update(cx, |pane, cx| {
            pane.take_item(ix, window, cx);
            cx.notify();
        });
        let empty = (self.panes(cx).iter()).all(|pane| pane.read(cx).tabs().is_empty());
        if empty {
            window.remove_window();
        }
    }

    /// `:e {path}` in a side of `diff`: opens `path` in the diff's pane.
    fn edit_from_diff(
        &mut self,
        diff: &Entity<DiffView>,
        editor: &Entity<EditorView>,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pane, _)) = self.diff_tab(diff, cx) else {
            return;
        };
        let file = diff
            .read(cx)
            .right()
            .read(cx)
            .model()
            .path()
            .map(Path::to_owned);
        let folder = (self.root.clone()).or_else(|| file?.parent().map(Path::to_owned));
        let resolved = match vim_host::resolve(path, folder.as_deref()) {
            Ok(resolved) => resolved,
            Err(error) => return message(editor, error, cx),
        };
        // An open file keeps its spelling of the path, as `:e` keeps it.
        let resolved = (self.file_editors(cx).iter())
            .filter_map(|open| open.read(cx).model().path().map(Path::to_owned))
            .find(|open| same_file(open, &resolved))
            .unwrap_or(resolved);
        pane.update(cx, |pane, cx| pane.open(resolved, window, cx));
    }
}

#[cfg(test)]
#[path = "git_diff_tests.rs"]
mod git_diff_tests;
