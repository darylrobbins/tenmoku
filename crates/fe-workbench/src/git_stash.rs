//! Stashing local changes and bringing them back: `git.stash`,
//! `git.stash_include_untracked`, `git.stash_pop` and `git.stash_apply`, as
//! VS Code's Git: Stash, Stash (Include Untracked), Pop Stash… and Apply
//! Stash…, routed here by `git_commands`.
//!
//! - **Stash.** Asks for an optional message in a picker of its own
//!   ("Stash message (optional)"), whose one row stashes with what's typed,
//!   or git's own message for nothing; untracked files too for
//!   `git.stash_include_untracked`. With nothing to stash it says `No local
//!   changes to save`.
//! - **Pop and apply.** List the stashes in the background, then show them
//!   in a picker, latest first, each `stash@{n}: message` with the branch
//!   it was made on; typing filters. Enter pops or applies the row's
//!   entry, if it's still the one listed (by its commit). With none it says
//!   `There are no stashes in the repository.`, VS Code's words.
//!
//! Each runs git in the background, in its turn among the work that
//! changes a repository (`git_stage`'s queue). Once it's run, the git
//! state is read again, refreshing the Changes panel, and the open files
//! are checked against disk, reloading those it changed. A pop or apply
//! that conflicts is a warning naming the files, the entry kept; git
//! failing otherwise is an error toast with its output.
//!
//! The pickers are a modal of their own, shown in place of any palette.
//! A listing that lands once any modal has shown since it started shows
//! nothing. Each step holds to the root it started in: a new root
//! dismisses the pickers, and a pick, or git landing, from before it does
//! nothing.

use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_git::{GitError, Repo, StashEntry, StashError};
use fe_ui::ui_theme;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, Subscription, WeakEntity,
    Window, div,
};
use kiln_shell::{ModalEvent, ModalId};

use super::{
    super::{
        git_stage,
        toast::{self, Toast},
    },
    GitCommand, GitOp,
};
use crate::{
    RootChanged, Workbench,
    picker::{Picker, PickerDelegate},
};

/// The pickers' id in the modal layer.
pub(crate) const STASH_PICKER: ModalId = ModalId("git_stash");

/// What a command answers outside a repository.
const NO_REPO: &str = "Not in a git repository";

/// What popping or applying answers with nothing stashed: VS Code's.
pub(crate) const NO_STASHES: &str = "There are no stashes in the repository.";

/// What a stash with nothing to stash answers.
pub(crate) const NOTHING_TO_SAVE: &str = "No local changes to save";

/// What a pop or apply answers once the entry picked has moved or gone.
pub(crate) const STASH_CHANGED: &str = "The stash changed since it was listed; pick again";

/// Runs `op`, a stash command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let changes = workbench.dock.read(cx).changes().clone();
    let origin = workbench.stash_origin();
    let Some(origin) = origin.filter(|_| !changes.read(cx).not_a_repository()) else {
        return workbench.answer_git_op(&op, NO_REPO.to_owned(), cx);
    };
    match op.command {
        GitCommand::Stash => workbench.ask_stash_message(origin, false, window, cx),
        GitCommand::StashIncludeUntracked => workbench.ask_stash_message(origin, true, window, cx),
        GitCommand::StashPop => workbench.list_stashes(origin, op, Verb::Pop, window, cx),
        _ => workbench.list_stashes(origin, op, Verb::Apply, window, cx),
    }
}

/// The root a stash command started in, and the git state that root's,
/// which a new root replaces.
#[derive(Clone, Debug)]
struct Origin {
    root: PathBuf,
    git: EntityId,
}

/// What's done to a picked entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verb {
    Pop,
    Apply,
}

/// A piece of stash work to run.
#[derive(Clone, Debug)]
enum Work {
    Push {
        message: Option<String>,
        include_untracked: bool,
    },
    /// Pops or applies `entry`, if it's still at its index.
    Take { verb: Verb, entry: StashEntry },
}

impl Work {
    /// What a failure's toast says.
    fn failed(&self) -> String {
        match self {
            Work::Push { .. } => "Can't stash".to_owned(),
            Work::Take { verb, entry } => {
                let verb = match verb {
                    Verb::Pop => "pop",
                    Verb::Apply => "apply",
                };
                format!("Can't {verb} stash@{{{}}}", entry.index)
            }
        }
    }
}

/// How a piece of stash work went.
enum Outcome {
    Done,
    NoRepo,
    /// The entry picked isn't at its index any more.
    Moved,
    Failed(StashError),
}

impl Workbench {
    /// The root stash commands start in now, if there's one.
    fn stash_origin(&self) -> Option<Origin> {
        Some(Origin {
            root: self.root()?.to_owned(),
            git: self.git.entity_id(),
        })
    }

    /// Whether `origin` is still the root.
    fn is_stash_origin(&self, origin: &Origin) -> bool {
        self.git.entity_id() == origin.git
    }

    /// Where focus goes back to once a stash picker shown now is done:
    /// where it is, or if a modal shows, the active editor or the
    /// workbench.
    fn stash_return_focus(&self, window: &mut Window, cx: &App) -> Option<FocusHandle> {
        if self.modals.read(cx).modal_view().is_none() {
            return window.focused(cx);
        }
        Some(match self.active_editor(cx) {
            Some(editor) => editor.focus_handle(cx),
            None => self.focus_handle(cx),
        })
    }

    /// Asks for the stash's message, then stashes, untracked files too if
    /// `include_untracked`.
    fn ask_stash_message(
        &mut self,
        origin: Origin,
        include_untracked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delegate = MessagePicker {
            workbench: cx.entity().downgrade(),
            origin,
            include_untracked,
            message: String::new(),
            back: self.stash_return_focus(window, cx),
            _root: None,
        };
        let picker = cx.new(|cx| {
            let ui = ui_theme(cx);
            Picker::new(delegate, "Stash message (optional)", ui, cx)
        });
        self.show_stash_modal(picker, window, cx);
    }

    /// Lists the stashes in the background, then shows them to `verb`
    /// one. Shows nothing if the root's changed by then, or any modal has
    /// shown meanwhile.
    fn list_stashes(
        &mut self,
        origin: Origin,
        op: GitOp,
        verb: Verb,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Superseded once any modal shows: another listing's picker, a
        // palette.
        let superseded = Rc::new(Cell::new(false));
        let watch = cx.subscribe(&self.modals, {
            let superseded = superseded.clone();
            move |_, _, event: &ModalEvent, _| {
                if matches!(event, ModalEvent::Shown(_)) {
                    superseded.set(true);
                }
            }
        });
        let root = origin.root.clone();
        let load = cx.background_spawn(async move {
            match Repo::discover(&root)? {
                Some(repo) => repo.stash_list().map(Some),
                None => Ok(None),
            }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let entries = load.await;
            drop(watch);
            workbench
                .update_in(cx, |workbench, window, cx| {
                    if superseded.get() || !workbench.is_stash_origin(&origin) {
                        return;
                    }
                    workbench.stashes_listed(origin, op, verb, entries, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// The stashes listed for `op`, from `origin`.
    fn stashes_listed(
        &mut self,
        origin: Origin,
        op: GitOp,
        verb: Verb,
        entries: Result<Option<Vec<StashEntry>>, GitError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = match entries {
            Ok(Some(entries)) => entries,
            Ok(None) => return self.answer_git_op(&op, NO_REPO.to_owned(), cx),
            Err(error) => {
                let toast = Toast::error("Can't list the stashes");
                return toast::show(cx, toast.detail(error.to_string()));
            }
        };
        if entries.is_empty() {
            return self.answer_git_op(&op, NO_STASHES.to_owned(), cx);
        }
        let delegate = StashPicker {
            workbench: cx.entity().downgrade(),
            origin,
            verb,
            matches: (0..entries.len()).collect(),
            entries,
            back: self.stash_return_focus(window, cx),
            _root: None,
        };
        let placeholder = match verb {
            Verb::Pop => "Pick a stash to pop",
            Verb::Apply => "Pick a stash to apply",
        };
        let picker = cx.new(|cx| {
            let ui = ui_theme(cx);
            Picker::new(delegate, placeholder, ui, cx)
        });
        self.show_stash_modal(picker, window, cx);
    }

    /// Shows `picker`, focused, in place of any palette, until it's picked
    /// from or dismissed, or the root changes.
    fn show_stash_modal<D: StashModal>(
        &mut self,
        picker: Entity<Picker<D>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette(window, cx);
        let workbench = cx.entity();
        picker.update(cx, |picker, cx| {
            let root_changed = cx.subscribe_in(
                &workbench,
                window,
                |picker, _, _: &RootChanged, window, cx| picker.dismiss(window, cx),
            );
            picker.delegate.follow_root(root_changed);
            picker.show(window, cx);
        });
        (self.modals.clone()).update(cx, |layer, cx| {
            layer.show_modal(STASH_PICKER, picker, window, cx)
        });
    }

    /// Runs `work` in `origin`'s repository in the background, in its
    /// turn among the work that changes a repository; then reads git
    /// again and checks the open files. Nothing if the root's changed.
    fn start_stash(
        &mut self,
        origin: Origin,
        work: Work,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_stash_origin(&origin) {
            return log::info!("not running {work:?}: the root changed");
        }
        let workbench = cx.weak_entity();
        let window = window.window_handle();
        git_stage::mutate(cx, move |cx| {
            #[cfg(test)]
            let gate = git_stage::test_gate(cx);
            let root = origin.root.clone();
            let run = work.clone();
            let outcome = cx.background_spawn(async move {
                #[cfg(test)]
                gate.await;
                run_stash(&root, &run)
            });
            cx.spawn(async move |cx| {
                let outcome = outcome.await;
                (cx.update_window(window, |_, window, cx| {
                    (workbench.update(cx, |workbench, cx| {
                        workbench.stashed(origin, work, outcome, window, cx)
                    }))
                    .ok();
                }))
                .ok();
            })
        });
    }

    /// `work`, from `origin`, has run: nothing more if the root's changed
    /// since.
    fn stashed(
        &mut self,
        origin: Origin,
        work: Work,
        outcome: Outcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_stash_origin(&origin) {
            return log::info!("{work:?} landed after the root changed");
        }
        let error = match outcome {
            Outcome::NoRepo => return toast::show(cx, Toast::info(NO_REPO)),
            Outcome::Moved => return toast::show(cx, Toast::info(STASH_CHANGED)),
            Outcome::Done => None,
            Outcome::Failed(error) => Some(error),
        };
        self.git.update(cx, |git, cx| git.refresh(cx));
        self.check_open_files(window, cx);
        let toast = match error {
            None => return,
            Some(StashError::NothingToStash) => Toast::info(NOTHING_TO_SAVE),
            Some(StashError::Conflict { conflicts }) => {
                let mut detail = conflicts.join("\n");
                if matches!(
                    work,
                    Work::Take {
                        verb: Verb::Pop,
                        ..
                    }
                ) {
                    detail.push_str("\nThe stash entry is kept in case you need it again.");
                }
                let Work::Take { entry, .. } = &work else {
                    unreachable!("a push applies nothing");
                };
                Toast::warning(format!("Conflicts applying stash@{{{}}}", entry.index))
                    .detail(detail)
            }
            Some(StashError::Failed { output }) => Toast::error(work.failed()).detail(output),
            Some(error) => Toast::error(work.failed()).detail(error.to_string()),
        };
        toast::show(cx, toast);
    }
}

/// Runs `work` in the repository containing `root`. Runs on the
/// background executor, and blocks until git exits.
fn run_stash(root: &Path, work: &Work) -> Outcome {
    let repo = match Repo::discover(root) {
        Ok(Some(repo)) => repo,
        Ok(None) => return Outcome::NoRepo,
        Err(error) => return Outcome::Failed(error.into()),
    };
    let ran = match work {
        Work::Push {
            message,
            include_untracked,
        } => repo.stash_push(message.as_deref(), *include_untracked),
        Work::Take { verb, entry } => {
            let listed = match repo.stash_list() {
                Ok(listed) => listed,
                Err(error) => return Outcome::Failed(error.into()),
            };
            if listed.get(entry.index).map(|at| &at.id) != Some(&entry.id) {
                return Outcome::Moved;
            }
            match verb {
                Verb::Pop => repo.stash_pop(entry.index),
                Verb::Apply => repo.stash_apply(entry.index),
            }
        }
    };
    match ran {
        Ok(()) => Outcome::Done,
        Err(error) => Outcome::Failed(error),
    }
}

/// `entry`'s row: `stash@{n}: message`.
fn label(entry: &StashEntry) -> String {
    format!("stash@{{{}}}: {}", entry.index, entry.message)
}

/// Focuses `back`, if there's one.
fn focus_back(back: Option<FocusHandle>, window: &mut Window, cx: &mut App) {
    if let Some(back) = back {
        window.focus(&back, cx);
    }
}

/// Has `workbench` run `work`, once the picker that asked for it is gone.
fn start_later(
    workbench: WeakEntity<Workbench>,
    origin: Origin,
    work: Work,
    window: &mut Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        (workbench.update(cx, |workbench, cx| {
            workbench.start_stash(origin, work, window, cx)
        }))
        .ok();
    });
}

/// A stash picker: it follows the root, dismissed once it changes.
trait StashModal: PickerDelegate {
    fn follow_root(&mut self, root_changed: Subscription);
}

/// `git.stash`'s and `git.stash_include_untracked`'s picker: the message
/// typed, and a row that stashes with it.
pub(crate) struct MessagePicker {
    workbench: WeakEntity<Workbench>,
    /// The root it stashes in.
    origin: Origin,
    include_untracked: bool,
    /// The query, trimmed.
    message: String,
    /// What had focus before it opened.
    back: Option<FocusHandle>,
    _root: Option<Subscription>,
}

impl StashModal for MessagePicker {
    fn follow_root(&mut self, root_changed: Subscription) {
        self._root = Some(root_changed);
    }
}

impl MessagePicker {
    /// Its one row's label.
    pub(crate) fn label(&self) -> String {
        let what = match self.include_untracked {
            true => "Stash local changes, untracked files included",
            false => "Stash local changes",
        };
        match self.message.is_empty() {
            true => what.to_owned(),
            false => format!("{what}, as '{}'", self.message),
        }
    }
}

impl PickerDelegate for MessagePicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.message = query.trim().to_owned();
        cx.notify();
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        1
    }

    fn match_id(&self, _: usize) -> SharedString {
        "stash".into()
    }

    fn render_match(&self, _: usize, _: bool, _: &App) -> AnyElement {
        div().child(self.label()).into_any_element()
    }

    /// Stashes, with the message typed, if any.
    fn confirm(&mut self, _: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let work = Work::Push {
            message: Some(self.message.clone()).filter(|message| !message.is_empty()),
            include_untracked: self.include_untracked,
        };
        focus_back(self.back.take(), window, cx);
        let origin = self.origin.clone();
        start_later(self.workbench.clone(), origin, work, window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        focus_back(self.back.take(), window, cx);
    }
}

/// `git.stash_pop`'s and `git.stash_apply`'s picker: the stashes whose
/// row matches the query, latest first.
pub(crate) struct StashPicker {
    workbench: WeakEntity<Workbench>,
    /// The root it lists the stashes of.
    origin: Origin,
    verb: Verb,
    /// As [`Repo::stash_list`] lists them.
    entries: Vec<StashEntry>,
    /// The entries matching the query, as indices into `entries`.
    matches: Vec<usize>,
    /// What had focus as it opened.
    back: Option<FocusHandle>,
    _root: Option<Subscription>,
}

impl StashModal for StashPicker {
    fn follow_root(&mut self, root_changed: Subscription) {
        self._root = Some(root_changed);
    }
}

impl StashPicker {
    fn entry(&self, ix: usize) -> Option<&StashEntry> {
        Some(&self.entries[*self.matches.get(ix)?])
    }
}

#[cfg(test)]
impl StashPicker {
    /// Each row's label.
    pub(crate) fn labels(&self) -> Vec<String> {
        (0..self.match_count())
            .filter_map(|ix| self.entry(ix).map(label))
            .collect()
    }
}

impl PickerDelegate for StashPicker {
    /// Entries whose row holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.trim().to_lowercase();
        self.matches = (self.entries.iter().enumerate())
            .filter(|(_, entry)| label(entry).to_lowercase().contains(&query))
            .map(|(ix, _)| ix)
            .collect();
        cx.notify();
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The entry's commit.
    fn match_id(&self, ix: usize) -> SharedString {
        self.entry(ix)
            .map_or_else(SharedString::default, |entry| entry.id.clone().into())
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let Some(entry) = self.entry(ix) else {
            return div().into_any_element();
        };
        let muted = ui_theme(cx).muted_text;
        let branch =
            (entry.branch.clone()).map(|branch| div().flex_none().text_color(muted).child(branch));
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().truncate().child(label(entry)))
            .children(branch)
            .into_any_element()
    }

    /// Pops or applies the row's entry.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(entry) = self.entry(ix).cloned() else {
            return false;
        };
        let work = Work::Take {
            verb: self.verb,
            entry,
        };
        focus_back(self.back.take(), window, cx);
        let origin = self.origin.clone();
        start_later(self.workbench.clone(), origin, work, window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        focus_back(self.back.take(), window, cx);
    }
}

#[cfg(test)]
#[path = "git_stash_tests.rs"]
mod git_stash_tests;
