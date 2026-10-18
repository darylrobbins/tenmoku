//! The Changes panel on the right dock: the project's files that differ
//! from git's HEAD in VS Code's groups, "Staged Changes" (how the index
//! differs from HEAD, shown only when there are any) and "Changes" (how the
//! working tree differs from the index, untracked files too, as `U`), each
//! header with its count. A file staged and changed again is in both. Each
//! row has a one-letter status, its name and its directory, sorted by path
//! within its group. A conflicted file is only in "Merge Changes", above
//! both and shown only when it has rows, still lettered `C`.
//!
//! Clicking a row, or Enter on the selected one, opens its diff in the
//! active pane (`git_diff`): a Staged row's HEAD ↔ Index, a Changes row's
//! Index ↔ Working Tree; a Merge row opens the file itself, whose editor
//! has the conflicts' bands and actions. A deleted file's row is dimmed. Alt-Enter, or the
//! row's "Open File" button, opens the file itself, which a deleted one
//! can't.
//!
//! The selected row's buttons show, and any row's under the pointer, as VS
//! Code's do: a Changes row's "Open File", "Discard Changes" and "Stage
//! Changes", a Staged row's "Open File" and "Unstage Changes", a Merge row's
//! "Open File" and "Stage Changes" (no discard). The headers' are "Stage All
//! Merge Changes" on Merge Changes, "Unstage All Changes" on Staged
//! Changes, "Discard All Changes" and "Stage All Changes" on Changes. The workbench runs them (`git_stage`),
//! asking before it discards.
//!
//! With vim on, the panel takes the keys of fugitive's `:Git` status
//! window: `j` and `k` move, `s` stages the selected file, `u` unstages it,
//! `-` stages a Changes or Merge row or unstages a Staged row, `X` discards
//! it (not a Merge row; `u` does nothing on one either),
//! and `o` opens its diff as Enter does. With vim off they do nothing.
//!
//! The workbench's [`GitState`] reads the status of the repository
//! containing the project root, its staged and unstaged halves apart, on
//! the background executor, one run at a time: when the project tree opens,
//! when the panel's first drawn unless a run has started, on `git.refresh`,
//! after staging, and as files and `.git` change. Each run, and each list
//! given by [`GitPanel::set_statuses`], has a sequence number from the
//! panel, and a run's result shows only if nothing newer has. Only files
//! under the project root are listed: the repository may be larger. Where
//! the root is in the repository is found through symlinks and `..`, so a
//! list given before that's known is kept and listed again once it is.
//!
//! The list is also the project tree's, each file's halves combined into
//! one status as [`fe_git::Repo::status`] combines them: whenever it
//! changes the panel emits [`GitPanelEvent::Listed`], and the workbench
//! hands [`GitPanel::statuses`] to the root's project panel.
//!
//! Above the list is the commit message box, an [`EditorView`] the
//! workbench gives the panel once it's first drawn (`git_commit`): not a
//! tab, so nothing saves, backs up or remembers it. It has no gutter,
//! soft-wraps at its width and grows from 3 to 10 rows with its text, as VS
//! Code's SCM input does from 1 to 10. While it's empty its placeholder
//! names the branch, VS Code's `Message (ctrl-enter to commit on "main")`.
//! Once its first row is over 50 characters, `<n>/50` shows after it,
//! muted, in the warning colour over 72; a body row (after the first blank
//! row) over 72 shows `<n>/72` in the warning colour. Below it are an
//! "Amend" checkbox and a "Commit" button ("Amend" while checked),
//! disabled while the message is blank and Amend isn't checked, and
//! "Committing…" while a commit runs. Checking Amend into an empty box
//! fills it with HEAD's message; unchecking it while that's untouched
//! empties it again. `git.commit_box.commit` (a key in the box) commits as
//! the button does.
//!
//! With vim on, the box edits in vim mode, starting in insert mode when
//! it's focused empty. The status bar's command line follows the tabs, so
//! the panel shows the box's own under it, and vim's messages there;
//! the host commands its line and keys leave (`:wq`, `ZZ`, `:q!`) go to the
//! workbench, which runs them in `git_commit`. `cc` in the panel focuses
//! the box, and `ca` does with Amend checked, as fugitive's keys do.

use std::{
    cell::RefCell,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{BufferSnapshot, Origin, Version};
use fe_command::{Invocation, Value};
use fe_editor::decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps};
use fe_git::{ChangeKind, FileStatus, GitError, Head, Repo, SplitStatus};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_text::Text;
use fe_ui::{Button, Checkbox, Icon, IconButton, ROW_HEIGHT, TextInput, Tooltip, UiTheme};
use fe_vim::{CommandLineKind, Mode};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, Modifiers,
    ParentElement as _, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, UniformListScrollHandle, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, uniform_list,
};

use crate::{
    Workbench,
    git::GitState,
    picker::{self, PaletteOp},
};

/// The git changes in the sidebar: focused when shown, as its rows take Enter
/// and the arrows.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("changes"),
    name: "Changes",
    icon: fe_ui::Icon::GitBranch,
    show_command: "git.show_changes",
    show_op: crate::dock::DockOp::ShowChanges,
    order: 40,
    always_on_rail: true,
    focus_on_show: true,
};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "git_panel";

/// The hue of added and untracked files' letters: the accent's, turned
/// green.
const ADDED_HUE: f32 = 130. / 360.;

/// What commits from the box, as its button does.
pub(crate) const COMMIT_COMMAND: &str = "git.commit_box.commit";

/// How long the first row of a message may be before its hint shows, and
/// how long any row may be before it warns: VS Code's
/// `git.inputValidationSubjectLength` and `git.inputValidationLength`.
const SUBJECT_LENGTH: usize = 50;
const LINE_LENGTH: usize = 72;

/// The fewest rows the box shows, room to start a message in.
const BOX_MIN_ROWS: u32 = 3;
/// The most rows the box grows to.
const BOX_ROWS: u32 = 10;

/// The ems the box's soft wrap leaves free, beyond its margin, for a
/// length hint after a wrapped row: its gap and up to 7 characters.
pub(crate) const HINT_ROOM: f32 = 6.;

/// The key context of the box's command line, so the vim line's keys
/// apply there: Enter submits, Escape cancels.
const LINE_CONTEXT: &str = "vim_line";
/// What Escape runs in the line.
const LINE_CANCEL: &str = "vim_line.cancel";
/// What Enter runs in the line's input.
const LINE_SUBMIT: &str = "text_input.submit";

/// The key that commits from the box, in its placeholder.
pub(crate) const COMMIT_KEY: &str = if cfg!(target_os = "macos") {
    "⌘⏎"
} else {
    "ctrl-enter"
};

/// What the placeholder says after a commit was refused for an empty
/// message.
const ENTER_A_MESSAGE: &str = "Enter a commit message";

/// What the panel asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitPanelEvent {
    /// Alt-Enter was pressed on a row, or its "Open File" button clicked:
    /// open this file in the active pane.
    OpenFile(PathBuf),
    /// The files listed, or their statuses, changed: see
    /// [`GitPanel::statuses`].
    Listed,
}

/// A group of rows, as VS Code's Source Control groups them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    /// Conflicted files, until they're staged.
    Merge,
    /// How the index differs from HEAD.
    Staged,
    /// How the working tree differs from the index, untracked files too.
    Changes,
}

impl Group {
    /// Its header's title.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Group::Merge => "Merge Changes",
            Group::Staged => "Staged Changes",
            Group::Changes => "Changes",
        }
    }
}

/// Where the project root is in its repository, once found.
#[derive(Clone, Debug)]
pub(crate) struct Located {
    workdir: PathBuf,
    /// `workdir` through symlinks and `..`.
    resolved: PathBuf,
    /// The root, relative to `workdir`.
    prefix: PathBuf,
}

/// What the panel says in place of the list, or the list.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    /// Nothing has landed yet.
    Reading,
    NotARepository,
    /// The error's message.
    Failed(String),
    Listed,
}

/// One changed file under the root, in one group.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// What a click opens: the root joined with `relative`.
    path: PathBuf,
    /// From the root, which each group is sorted by.
    relative: PathBuf,
    /// Its status in its group.
    kind: ChangeKind,
    group: Group,
}

/// One file as the project tree shows it: its path from the root and as
/// opened, and its halves combined.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Changed {
    relative: PathBuf,
    path: PathBuf,
    kind: ChangeKind,
}

/// A row of the list: a group's header, or a file, by its index in
/// `entries`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Header(Group),
    File(usize),
}

/// What a row's or a header's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    OpenFile,
    Discard,
    Stage,
    Unstage,
}

impl Action {
    /// Its tooltip on a row, and on a header.
    fn tooltip(self, header: bool) -> &'static str {
        match (self, header) {
            (Action::OpenFile, _) => "Open File",
            (Action::Discard, false) => "Discard Changes",
            (Action::Discard, true) => "Discard All Changes",
            (Action::Stage, false) => "Stage Changes",
            (Action::Stage, true) => "Stage All Changes",
            (Action::Unstage, false) => "Unstage Changes",
            (Action::Unstage, true) => "Unstage All Changes",
        }
    }

    /// Its icon: the set has no plus or undo arrow, so staging is a check
    /// and discarding an arrow back.
    fn icon(self) -> Icon {
        match self {
            Action::OpenFile => Icon::File,
            Action::Discard => Icon::ArrowLeft,
            Action::Stage => Icon::Check,
            Action::Unstage => Icon::Minus,
        }
    }

    /// Its name in its button's ID.
    fn name(self) -> &'static str {
        match self {
            Action::OpenFile => "open",
            Action::Discard => "discard",
            Action::Stage => "stage",
            Action::Unstage => "unstage",
        }
    }
}

/// The buttons a row of `group` has, in order: "Open File" only for a file
/// that's there.
fn row_actions(group: Group, deleted: bool) -> &'static [Action] {
    match (group, deleted) {
        (Group::Merge, false) => &[Action::OpenFile, Action::Stage],
        (Group::Merge, true) => &[Action::Stage],
        (Group::Changes, false) => &[Action::OpenFile, Action::Discard, Action::Stage],
        (Group::Changes, true) => &[Action::Discard, Action::Stage],
        (Group::Staged, false) => &[Action::OpenFile, Action::Unstage],
        (Group::Staged, true) => &[Action::Unstage],
    }
}

/// The buttons a header of `group` has, in order.
fn header_actions(group: Group) -> &'static [Action] {
    match group {
        Group::Merge => &[Action::Stage],
        Group::Changes => &[Action::Discard, Action::Stage],
        Group::Staged => &[Action::Unstage],
    }
}

/// What a run found: where the root is in its repository and the
/// repository's status, its halves apart, or no repository.
pub(crate) type Found = Result<Option<(Located, Vec<SplitStatus>)>, GitError>;

/// `status`'s halves combined as [`fe_git::Repo::status`] combines them: a
/// conflict, else an addition (none if the file's gone from disk again), a
/// deletion, a modification, or an untracked file.
pub(crate) fn combined(status: &SplitStatus) -> Option<ChangeKind> {
    use ChangeKind::*;
    let has = |kind| status.staged == Some(kind) || status.unstaged == Some(kind);
    if has(Conflicted) {
        Some(Conflicted)
    } else if has(Added) {
        (status.unstaged != Some(Deleted)).then_some(Added)
    } else if has(Deleted) {
        Some(Deleted)
    } else if has(Modified) {
        Some(Modified)
    } else {
        status.unstaged.filter(|kind| *kind == Untracked)
    }
}

/// The commit message box and what goes with it.
struct CommitBox {
    editor: Entity<EditorView>,
    /// Its placeholder and length hints, which its model paints.
    hints: Rc<RefCell<CommitHints>>,
    amend: bool,
    /// What the box held before checking Amend filled it, and what it was
    /// filled with: put back on unchecking if the text is still that.
    filled: Option<(String, String)>,
    /// HEAD-message reads started, the last the only one that may fill.
    fills: u64,
    /// The message being committed, and the box's version it was taken
    /// at, while a commit runs.
    committing: Option<(String, Version)>,
    /// The box's version when a commit was refused for an empty message,
    /// until it's edited.
    refused_at: Option<Version>,
    /// An explicit open (`git.commit`, `cc`) is to put the box in its
    /// opening mode once it's drawn.
    open_pending: bool,
    /// The next focus is the box's own command line handing it back, which
    /// leaves its mode be.
    line_refocus: bool,
    /// The box's vim command line's input, once made.
    line: Option<Entity<TextInput>>,
    /// The open line's prefix (`:`, `/`, `?`), while it's open.
    line_open: Option<char>,
    _subscriptions: Vec<Subscription>,
}

/// What the workbench commits: the box's message, in the repository
/// containing `root`.
pub(crate) struct CommitRequest {
    pub(crate) root: PathBuf,
    pub(crate) message: String,
    pub(crate) amend: bool,
    /// The box's version the message was taken at.
    pub(crate) version: Version,
}

/// The box's after-line text: while it's empty, its placeholder, or after
/// a commit refused for an empty message the warning asking for one; else
/// the rows over their length.
struct CommitHints {
    placeholder: String,
    refused: bool,
}

impl DecorationProvider for CommitHints {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let text = snapshot.text();
        if text.is_empty() {
            let (hint, tint) = match self.refused {
                true => (ENTER_A_MESSAGE, Tint::Warning),
                false => (self.placeholder.as_str(), Tint::Muted),
            };
            let kind = DecorationKind::AfterLine(hint.into());
            out.push(Decoration::new(0..text.row_len(0) as usize, kind, tint));
            return;
        }
        for (row, hint, tint) in length_hints(text) {
            let start = text.row_start(row);
            let row_range = start..start + text.row_len(row) as usize;
            if overlaps(&row_range, &range) {
                let kind = DecorationKind::AfterLine(hint.into());
                out.push(Decoration::new(row_range, kind, tint));
            }
        }
    }
}

/// Each row of a message over its length, with its hint and tint: the
/// first over [`SUBJECT_LENGTH`], muted until it's over [`LINE_LENGTH`],
/// and each after the first blank row over [`LINE_LENGTH`].
fn length_hints(text: &Text) -> Vec<(u32, String, Tint)> {
    let mut hints = Vec::new();
    let mut body = false;
    for row in 0..text.row_count() {
        let start = text.row_start(row);
        let end = start + text.row_len(row) as usize;
        let chars = text.offset_to_char(end) - text.offset_to_char(start);
        if row == 0 {
            if chars > SUBJECT_LENGTH {
                let tint = if chars > LINE_LENGTH {
                    Tint::Warning
                } else {
                    Tint::Muted
                };
                hints.push((row, format!("{chars}/{SUBJECT_LENGTH}"), tint));
            }
        } else if body {
            if chars > LINE_LENGTH {
                hints.push((row, format!("{chars}/{LINE_LENGTH}"), Tint::Warning));
            }
        } else if text.row_to_string(row).trim().is_empty() {
            body = true;
        }
    }
    hints
}

/// The kind of command line `editor`'s vim has open, if any.
fn open_line(editor: &EditorView) -> Option<CommandLineKind> {
    match editor.vim_state()?.mode() {
        Mode::CmdLine(kind) => Some(kind),
        _ => None,
    }
}

/// `editor`'s text.
fn box_text(editor: &EditorView) -> String {
    editor.model().text().to_string()
}

/// Replaces `editor`'s text with `text`, as an undo step of its own.
pub(crate) fn set_box_text(editor: &Entity<EditorView>, text: &str, cx: &mut App) {
    editor.update(cx, |view, cx| {
        let len = view.model().text().len();
        let mut buffer = view.model_mut().buffer_mut();
        buffer.with_transaction_aside(|buffer| buffer.edit([(0..len, text)], Origin::User));
        drop(buffer);
        cx.notify();
    });
}

pub struct GitPanel {
    root: Option<PathBuf>,
    /// What reads the status.
    git: WeakEntity<GitState>,
    /// Where the root is in its repository, once found.
    located: Option<Located>,
    status: Status,
    /// The statuses `entries` were listed from, to list again once the
    /// root is located.
    listed: Vec<SplitStatus>,
    /// The Merge group's files, then the Staged group's, then the Changes
    /// group's.
    entries: Vec<Entry>,
    /// How many of `entries` are conflicted.
    merge: usize,
    /// How many of `entries` are staged.
    staged: usize,
    /// Each listed file as the project tree shows it, sorted by path.
    files: Vec<Changed>,
    /// The selected file's index in `entries`.
    selected: Option<usize>,
    /// Whether a run has started or been asked for: the first is when the
    /// project tree opens, or when the panel's first drawn if that's
    /// sooner or files changed before.
    read: bool,
    /// The last run started's, or list given's, sequence number.
    started: u64,
    /// The sequence number of the list shown.
    shown: u64,
    /// The sequence number of the run started and not landed, if any.
    running: Option<u64>,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    focus: FocusHandle,
    /// The commit message box, once the workbench has given it.
    commit: Option<CommitBox>,
    /// Whether the workbench has been asked for the box.
    box_asked: bool,
    /// Where HEAD points, for the box's placeholder, once read.
    head: Option<Head>,
    /// Reads of where HEAD points started, the last the only one shown.
    head_reads: u64,
    /// With vim on, `c` was pressed: `c` or `a` follows.
    pending_c: bool,
    #[cfg(test)]
    test: std::rc::Rc<std::cell::RefCell<TestHooks>>,
}

impl GitPanel {
    /// A panel for the repository containing `root`, if any. Nothing runs
    /// until it's drawn or refreshed, and nothing is read until the
    /// workbench connects its git state.
    pub fn new(root: Option<PathBuf>, ui: UiTheme, cx: &mut App) -> Self {
        let status = match root {
            Some(_) => Status::Reading,
            None => Status::NotARepository,
        };
        Self {
            root,
            git: WeakEntity::new_invalid(),
            located: None,
            status,
            listed: Vec::new(),
            entries: Vec::new(),
            merge: 0,
            staged: 0,
            files: Vec::new(),
            selected: None,
            read: false,
            started: 0,
            shown: 0,
            running: None,
            scroll: UniformListScrollHandle::new(),
            ui,
            focus: cx.focus_handle(),
            commit: None,
            box_asked: false,
            head: None,
            head_reads: 0,
            pending_c: false,
            #[cfg(test)]
            test: Default::default(),
        }
    }

    /// Reads through `git` from now on.
    pub(crate) fn connect(&mut self, git: WeakEntity<GitState>) {
        self.git = git;
    }

    /// Reads the status again, in the background: now if none is running,
    /// else once more after it lands. With no folder open there's nothing
    /// to read.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.root.is_none() {
            return;
        }
        self.read = true;
        if let Some(git) = self.git.upgrade() {
            git.update(cx, GitState::refresh);
        }
    }

    /// A run starts: its sequence number.
    pub(crate) fn begin(&mut self) -> u64 {
        self.read = true;
        self.started += 1;
        self.running = Some(self.started);
        self.started
    }

    /// Shows run `seq`'s result unless something newer has shown. Where
    /// it found the root is kept even so, and the newer list listed again
    /// where the root is in it.
    pub(crate) fn landed(&mut self, seq: u64, found: Found, cx: &mut Context<Self>) {
        self.running.take_if(|running| *running == seq);
        if seq <= self.shown {
            if let Ok(Some((located, _))) = found
                && self.located.is_none()
            {
                self.located = Some(located);
                if self.status == Status::Listed {
                    let listed = std::mem::take(&mut self.listed);
                    self.show(self.shown, listed, cx);
                    cx.notify();
                }
            }
            return;
        }
        match found {
            Ok(Some((located, statuses))) => {
                self.located = Some(located);
                self.show(seq, statuses, cx);
                self.read_head(cx);
            }
            Ok(None) => {
                self.located = None;
                self.shown = seq;
                self.status = Status::NotARepository;
                self.clear(cx);
            }
            Err(error) => {
                self.shown = seq;
                self.status = Status::Failed(error.to_string());
                self.clear(cx);
            }
        }
        cx.notify();
    }

    /// Lists nothing, saying so if something was listed.
    fn clear(&mut self, cx: &mut Context<Self>) {
        self.listed.clear();
        self.selected = None;
        self.merge = 0;
        self.staged = 0;
        self.files.clear();
        if !self.entries.is_empty() {
            self.entries.clear();
            cx.emit(GitPanelEvent::Listed);
        }
    }

    /// Replaces the list with `statuses`, the repository's, each a change
    /// not staged, as of now: a run in flight lands unshown, though the
    /// repository it finds is kept. Before the root is located, those of
    /// `statuses` under it as spelled are listed; the rest are once it is.
    pub fn set_statuses(&mut self, statuses: Vec<FileStatus>, cx: &mut Context<Self>) {
        let statuses = (statuses.into_iter())
            .map(|status| SplitStatus {
                path: status.path,
                staged: None,
                unstaged: Some(status.kind),
            })
            .collect();
        self.started += 1;
        self.show(self.started, statuses, cx);
        cx.notify();
    }

    /// Each listed file's path from the root, and its status, sorted by
    /// path.
    pub(crate) fn statuses(&self) -> impl Iterator<Item = (&Path, ChangeKind)> {
        (self.files.iter()).map(|file| (file.relative.as_path(), file.kind))
    }

    /// Each listed file's path as opened, and its status.
    pub(crate) fn changed_paths(&self) -> impl Iterator<Item = (&Path, ChangeKind)> {
        (self.files.iter()).map(|file| (file.path.as_path(), file.kind))
    }

    /// Lists those of `statuses` under the root in their groups, each
    /// sorted by path from it, and says so if that changed what's listed.
    /// The selected file stays selected if it's still in its group; else
    /// the row now where it was in its group is, as after staging it; else
    /// the file in its other group.
    fn show(&mut self, seq: u64, statuses: Vec<SplitStatus>, cx: &mut Context<Self>) {
        let selected = (self.selected).and_then(|ix| {
            let entry = self.entries.get(ix)?;
            let start = self.group_range(entry.group).start;
            Some((entry.path.clone(), entry.group, ix - start))
        });
        self.shown = seq;
        let Some(root) = &self.root else {
            return;
        };
        self.status = Status::Listed;
        let located = self.located.as_ref();
        let (mut merge, mut staged, mut changes, mut files) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for status in &statuses {
            let Some(relative) = relative_to(root, located, &status.path) else {
                continue;
            };
            let path = root.join(&relative);
            let entry = |kind, group| Entry {
                path: path.clone(),
                relative: relative.clone(),
                kind,
                group,
            };
            let combined = combined(status);
            match combined {
                Some(ChangeKind::Conflicted) => {
                    merge.push(entry(ChangeKind::Conflicted, Group::Merge))
                }
                _ => {
                    staged.extend(status.staged.map(|kind| entry(kind, Group::Staged)));
                    changes.extend(status.unstaged.map(|kind| entry(kind, Group::Changes)));
                }
            }
            if let Some(kind) = combined {
                files.push(Changed {
                    relative,
                    path,
                    kind,
                });
            }
        }
        merge.sort_by(|a, b| a.relative.cmp(&b.relative));
        staged.sort_by(|a, b| a.relative.cmp(&b.relative));
        changes.sort_by(|a, b| a.relative.cmp(&b.relative));
        files.sort_by(|a, b| a.relative.cmp(&b.relative));
        let changed = merge.len() + staged.len() + changes.len() != self.entries.len()
            || (merge.iter().chain(&staged).chain(&changes)).ne(&self.entries)
            || files != self.files;
        self.merge = merge.len();
        self.staged = staged.len();
        self.entries = (merge.into_iter().chain(staged).chain(changes)).collect();
        self.selected = selected.and_then(|(path, group, at)| {
            let range = self.group_range(group);
            let same = |entry: &Entry| entry.path == path;
            (self.entries[range.clone()].iter().position(same))
                .map(|ix| range.start + ix)
                .or_else(|| (!range.is_empty()).then(|| (range.start + at).min(range.end - 1)))
                .or_else(|| self.entries.iter().position(same))
        });
        self.files = files;
        self.listed = statuses;
        if changed {
            cx.emit(GitPanelEvent::Listed);
        }
    }

    /// The indices in `entries` of `group`'s files.
    fn group_range(&self, group: Group) -> Range<usize> {
        match group {
            Group::Merge => 0..self.merge,
            Group::Staged => self.merge..self.merge + self.staged,
            Group::Changes => self.merge + self.staged..self.entries.len(),
        }
    }

    /// The groups with a header, in order: Changes whenever anything is
    /// listed, the others only with rows.
    fn headed(&self) -> impl Iterator<Item = Group> + use<> {
        let (merge, staged) = (self.merge > 0, self.staged > 0);
        let any = !self.entries.is_empty();
        [
            (Group::Merge, merge),
            (Group::Staged, staged),
            (Group::Changes, any),
        ]
        .into_iter()
        .filter_map(|(group, shown)| shown.then_some(group))
    }

    /// How many rows the list has: each file's and each group's header,
    /// Staged Changes only if it has any, none if nothing's listed.
    fn row_count(&self) -> usize {
        match self.entries.is_empty() {
            true => 0,
            false => self.entries.len() + self.headed().count(),
        }
    }

    /// The list's row `at`.
    fn row(&self, at: usize) -> Option<Row> {
        if at >= self.row_count() {
            return None;
        }
        let mut at = at;
        for group in self.headed() {
            if at == 0 {
                return Some(Row::Header(group));
            }
            let range = self.group_range(group);
            if at <= range.len() {
                return Some(Row::File(range.start + at - 1));
            }
            at -= range.len() + 1;
        }
        None
    }

    /// The list's row of file `ix`.
    fn row_of(&self, ix: usize) -> usize {
        let group = (self.entries.get(ix)).map_or(Group::Changes, |entry| entry.group);
        ix + self.headed().take_while(|g| *g <= group).count()
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected?)
    }

    /// If the panel has focus, its selected file, if any: what
    /// `git.stage_file`, `git.unstage_file` and `git.discard_file` act on
    /// then. `None` if it hasn't focus.
    pub(crate) fn focused_selection(&self, window: &Window, cx: &App) -> Option<Option<PathBuf>> {
        let focused = self.focus.contains_focused(window, cx) && !self.box_focused(window, cx);
        focused.then(|| self.selected_entry().map(|entry| entry.path.clone()))
    }

    /// Whether the commit box, or its command line, has focus.
    fn box_focused(&self, window: &Window, cx: &App) -> bool {
        self.commit.as_ref().is_some_and(|commit| {
            commit.editor.focus_handle(cx).contains_focused(window, cx)
                || (commit.line.as_ref())
                    .is_some_and(|line| line.focus_handle(cx).contains_focused(window, cx))
        })
    }

    /// What shows in place of the list, if anything.
    fn message(&self) -> Option<String> {
        Some(match &self.status {
            Status::Reading => return None,
            Status::NotARepository => "Not a git repository".into(),
            Status::Failed(message) => message.clone(),
            Status::Listed if self.entries.is_empty() => "No changes".into(),
            Status::Listed => return None,
        })
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll
            .scroll_to_item(self.row_of(ix), ScrollStrategy::Nearest);
        cx.notify();
    }

    /// The workbench the git state gives bases to, which runs what the
    /// rows ask.
    fn workbench(&self, cx: &App) -> Option<Entity<Workbench>> {
        self.git.upgrade()?.read(cx).workbench().upgrade()
    }

    /// Selects file `ix` and opens its diff: a staged file's HEAD ↔ Index,
    /// a changed one's Index ↔ Working Tree.
    fn open_diff(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(ix) else {
            return;
        };
        // Whether it's on disk is looked at as it opens: a staged deletion
        // may have left it there.
        let (path, staged) = (entry.path.clone(), entry.group == Group::Staged);
        if entry.group == Group::Merge {
            return self.open_file(ix, cx);
        }
        self.select(ix, cx);
        if let Some(workbench) = self.workbench(cx) {
            workbench.update(cx, |workbench, cx| {
                workbench.open_change_diff(path, staged, window, cx)
            });
        }
    }

    /// Selects file `ix` and opens it, unless it's deleted.
    fn open_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(ix) else {
            return;
        };
        let open = (entry.kind != ChangeKind::Deleted).then(|| entry.path.clone());
        self.select(ix, cx);
        if let Some(path) = open {
            cx.emit(GitPanelEvent::OpenFile(path));
        }
    }

    /// Stages `paths`, or with `stage` false unstages them, through the
    /// workbench.
    fn stage(&mut self, paths: Vec<PathBuf>, stage: bool, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        if let Some(workbench) = self.workbench(cx) {
            workbench.update(cx, |workbench, cx| workbench.stage_files(paths, stage, cx));
        }
    }

    /// Stages the conflicted `paths`, through the workbench, which asks
    /// first about those still holding conflict markers.
    fn stage_merge(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        if let Some(workbench) = self.workbench(cx) {
            workbench.update(cx, |workbench, cx| {
                workbench.stage_merge_files(paths, window, cx)
            });
        }
    }

    /// Discards the changes to `paths`, through the workbench, which asks
    /// first.
    fn discard(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        if let Some(workbench) = self.workbench(cx) {
            workbench.update(cx, |workbench, cx| {
                workbench.discard_files(paths, window, cx)
            });
        }
    }

    /// Runs `action` on file `ix`.
    fn act(&mut self, ix: usize, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(ix) else {
            return;
        };
        let path = entry.path.clone();
        let merge = entry.group == Group::Merge;
        match action {
            Action::OpenFile => self.open_file(ix, cx),
            Action::Discard if merge => {}
            Action::Unstage if merge => {}
            Action::Discard => self.discard(vec![path], window, cx),
            Action::Stage if merge => self.stage_merge(vec![path], window, cx),
            Action::Stage => self.stage(vec![path], true, cx),
            Action::Unstage => self.stage(vec![path], false, cx),
        }
    }

    /// Runs `action` on every file of `group`.
    fn act_on_all(
        &mut self,
        group: Group,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<PathBuf> = (self.entries[self.group_range(group)].iter())
            .map(|entry| entry.path.clone())
            .collect();
        match action {
            Action::OpenFile => {}
            Action::Discard | Action::Unstage if group == Group::Merge => {}
            Action::Discard => self.discard(paths, window, cx),
            Action::Stage if group == Group::Merge => self.stage_merge(paths, window, cx),
            Action::Stage => self.stage(paths, true, cx),
            Action::Unstage => self.stage(paths, false, cx),
        }
    }

    /// Up and down move the selection, the first press selecting the first
    /// row; Enter opens the selected row's diff, and Alt-Enter its file.
    /// With vim on, so do fugitive's keys: see the module docs. Keys typed
    /// in the commit box are the box's.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.box_focused(window, cx) {
            return;
        }
        let keystroke = &event.keystroke;
        if fe_gpui::vim_enabled(cx) && !keystroke.modifiers.modified() {
            let key = keystroke.key.as_str();
            if std::mem::take(&mut self.pending_c) && matches!(key, "c" | "a") {
                self.ask_to_focus_box(key == "a", window, cx);
                cx.stop_propagation();
                return;
            }
            if key == "c" {
                self.pending_c = true;
                cx.stop_propagation();
                return;
            }
        }
        if keystroke.key == "enter" && keystroke.modifiers == Modifiers::alt() {
            if let Some(ix) = self.selected {
                self.open_file(ix, cx);
                cx.stop_propagation();
            }
            return;
        }
        if self.entries.is_empty() {
            return;
        }
        let vim = fe_gpui::vim_enabled(cx);
        let key = match (keystroke.modifiers, keystroke.key.as_str()) {
            (modifiers, key) if !modifiers.modified() => key,
            (modifiers, "x") if vim && modifiers == Modifiers::shift() => "X",
            _ => return,
        };
        let last = self.entries.len() - 1;
        match key {
            "up" => {
                let ix = self.selected.map_or(0, |ix| ix.saturating_sub(1));
                self.select(ix, cx);
            }
            "down" => {
                let ix = self.selected.map_or(0, |ix| (ix + 1).min(last));
                self.select(ix, cx);
            }
            "enter" => match self.selected {
                Some(ix) => self.open_diff(ix, window, cx),
                None => return,
            },
            "k" if vim => {
                let ix = self.selected.map_or(0, |ix| ix.saturating_sub(1));
                self.select(ix, cx);
            }
            "j" if vim => {
                let ix = self.selected.map_or(0, |ix| (ix + 1).min(last));
                self.select(ix, cx);
            }
            "o" | "s" | "u" | "-" | "X" if vim => {
                let Some(ix) = self.selected else {
                    return;
                };
                let staged = self.entries[ix].group == Group::Staged;
                match key {
                    "o" => self.open_diff(ix, window, cx),
                    "s" => self.act(ix, Action::Stage, window, cx),
                    "u" => self.act(ix, Action::Unstage, window, cx),
                    "-" if staged => self.act(ix, Action::Unstage, window, cx),
                    "-" => self.act(ix, Action::Stage, window, cx),
                    _ => self.act(ix, Action::Discard, window, cx),
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The ID of a button for `action` on row `at` of the list shown.
    fn button_id(&self, at: usize, action: Action) -> String {
        format!("change-{}-{}-{at}", self.shown, action.name())
    }

    /// `actions`' buttons on row `at`: on file `ix`'s, or on a header's.
    fn buttons(
        &self,
        at: usize,
        target: Row,
        actions: &[Action],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let ui = self.ui;
        let header = matches!(target, Row::Header(_));
        let panel = cx.weak_entity();
        div()
            .flex_none()
            .flex()
            .items_center()
            .children(actions.iter().map(|&action| {
                let panel = panel.clone();
                let tooltip = match (target, action) {
                    (Row::Header(Group::Merge), Action::Stage) => "Stage All Merge Changes",
                    _ => action.tooltip(header),
                };
                IconButton::new(self.button_id(at, action), action.icon(), &ui)
                    .tooltip(move |_, cx| {
                        cx.new(|_| Tooltip::new(tooltip, None::<&str>, &ui)).into()
                    })
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        (panel.update(cx, |panel, cx| match target {
                            Row::File(ix) => panel.act(ix, action, window, cx),
                            Row::Header(group) => panel.act_on_all(group, action, window, cx),
                        }))
                        .ok();
                    })
            }))
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let ui = self.ui;
        let selected_bg = if self.focus.contains_focused(window, cx) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        // Each list shown has its own IDs, so a row pressed before the list
        // changed isn't clicked as the row that replaced it.
        let name = SharedString::from(format!("change-{}", self.shown));
        range
            .filter_map(|at| {
                let row = self.row(at)?;
                // Its buttons show under the pointer, and on the selected row.
                let hover = SharedString::from(format!("{name}-row-{at}"));
                let base = div()
                    .id(ElementId::NamedInteger(name.clone(), at as u64))
                    .group(hover.clone())
                    .h(ROW_HEIGHT)
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap();
                let element = match row {
                    Row::Header(group) => {
                        let count = self.group_range(group).len();
                        let buttons = self.buttons(at, row, header_actions(group), cx);
                        base.font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(div().flex_1().min_w_0().child(group.title()))
                            .child(
                                div()
                                    .invisible()
                                    .group_hover(hover, |style| style.visible())
                                    .child(buttons),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .px_1()
                                    .rounded_md()
                                    .bg(ui.row_hover)
                                    .text_color(ui.muted_text)
                                    .child(count.to_string()),
                            )
                    }
                    Row::File(ix) => {
                        let entry = self.entries.get(ix)?;
                        let name_text = entry
                            .relative
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let directory = entry
                            .relative
                            .parent()
                            .map(|dir| dir.display().to_string())
                            .unwrap_or_default();
                        let deleted = entry.kind == ChangeKind::Deleted;
                        let selected = self.selected == Some(ix);
                        let (kind, group) = (entry.kind, entry.group);
                        let buttons = self.buttons(at, row, row_actions(group, deleted), cx);
                        base.when(deleted, |row| row.opacity(0.5))
                            .when(selected, |row| row.bg(selected_bg))
                            .hover(move |row| row.bg(ui.row_hover))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.open_diff(ix, window, cx)
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(10.))
                                    .text_color(letter_color(kind, &ui))
                                    .child(letter(kind)),
                            )
                            .child(div().flex_none().child(name_text))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(ui.muted_text)
                                    .child(directory),
                            )
                            .child(
                                div()
                                    .when(!selected, |buttons| {
                                        buttons
                                            .invisible()
                                            .group_hover(hover, |style| style.visible())
                                    })
                                    .child(buttons),
                            )
                    }
                };
                Some(element.into_any_element())
            })
            .collect()
    }
}

/// The commit message box: see the module docs.
impl GitPanel {
    /// Whether the root's known not to be in a repository, or there's no
    /// root.
    pub(crate) fn not_a_repository(&self) -> bool {
        self.status == Status::NotARepository
    }

    /// The commit message box, once the workbench has given it.
    pub(crate) fn commit_box(&self) -> Option<&Entity<EditorView>> {
        self.commit.as_ref().map(|commit| &commit.editor)
    }

    /// Takes `editor`, the commit box the workbench made, and shows it with
    /// its placeholder and length hints.
    pub(crate) fn set_commit_box(
        &mut self,
        editor: Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hints = Rc::new(RefCell::new(CommitHints {
            placeholder: placeholder(None),
            refused: false,
        }));
        let provider: Rc<RefCell<dyn DecorationProvider>> = hints.clone();
        editor.update(cx, |editor, cx| {
            editor.model_mut().add_decoration_provider(provider);
            editor.set_wrap_reserve(HINT_ROOM, cx);
        });
        let focus = editor.focus_handle(cx);
        let subscriptions = vec![
            cx.observe_in(&editor, window, Self::box_changed),
            cx.on_focus(&focus, window, Self::box_focused_in),
        ];
        self.commit = Some(CommitBox {
            editor,
            hints,
            amend: false,
            filled: None,
            fills: 0,
            committing: None,
            refused_at: None,
            open_pending: false,
            line_refocus: false,
            line: None,
            line_open: None,
            _subscriptions: subscriptions,
        });
        self.read_head(cx);
        cx.notify();
    }

    /// Asks the workbench for the commit box, once: it makes the box with
    /// the editors' theme and runs what's typed in it.
    fn ask_for_box(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit.is_some() || self.box_asked || self.root.is_none() {
            return;
        }
        if self.workbench(cx).is_none() {
            return;
        }
        self.box_asked = true;
        let panel = cx.entity();
        self.with_workbench(window, cx, move |workbench, window, cx| {
            workbench.attach_commit_box(&panel, window, cx)
        });
    }

    /// Runs `run` on the workbench once the panel's update is over: the
    /// workbench reads the dock's panels as it runs.
    fn with_workbench(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
        run: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
    ) {
        let Some(workbench) = self.workbench(cx) else {
            return;
        };
        window.defer(cx, move |window, cx| {
            workbench.update(cx, |workbench, cx| run(workbench, window, cx));
        });
    }

    /// Has the workbench focus the box, with `amend` Amend checked: `cc`
    /// and `ca`.
    fn ask_to_focus_box(&mut self, amend: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.with_workbench(window, cx, move |workbench, window, cx| {
            workbench.focus_commit_box(amend, window, cx)
        });
    }

    /// Focuses the box in its opening mode: see [`Self::open_mode`]. One
    /// not drawn yet takes it once it's focused in the frame that draws it.
    pub(crate) fn focus_commit_box(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        commit.open_pending = true;
        window.focus(&commit.editor.focus_handle(cx), cx);
        self.open_mode(window, cx);
    }

    /// Has the workbench commit the box's message: the button and
    /// [`COMMIT_COMMAND`].
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_workbench(window, cx, |workbench, window, cx| {
            workbench.commit_from_box(window, cx)
        });
    }

    /// Where the repository is: the working tree the root's in, once
    /// found, else the root.
    fn repo_root(&self) -> Option<PathBuf> {
        (self.located.as_ref())
            .map(|located| located.workdir.clone())
            .or_else(|| self.root.clone())
    }

    /// Starts committing the box's message: what to commit, unless a
    /// commit is running or there's no box.
    pub(crate) fn begin_commit(&mut self, cx: &mut Context<Self>) -> Option<CommitRequest> {
        let root = self.repo_root()?;
        let commit = self
            .commit
            .as_mut()
            .filter(|commit| commit.committing.is_none())?;
        let editor = commit.editor.read(cx);
        let message = box_text(editor);
        let version = editor.model().snapshot().version();
        commit.committing = Some((message.clone(), version));
        cx.notify();
        Some(CommitRequest {
            root,
            message,
            amend: commit.amend,
            version,
        })
    }

    /// Commits `request` again, which was refused with nothing staged and
    /// is to stage everything first: whether it may, as no commit is
    /// running. What was typed since it was submitted stays once it's made.
    pub(crate) fn resume_commit(
        &mut self,
        request: &CommitRequest,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(commit) = self
            .commit
            .as_mut()
            .filter(|commit| commit.committing.is_none())
        else {
            return false;
        };
        commit.committing = Some((request.message.clone(), request.version));
        cx.notify();
        true
    }

    /// The commit started has been made: Amend is unchecked, the box
    /// empties unless it was edited meanwhile, which keeps what was typed,
    /// and where HEAD points is read again.
    pub(crate) fn committed(&mut self, cx: &mut Context<Self>) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        let committed = commit.committing.take();
        commit.refused_at = None;
        if committed.is_some_and(|(message, _)| message == box_text(commit.editor.read(cx))) {
            self.clear_box(cx);
        } else {
            commit.amend = false;
            commit.filled = None;
            commit.fills += 1;
            cx.notify();
        }
        self.sync_hints(cx);
        self.read_head(cx);
    }

    /// The commit started was refused, `empty` if for an empty message:
    /// unless the box was edited meanwhile, it then asks for one until it
    /// is. The message stays.
    pub(crate) fn commit_refused(&mut self, empty: bool, cx: &mut Context<Self>) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        let submitted = commit.committing.take().map(|(_, version)| version);
        let version = commit.editor.read(cx).model().snapshot().version();
        if empty && submitted == Some(version) {
            commit.refused_at = Some(version);
        }
        self.sync_hints(cx);
        cx.notify();
    }

    /// Empties the box and unchecks Amend: what a commit and fugitive's
    /// abort (`:q!`) do.
    pub(crate) fn clear_box(&mut self, cx: &mut Context<Self>) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        commit.amend = false;
        commit.filled = None;
        commit.fills += 1;
        let editor = commit.editor.clone();
        set_box_text(&editor, "", cx);
        cx.notify();
    }

    /// Checks or unchecks Amend. Checking it into an empty box fills the
    /// box with HEAD's message, read in the background; unchecking it puts
    /// back what the box held if that's still untouched.
    pub(crate) fn set_amend(&mut self, amend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.repo_root();
        let Some(commit) = &mut self.commit else {
            return;
        };
        if commit.amend == amend {
            return;
        }
        commit.amend = amend;
        commit.fills += 1;
        cx.notify();
        let editor = commit.editor.clone();
        let text = box_text(editor.read(cx));
        if !amend {
            if let Some((before, filled)) = commit.filled.take()
                && text == filled
            {
                set_box_text(&editor, &before, cx);
            }
            return;
        }
        let Some(root) = root.filter(|_| text.trim().is_empty()) else {
            return;
        };
        let fill = commit.fills;
        let read = cx.background_spawn(async move {
            let repo = Repo::discover(&root).ok().flatten()?;
            repo.head_message().ok().flatten()
        });
        cx.spawn_in(window, async move |panel, cx| {
            let Some(message) = read.await else {
                return;
            };
            (panel.update_in(cx, |panel, window, cx| {
                panel.fill(fill, message, window, cx)
            }))
            .ok();
        })
        .detach();
    }

    /// Fills the box with `message`, HEAD's, if Amend's still checked as
    /// fill `fill` was started and the box is still empty. A focused box
    /// then takes the mode a box with a message opens in.
    fn fill(&mut self, fill: u64, message: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        let before = box_text(commit.editor.read(cx));
        if commit.fills != fill || !commit.amend || !before.trim().is_empty() {
            return;
        }
        let message = message.trim_end().to_owned();
        let editor = commit.editor.clone();
        set_box_text(&editor, &message, cx);
        commit.filled = Some((before, message));
        if editor.focus_handle(cx).is_focused(window) {
            self.open_mode(window, cx);
        }
    }

    /// Reads where HEAD points in the background, for the placeholder.
    fn read_head(&mut self, cx: &mut Context<Self>) {
        if self.commit.is_none() {
            return;
        }
        let Some(root) = self.repo_root() else {
            return;
        };
        self.head_reads += 1;
        let read = self.head_reads;
        let head = cx.background_spawn(async move {
            Repo::discover(&root).ok().flatten()?.current_branch().ok()
        });
        cx.spawn(async move |panel, cx| {
            let head = head.await;
            (panel.update(cx, |panel, cx| {
                if panel.head_reads == read {
                    panel.head = head;
                    panel.sync_hints(cx);
                }
            }))
            .ok();
        })
        .detach();
    }

    /// Has the box's placeholder name where HEAD points, and the box ask
    /// for a message while an empty one's refusal stands.
    fn sync_hints(&mut self, cx: &mut Context<Self>) {
        let Some(commit) = &self.commit else {
            return;
        };
        let placeholder = placeholder(self.head.as_ref());
        let refused = commit.refused_at.is_some();
        let mut hints = commit.hints.borrow_mut();
        if hints.placeholder != placeholder || hints.refused != refused {
            *hints = CommitHints {
                placeholder,
                refused,
            };
            drop(hints);
            commit.editor.update(cx, |_, cx| cx.notify());
        }
    }

    /// The box notified: a host command its vim left (`:wq`, `ZZ`) goes to
    /// the workbench, its command line opens or closes, a refused empty
    /// message is forgotten once the box is edited, and the panel redraws
    /// for its height and button.
    fn box_changed(
        &mut self,
        editor: Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = editor.update(cx, |editor, _| editor.take_vim_host_command());
        if let Some(command) = command {
            let editor = editor.clone();
            self.with_workbench(window, cx, move |workbench, window, cx| {
                workbench.commit_box_host_command(editor, command, window, cx)
            });
        }
        self.sync_line(&editor, window, cx);
        let version = editor.read(cx).model().snapshot().version();
        if let Some(commit) = &mut self.commit
            && commit.refused_at.is_some_and(|at| at != version)
        {
            commit.refused_at = None;
            self.sync_hints(cx);
        }
        cx.notify();
    }

    /// Focus is leaving the box for an editor (`:q!`, `:q`): the next focus
    /// isn't its command line's handing it back, whichever frame sees it.
    pub(crate) fn leave_box(&mut self) {
        if let Some(commit) = &mut self.commit {
            commit.line_refocus = false;
        }
    }

    /// The box has focus: it takes its opening mode, unless its own command
    /// line handed focus back.
    fn box_focused_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        if std::mem::take(&mut commit.line_refocus) && !commit.open_pending {
            return;
        }
        commit.open_pending = true;
        self.open_mode(window, cx);
    }

    /// With vim on, puts the box in the mode it opens in: insert if it's
    /// empty, else normal. Pending until the box is drawn to take it.
    fn open_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        let editor = commit.editor.read(cx);
        if !fe_gpui::vim_enabled(cx) || open_line(editor).is_some() {
            commit.open_pending = false;
            return;
        }
        let empty = editor.model().text().is_empty();
        let in_mode = |mode: Option<&str>| match empty {
            true => mode == Some("insert"),
            false => mode.is_none_or(|mode| mode == "normal"),
        };
        if in_mode(editor.vim_mode()) {
            commit.open_pending = false;
            return;
        }
        let invocation = match empty {
            true => Invocation::of("vim.insert")
                .expect("a valid command ID")
                .args(Value::from_iter([("at", "before")])),
            false => Invocation::of("vim.normal_mode").expect("a valid command ID"),
        };
        let focus = commit.editor.focus_handle(cx);
        // Nothing runs in a box the last frame didn't draw.
        focus.dispatch_action(&RunCommand(invocation), window, cx);
        commit.open_pending = !in_mode(commit.editor.read(cx).vim_mode());
    }

    /// Opens the box's command line when its vim has opened one, focusing
    /// it, and closes it when vim's has closed some other way.
    fn sync_line(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = open_line(editor.read(cx));
        let Some(commit) = &mut self.commit else {
            return;
        };
        match (kind, commit.line_open) {
            (Some(kind), None) => {
                let input = match &commit.line {
                    Some(input) => input.clone(),
                    None => {
                        let input = cx.new(|cx| TextInput::new("", &self.ui, cx));
                        let focus = input.focus_handle(cx);
                        (commit._subscriptions).push(cx.on_blur(
                            &focus,
                            window,
                            Self::line_blurred,
                        ));
                        commit.line = Some(input.clone());
                        input
                    }
                };
                let prefill = (editor.read(cx).vim_state())
                    .map_or("", |state| state.command_line_prefill())
                    .to_owned();
                input.update(cx, |input, cx| input.set_text(prefill, cx));
                commit.line_open = Some(match kind {
                    CommandLineKind::SearchForward => '/',
                    CommandLineKind::SearchBackward => '?',
                    CommandLineKind::Ex => ':',
                });
                window.focus(&input.focus_handle(cx), cx);
            }
            (None, Some(_)) => {
                commit.line_open = None;
                let focused = (commit.line.as_ref())
                    .is_some_and(|input| input.focus_handle(cx).is_focused(window));
                if focused {
                    window.focus(&editor.focus_handle(cx), cx);
                }
            }
            _ => {}
        }
    }

    /// Focus left the box's command line for something else: the line is
    /// cancelled, focus staying there. Not when the window goes inactive.
    fn line_blurred(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_window_active() {
            self.end_line(None, false, window, cx);
        }
    }

    /// Ends the box's command line: submits `text` in the box's vim, or
    /// cancels it with `None`, focusing the box again if `refocus`.
    fn end_line(
        &mut self,
        text: Option<String>,
        refocus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(commit) = &mut self.commit else {
            return;
        };
        // Closed first, so neither the blur nor the box's notifying reads
        // it as open.
        if commit.line_open.take().is_none() {
            return;
        }
        let editor = commit.editor.clone();
        cx.notify();
        if refocus {
            commit.line_refocus = true;
            window.focus(&editor.focus_handle(cx), cx);
        }
        if open_line(editor.read(cx)).is_none() {
            return;
        }
        let invocation = match text {
            Some(text) => Invocation::of("vim.command_line_submit")
                .expect("a valid command ID")
                .args(Value::from_iter([("text", text)])),
            None => Invocation::of("vim.command_line_cancel").expect("a valid command ID"),
        };
        // Vim's registry takes it in the box, so it goes no further.
        let focus = editor.focus_handle(cx);
        focus.dispatch_action(&RunCommand(invocation), window, cx);
    }

    /// The box's command line's commands: Enter submits it, Escape, and a
    /// backspace on an empty line, cancel it, and the input's edits edit
    /// it. Others go on.
    fn on_line_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.commit.as_ref().and_then(|commit| commit.line.clone()) else {
            return cx.propagate();
        };
        match run.0.command.as_str() {
            LINE_SUBMIT => {
                let text = input.read(cx).text().to_owned();
                return self.end_line(Some(text), true, window, cx);
            }
            LINE_CANCEL => return self.end_line(None, true, window, cx),
            _ => {}
        }
        let edit: fn(&mut TextInput, &mut Context<TextInput>) = match picker::op(run, cx) {
            Some(PaletteOp::Backspace) if input.read(cx).text().is_empty() => {
                return self.end_line(None, true, window, cx);
            }
            Some(PaletteOp::Backspace) => TextInput::backspace,
            Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
            Some(PaletteOp::Clear) => TextInput::clear,
            Some(PaletteOp::Paste) => TextInput::paste,
            _ => return cx.propagate(),
        };
        input.update(cx, edit);
    }

    /// [`COMMIT_COMMAND`] commits; others go on.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        if run.0.command.as_str() == COMMIT_COMMAND && self.commit.is_some() {
            self.commit(window, cx);
        } else {
            cx.propagate();
        }
    }

    /// The box, its command line or vim's message, and its Amend checkbox
    /// and Commit button; nothing without a repository.
    fn render_commit_box(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.status == Status::NotARepository {
            return None;
        }
        let commit = self.commit.as_ref()?;
        let ui = self.ui;
        let editor = commit.editor.read(cx);
        let theme = editor.theme();
        let warning = theme.warning;
        let line_height = (theme.font_size * theme.line_height).round();
        let rows = editor.model().display().row_count().clamp(1, BOX_ROWS);
        let height = line_height * box_rows(rows) + px(1.);
        let message = (editor.vim_state())
            .and_then(|state| state.message())
            .map(str::to_owned);
        let (label, enabled) = commit_button(commit, cx);
        let line = commit.line_open.zip(commit.line.clone());
        let (amend, committing) = (commit.amend, commit.committing.is_some());
        let (checkbox_panel, button_panel) = (cx.weak_entity(), cx.weak_entity());
        let below = match (line, message) {
            (Some((prefix, input)), _) => Some(
                div()
                    .key_context(LINE_CONTEXT)
                    .on_action(cx.listener(Self::on_line_command))
                    .debug_selector(|| "commit-line".into())
                    .flex()
                    .items_center()
                    .h(ROW_HEIGHT)
                    .child(div().flex_none().child(prefix.to_string()))
                    // The input pads its text; the line's follows the
                    // prefix, as Vim's does.
                    .child(div().flex_1().min_w_0().ml(px(-12.)).child(input))
                    .into_any_element(),
            ),
            (None, Some(message)) => {
                let error = message.starts_with('E');
                let color = if error { ui.error_text } else { ui.muted_text };
                Some(
                    div()
                        .debug_selector(|| "commit-message".into())
                        .text_color(color)
                        .child(message)
                        .into_any_element(),
                )
            }
            // Blanks or comments refused: wider than the room after a row.
            (None, None) if commit_warning(commit, cx).is_some() => Some(
                div()
                    .debug_selector(|| "commit-message".into())
                    .text_color(warning)
                    .child(ENTER_A_MESSAGE)
                    .into_any_element(),
            ),
            (None, None) => None,
        };
        let element = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_2()
            .border_b_1()
            .border_color(ui.border)
            .child(
                div()
                    .debug_selector(|| "commit-box".into())
                    .w_full()
                    .h(height)
                    .overflow_hidden()
                    .border_1()
                    .border_color(ui.border)
                    .child(commit.editor.clone()),
            )
            .children(below)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Checkbox::new("commit-amend")
                            .checked(amend)
                            .disabled(committing)
                            .label("Amend")
                            .on_change(move |checked, window, cx| {
                                (checkbox_panel
                                    .update(cx, |panel, cx| panel.set_amend(*checked, window, cx)))
                                .ok();
                            }),
                    )
                    .child(
                        Button::new("commit-button", label)
                            .primary()
                            .small()
                            .disabled(!enabled)
                            .on_click(move |_, window, cx| {
                                (button_panel.update(cx, |panel, cx| panel.commit(window, cx)))
                                    .ok();
                            }),
                    ),
            );
        Some(element.into_any_element())
    }
}

/// How many rows tall the box is to show `rows` of text unscrolled with
/// the caret on the last: an editor scrolls to keep its caret a third of
/// its height, or 3 rows if less, from the bottom edge.
/// At least [`BOX_MIN_ROWS`], and at most [`BOX_ROWS`], where a long
/// message scrolls.
fn box_rows(rows: u32) -> f32 {
    let rows = rows as f32;
    ((rows * 1.5).min(rows + 3.))
        .max(BOX_MIN_ROWS as f32)
        .min(BOX_ROWS as f32)
}

/// The warning under the box after a commit was refused for a message of
/// only blanks or comments, until the box is edited; the warning for an
/// empty box is its placeholder.
fn commit_warning(commit: &CommitBox, cx: &App) -> Option<&'static str> {
    let empty = commit.editor.read(cx).model().text().is_empty();
    (commit.refused_at.is_some() && !empty).then_some(ENTER_A_MESSAGE)
}

/// The Commit button's label, and whether it's enabled: not while the
/// message is blank and Amend isn't checked, nor while committing.
fn commit_button(commit: &CommitBox, cx: &App) -> (&'static str, bool) {
    let blank = box_text(commit.editor.read(cx)).trim().is_empty();
    let committing = commit.committing.is_some();
    let enabled = !committing && (commit.amend || !blank);
    let label = match (committing, commit.amend) {
        (true, _) => "Committing…",
        (false, true) => "Amend",
        (false, false) => "Commit",
    };
    (label, enabled)
}

/// The box's placeholder, naming where `head` points if on a branch.
fn placeholder(head: Option<&Head>) -> String {
    match head {
        Some(Head::Branch(name) | Head::Unborn(name)) => {
            format!("Message ({COMMIT_KEY} to commit on \"{name}\")")
        }
        _ => format!("Message ({COMMIT_KEY} to commit)"),
    }
}

/// Where `root` is in the working tree at `workdir`, through symlinks and
/// `..`: `None` if it isn't in it. Spelled as given if either is gone.
pub(crate) fn locate(root: &Path, workdir: &Path) -> Option<Located> {
    let (prefix, resolved) = match (root.canonicalize(), workdir.canonicalize()) {
        (Ok(root), Ok(resolved)) => (root.strip_prefix(&resolved).ok()?.to_owned(), resolved),
        _ => (
            root.strip_prefix(workdir).ok()?.to_owned(),
            workdir.to_owned(),
        ),
    };
    Some(Located {
        workdir: workdir.to_owned(),
        resolved,
        prefix,
    })
}

/// `path`, absolute, relative to `root`, if it's under it: directly, or
/// through the working tree `located` names, as found or resolved.
fn relative_to(root: &Path, located: Option<&Located>, path: &Path) -> Option<PathBuf> {
    if let Ok(relative) = path.strip_prefix(root) {
        return (!relative.as_os_str().is_empty()).then(|| relative.to_owned());
    }
    let located = located?;
    let relative = path
        .strip_prefix(&located.workdir)
        .or_else(|_| path.strip_prefix(&located.resolved))
        .ok()?
        .strip_prefix(&located.prefix)
        .ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_owned())
}

/// A status's letter in its row.
pub(crate) fn letter(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Modified => "M",
        ChangeKind::Added => "A",
        ChangeKind::Deleted => "D",
        ChangeKind::Untracked => "U",
        ChangeKind::Conflicted => "C",
    }
}

/// A status letter's colour: deletions and conflicts in the error colour,
/// modifications in the accent, additions in the accent turned green.
pub(crate) fn letter_color(kind: ChangeKind, ui: &UiTheme) -> Hsla {
    match kind {
        ChangeKind::Deleted | ChangeKind::Conflicted => ui.error_text,
        ChangeKind::Modified => ui.accent,
        ChangeKind::Added | ChangeKind::Untracked => Hsla {
            h: ADDED_HUE,
            ..ui.accent
        },
    }
}

impl EventEmitter<GitPanelEvent> for GitPanel {}

impl Focusable for GitPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Read when first shown, unless a run has started.
        if !self.read {
            self.refresh(cx);
        }
        self.ask_for_box(window, cx);
        let commit_box = self.render_commit_box(cx);
        let ui = self.ui;
        let list = uniform_list(
            "change-rows",
            self.row_count(),
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        let error = matches!(self.status, Status::Failed(_));
        div()
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::on_run_command))
            .debug_selector(|| "sidebar-panel".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(ui.border)
            .bg(ui.panel_background)
            .text_color(ui.text)
            .text_size(px(12.))
            .children(commit_box)
            .when_some(self.message(), |panel, message| {
                let color = if error { ui.error_text } else { ui.muted_text };
                panel.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_color(color)
                        .child(message),
                )
            })
            .child(list)
    }
}

/// What the tests hold and count of [`GitState`]'s status runs, which the
/// Changes panel's tests see too.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestHooks {
    /// Runs started.
    started: usize,
    /// Runs that read the status, and of them those that read it off the
    /// main thread.
    ran: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
    /// While set, runs that have read the status wait for
    /// [`GitPanel::release_runs`] before they land.
    held: Option<Vec<futures::channel::oneshot::Sender<()>>>,
}

#[cfg(test)]
impl TestHooks {
    /// Counts a run started, and returns what it calls once it has read the
    /// status, and the gate it then awaits.
    pub(crate) fn started(
        &mut self,
        cx: &App,
    ) -> (
        impl FnOnce() + Send + 'static,
        impl Future<Output = ()> + Send + 'static,
    ) {
        self.started += 1;
        let (release, gate) = futures::channel::oneshot::channel();
        match &mut self.held {
            Some(held) => held.push(release),
            None => drop(release.send(())),
        }
        let gate = async move {
            gate.await.ok();
        };
        let ran = self.ran.clone();
        let executor = cx.background_executor().clone();
        let ran = move || {
            let mut ran = ran.lock().unwrap();
            ran.0 += 1;
            ran.1 += usize::from(!executor.is_main_thread());
        };
        (ran, gate)
    }

    /// Holds the runs started from now until [`Self::release`], once
    /// they've read the status.
    pub(crate) fn hold(&mut self) {
        self.held.get_or_insert_default();
    }

    /// Lets the held runs, and the ones started from now, land.
    pub(crate) fn release(&mut self) {
        for release in self.held.take().unwrap_or_default() {
            release.send(()).ok();
        }
    }

    /// Runs started, and runs that read the status and of them those off
    /// the main thread.
    pub(crate) fn runs(&self) -> (usize, (usize, usize)) {
        (self.started, *self.ran.lock().unwrap())
    }
}

/// A group as shown: its title, its count, and its rows' letters and paths
/// from the root.
#[cfg(test)]
pub(crate) type ShownGroup = (&'static str, usize, Vec<(&'static str, String)>);

/// A row as shown: its letter, name and directory, and whether it's dimmed.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShownRow {
    pub letter: &'static str,
    pub name: String,
    pub directory: String,
    pub dimmed: bool,
}

#[cfg(test)]
impl GitPanel {
    /// Holds the runs started from now until [`Self::release_runs`], once
    /// they've read the status.
    pub(crate) fn hold_runs(&mut self) {
        self.test.borrow_mut().hold();
    }

    /// Lets the held runs, and the ones started from now, land.
    pub(crate) fn release_runs(&mut self) {
        self.test.borrow_mut().release();
    }

    /// Runs started.
    pub(crate) fn runs_started(&self) -> usize {
        self.test.borrow().runs().0
    }

    /// Runs that read the status, and of them those off the main thread.
    pub(crate) fn runs_ran(&self) -> (usize, usize) {
        self.test.borrow().runs().1
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// What the tests hold and count, shared with the git state.
    pub(crate) fn test_hooks(&self) -> std::rc::Rc<std::cell::RefCell<TestHooks>> {
        self.test.clone()
    }

    pub(crate) fn shown_rows(&self) -> Vec<ShownRow> {
        self.entries
            .iter()
            .map(|entry| ShownRow {
                letter: letter(entry.kind),
                name: entry
                    .relative
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                directory: entry
                    .relative
                    .parent()
                    .map(|dir| dir.display().to_string())
                    .unwrap_or_default(),
                dimmed: entry.kind == ChangeKind::Deleted,
            })
            .collect()
    }

    /// Each group shown, in order: its title, its count, and its rows'
    /// letters and paths from the root.
    pub(crate) fn groups(&self) -> Vec<ShownGroup> {
        self.headed()
            .map(|group| {
                let range = self.group_range(group);
                let rows = (self.entries[range.clone()].iter())
                    .map(|entry| (letter(entry.kind), entry.relative.display().to_string()))
                    .collect();
                (group.title(), range.len(), rows)
            })
            .collect()
    }

    /// The debug selector of the button for `action` on file `ix`'s row, or
    /// with `ix` `None` on `group`'s header.
    pub(crate) fn button_selector(
        &self,
        ix: Option<usize>,
        group: Group,
        action: Action,
    ) -> String {
        let at = match ix {
            Some(ix) => self.row_of(ix),
            None => (0..self.row_count())
                .find(|at| self.row(*at) == Some(Row::Header(group)))
                .unwrap_or(usize::MAX),
        };
        format!("icon-button:{}", self.button_id(at, action))
    }

    /// The paths a row would open, in row order.
    pub(crate) fn paths(&self) -> Vec<PathBuf> {
        self.entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// The commit box's after-line text: each row's, with its tint.
    pub(crate) fn commit_hints(&self, cx: &App) -> Vec<(u32, String, Tint)> {
        let Some(commit) = &self.commit else {
            return Vec::new();
        };
        let model = commit.editor.read(cx).model();
        let text = model.text();
        (model.decorations(0..text.len()).into_iter())
            .filter_map(|decoration| match decoration.kind {
                DecorationKind::AfterLine(hint) => {
                    let row = text.offset_to_point(decoration.range.start).row;
                    Some((row, hint.to_string(), decoration.tint))
                }
                _ => None,
            })
            .collect()
    }

    /// The warning under the box, if any.
    pub(crate) fn commit_warning(&self, cx: &App) -> Option<&'static str> {
        commit_warning(self.commit.as_ref()?, cx)
    }

    /// Whether Amend is checked.
    pub(crate) fn amend_checked(&self) -> bool {
        self.commit.as_ref().is_some_and(|commit| commit.amend)
    }

    /// The Commit button's label and whether it's enabled.
    pub(crate) fn commit_button_state(&self, cx: &App) -> Option<(&'static str, bool)> {
        Some(commit_button(self.commit.as_ref()?, cx))
    }

    /// Whether the box's command line is open.
    pub(crate) fn commit_line_open(&self) -> bool {
        (self.commit.as_ref()).is_some_and(|commit| commit.line_open.is_some())
    }

    /// What shows in place of the rows, if anything.
    pub(crate) fn message_text(&self) -> Option<String> {
        self.message()
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Where file `ix`'s row was laid out in the last frame, from the list's
    /// bounds, scroll offset and measured row height.
    pub(crate) fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.list_row_bounds(self.row_of(ix))
    }

    /// Where the header of `group` was laid out in the last frame.
    pub(crate) fn header_bounds(&self, group: Group) -> Option<gpui::Bounds<gpui::Pixels>> {
        let at = (0..self.row_count()).find(|at| self.row(*at) == Some(Row::Header(group)))?;
        self.list_row_bounds(at)
    }

    /// Where row `at` of the list was laid out in the last frame.
    fn list_row_bounds(&self, at: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        let count = self.row_count();
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let height = state.last_item_size?.contents.height / count as f32;
        let top = list.origin.y + state.base_handle.offset().y + height * at as f32;
        Some(gpui::Bounds::new(
            gpui::point(list.origin.x, top),
            gpui::size(list.size.width, height),
        ))
    }
}

#[cfg(test)]
#[path = "git_panel_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "git_panel_groups_tests.rs"]
mod git_panel_groups_tests;
