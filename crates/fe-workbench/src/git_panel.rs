//! The Changes panel on the right dock: the project's files that differ
//! from git's HEAD in VS Code's groups, "Staged Changes" (how the index
//! differs from HEAD, shown only when there are any) and "Changes" (how the
//! working tree differs from the index, untracked files too, as `U`), each
//! header with its count. A file staged and changed again is in both. Each
//! row has a one-letter status, its name and its directory, sorted by path
//! within its group; a conflicted file is only in Changes.
//!
//! Clicking a row, or Enter on the selected one, opens its diff in the
//! active pane (`git_diff`): a Staged row's HEAD ↔ Index, a Changes row's
//! Index ↔ Working Tree. A deleted file's row is dimmed. Alt-Enter, or the
//! row's "Open File" button, opens the file itself, which a deleted one
//! can't.
//!
//! The selected row's buttons show, and any row's under the pointer, as VS
//! Code's do: a Changes row's "Open File", "Discard Changes" and "Stage
//! Changes", a Staged row's "Open File" and "Unstage Changes". The headers'
//! are "Unstage All Changes" on Staged Changes, "Discard All Changes" and
//! "Stage All Changes" on Changes. The workbench runs them (`git_stage`),
//! asking before it discards.
//!
//! With vim on, the panel takes the keys of fugitive's `:Git` status
//! window: `j` and `k` move, `s` stages the selected file, `u` unstages it,
//! `-` stages a Changes row's or unstages a Staged row's, `X` discards it,
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

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use fe_git::{ChangeKind, FileStatus, GitError, SplitStatus};
use fe_ui::{Icon, IconButton, ROW_HEIGHT, Tooltip, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, Modifiers,
    ParentElement as _, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};

use crate::{Workbench, git::GitState};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "git_panel";

/// The hue of added and untracked files' letters: the accent's, turned
/// green.
const ADDED_HUE: f32 = 130. / 360.;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    /// How the index differs from HEAD.
    Staged,
    /// How the working tree differs from the index, untracked files too.
    Changes,
}

impl Group {
    /// Its header's title.
    pub(crate) fn title(self) -> &'static str {
        match self {
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
        (Group::Changes, false) => &[Action::OpenFile, Action::Discard, Action::Stage],
        (Group::Changes, true) => &[Action::Discard, Action::Stage],
        (Group::Staged, false) => &[Action::OpenFile, Action::Unstage],
        (Group::Staged, true) => &[Action::Unstage],
    }
}

/// The buttons a header of `group` has, in order.
fn header_actions(group: Group) -> &'static [Action] {
    match group {
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
    /// The Staged group's files, then the Changes group's.
    entries: Vec<Entry>,
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
        let (mut staged, mut changes, mut files) = (Vec::new(), Vec::new(), Vec::new());
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
                    changes.push(entry(ChangeKind::Conflicted, Group::Changes))
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
        staged.sort_by(|a, b| a.relative.cmp(&b.relative));
        changes.sort_by(|a, b| a.relative.cmp(&b.relative));
        files.sort_by(|a, b| a.relative.cmp(&b.relative));
        let changed = staged.len() + changes.len() != self.entries.len()
            || (staged.iter().chain(&changes)).ne(&self.entries)
            || files != self.files;
        self.staged = staged.len();
        self.entries = staged.into_iter().chain(changes).collect();
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
            Group::Staged => 0..self.staged,
            Group::Changes => self.staged..self.entries.len(),
        }
    }

    /// How many rows the list has: each file's and each group's header,
    /// Staged Changes only if it has any, none if nothing's listed.
    fn row_count(&self) -> usize {
        match self.entries.is_empty() {
            true => 0,
            false => self.entries.len() + 1 + usize::from(self.staged > 0),
        }
    }

    /// The list's row `at`.
    fn row(&self, at: usize) -> Option<Row> {
        if at >= self.row_count() {
            return None;
        }
        let staged = self.staged;
        Some(match (staged > 0, at) {
            (true, 0) => Row::Header(Group::Staged),
            (true, at) if at <= staged => Row::File(at - 1),
            (true, at) if at == staged + 1 => Row::Header(Group::Changes),
            (true, at) => Row::File(at - 2),
            (false, 0) => Row::Header(Group::Changes),
            (false, at) => Row::File(at - 1),
        })
    }

    /// The list's row of file `ix`.
    fn row_of(&self, ix: usize) -> usize {
        ix + 1 + usize::from(self.staged > 0 && ix >= self.staged)
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected?)
    }

    /// If the panel has focus, its selected file, if any: what
    /// `git.stage_file`, `git.unstage_file` and `git.discard_file` act on
    /// then. `None` if it hasn't focus.
    pub(crate) fn focused_selection(&self, window: &Window, cx: &App) -> Option<Option<PathBuf>> {
        let focused = self.focus.contains_focused(window, cx);
        focused.then(|| self.selected_entry().map(|entry| entry.path.clone()))
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
        match action {
            Action::OpenFile => self.open_file(ix, cx),
            Action::Discard => self.discard(vec![path], window, cx),
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
            Action::Discard => self.discard(paths, window, cx),
            Action::Stage => self.stage(paths, true, cx),
            Action::Unstage => self.stage(paths, false, cx),
        }
    }

    /// Up and down move the selection, the first press selecting the first
    /// row; Enter opens the selected row's diff, and Alt-Enter its file.
    /// With vim on, so do fugitive's keys: see the module docs.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
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
                let tooltip = action.tooltip(header);
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Read when first shown, unless a run has started.
        if !self.read {
            self.refresh(cx);
        }
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
        let shown = [Group::Staged, Group::Changes].into_iter().filter(|group| {
            (0..self.row_count()).any(|at| self.row(at) == Some(Row::Header(*group)))
        });
        shown
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
