//! Switching to a branch and creating one: `git.switch_branch` and
//! `git.create_branch`, with the branch typed after `:Git switch` in
//! [`GitOp::args`], routed here by `git_commands`; and what the title bar's
//! branch widget shows.
//!
//! - **Branch.** The title bar's branch widget (`title_bar.rs`) shows
//!   HEAD's branch, or its short id when detached, then as VS Code's status
//!   item does: `*` for working-tree or untracked changes, `+` for staged
//!   ones and `!` while a merge or rebase is in progress, in that order;
//!   then how far the branch is from its upstream as of the last fetch,
//!   `main ↑2 ↓1`: `↑` the commits to push, `↓` those to pull, each left
//!   out at 0, both with no upstream; nothing outside a repository. Its
//!   tooltip names the upstream. The git refresh ([`crate::git`]) sets it
//!   each time it reads the status, which fetching, pulling, pushing,
//!   committing and switching each have it do. A click runs
//!   `git.switch_branch`. The status bar shows no branch (the design's
//!   status bar row).
//! - **Picker.** `git.switch_branch`, or `:Git switch` and `:Git checkout`
//!   with no name, lists the branches in the background, then shows the
//!   shared picker (kiln-shell's: `ctrl-n`/`ctrl-p`, up and down move):
//!   `+ Create new branch…`, then the local branches, the current one
//!   marked, then the remote-tracking ones, each group newest commit first,
//!   each with its tip's summary and age. Typing filters by name. Enter
//!   switches to the row's ref, local or remote ([`BranchRef`]), however
//!   its short name reads; on a remote branch with no local one of its
//!   name, that makes a local one tracking it.
//! - **Create.** `git.create_branch`, or the picker's first row, asks for
//!   the name in a picker of its own ("Branch name"), whose one row
//!   creates the branch at HEAD and switches to it; a name git wouldn't
//!   take shows `Not a valid branch name` under the input, and Enter does
//!   nothing. `:Git switch -c {name}` and `:Git checkout -b {name}` create
//!   it at once.
//! - **Switch.** `:Git switch {name}` and `:Git checkout {name}` switch at
//!   once. Every switch runs git in the background; one that would
//!   overwrite local changes asks first, listing them, whether to stash
//!   them and switch, switch dropping them, or cancel. An unknown branch
//!   answers `Not found: <name>`, an invalid name `Not a valid branch
//!   name`, on the vim line the command was typed on, else in a toast; git
//!   failing otherwise is an error toast with its output. Once switched,
//!   git refreshes, rebasing the change bars on the new HEAD, and the open
//!   files are checked against disk, reloading those the switch changed.
//!
//! The pickers are the workbench's [`Palette::Branch`]: another palette
//! dismisses them before it shows, as they do it. A listing that lands
//! once a later listing has started, or any modal has shown since it
//! started, shows nothing.
//!
//! Each step holds to the root it started in ([`Origin`]): a new root
//! dismisses the pickers, and a pick, a switch landing or an answer to the
//! question from before it does nothing, so nothing done in one repository
//! was asked about in another.

use kiln_shell::picker::joined_label;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_git::{Branch, BranchRef, Head, Repo, SwitchError, Upstream};
use fe_ui::ui_theme;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable as _,
    IntoElement as _, ParentElement as _, PromptButton, PromptLevel, SharedString, Styled as _,
    Subscription, WeakEntity, Window, div,
};
use kiln_l10n::{text, tr};
use kiln_shell::ModalEvent;

use super::{
    Palette, RootChanged, Workbench,
    git_commands::{GitCommand, GitOp},
    toast::{self, Toast},
};
use crate::{
    git::Marks,
    picker::{Picker, PickerDelegate},
};

/// The branch picker's first row, in en-US, for tests.
#[cfg(test)]
pub(crate) const CREATE_ROW: &str = "+ Create new branch…";

/// The overwrite question, in en-US, for tests.
#[cfg(test)]
pub(crate) const OVERWRITE: &str = "Your local changes would be overwritten by checkout.";

/// The branch picker's first row.
fn create_row() -> SharedString {
    tr!("workbench-chrome-branch-create-row")
}

/// What a command answers outside a repository.
fn no_repo() -> String {
    tr!("workbench-chrome-branch-not-a-repository").to_string()
}

/// What an invalid branch name answers.
fn invalid_name() -> SharedString {
    tr!("workbench-chrome-branch-invalid-name")
}

/// The overwrite question's buttons, VS Code's but for Migrate Changes.
fn overwrite_buttons() -> [PromptButton; 3] {
    [
        PromptButton::Ok(tr!("workbench-chrome-branch-stash-and-checkout")),
        PromptButton::Other(tr!("workbench-chrome-branch-force-checkout")),
        PromptButton::Cancel(tr!("workbench-chrome-branch-cancel")),
    ]
}

/// The answers to [`overwrite_buttons`]; anything else cancels.
const STASH: usize = 0;
const FORCE: usize = 1;

/// How many paths the question lists before `and N more`.
const MAX_PATHS: usize = 10;

/// Runs `op`, a branch command.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let Some(origin) = workbench.origin() else {
        return workbench.fail_git_op(&op, no_repo(), cx);
    };
    let name = op.args.trim().to_owned();
    match (op.command, name.is_empty()) {
        (GitCommand::CreateBranch, true) => {
            let back = workbench.return_focus(window, cx);
            workbench.ask_branch_name(origin, back, window, cx);
        }
        (GitCommand::CreateBranch, false) => {
            workbench.start_switch(origin, Switch::Create(name), Some(op), window, cx)
        }
        (_, true) => workbench.open_branch_picker(origin, op, window, cx),
        (_, false) => {
            let target = Target::Name(name);
            workbench.start_switch(origin, Switch::To(target), Some(op), window, cx)
        }
    }
}

/// The root a branch command started in, and the git state that root's,
/// which a new root replaces.
#[derive(Clone, Debug)]
struct Origin {
    root: PathBuf,
    git: EntityId,
}

/// The branch a switch goes to.
#[derive(Clone, Debug)]
enum Target {
    /// By the name typed, as git takes it: a local branch's, else a
    /// remote-tracking one's.
    Name(String),
    /// A picked branch, by the kind of ref it is.
    Ref(BranchRef),
}

impl Target {
    fn name(&self) -> String {
        match self {
            Self::Name(name) | Self::Ref(BranchRef::Local(name)) => name.clone(),
            Self::Ref(BranchRef::Remote { remote, branch }) => format!("{remote}/{branch}"),
        }
    }
}

/// A switch to run.
#[derive(Clone, Debug)]
enum Switch {
    To(Target),
    /// To a new branch at HEAD, by its name.
    Create(String),
    /// Dropping the local changes in the way.
    Force(Target),
    /// Stashing the local changes first.
    Stash(Target),
}

impl Switch {
    fn name(&self) -> String {
        match self {
            Self::Create(name) => name.clone(),
            Self::To(target) | Self::Force(target) | Self::Stash(target) => target.name(),
        }
    }

    /// The branch it goes to, if it's there already.
    fn target(&self) -> Option<&Target> {
        match self {
            Self::Create(_) => None,
            Self::To(target) | Self::Force(target) | Self::Stash(target) => Some(target),
        }
    }
}

/// How a switch went.
enum Outcome {
    Done,
    NoRepo,
    /// Local changes to these paths, from the working tree, are in the way
    /// of switching to this branch, as resolved once for a typed name.
    Overwrite(Vec<String>, Option<BranchRef>),
    Failed(SwitchError),
}

impl Workbench {
    /// Shows `branch` in the title bar's branch widget: where HEAD points,
    /// what changed ([`Marks`]) and its upstream, if it has one. None,
    /// outside a repository, hides the widget.
    pub(crate) fn set_branch_item(
        &mut self,
        branch: Option<(Head, Marks, Option<Upstream>)>,
        cx: &mut Context<Self>,
    ) {
        let (shown, upstream) = match branch {
            Some((head, marks, upstream)) => {
                let label = branch_label(&head, marks, upstream.as_ref());
                let name = upstream.map(|upstream| SharedString::from(upstream.name));
                (Some(SharedString::from(label)), name)
            }
            None => (None, None),
        };
        if shown != self.title_branch || upstream != self.title_upstream {
            self.title_branch = shown;
            self.title_upstream = upstream;
            cx.notify();
        }
    }

    /// Lists the branches in the background, then shows the branch picker.
    /// Shows nothing if the root's changed by then, another listing has
    /// started, or any modal has shown meanwhile.
    fn open_branch_picker(
        &mut self,
        origin: Origin,
        op: GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Superseded once any modal shows: another listing's picker, a
        // palette, the name's picker.
        let superseded = Rc::new(Cell::new(false));
        let watch = cx.subscribe(&self.modals, {
            let superseded = superseded.clone();
            move |_, _, event: &ModalEvent, _| {
                if matches!(event, ModalEvent::Shown(_)) {
                    superseded.set(true);
                }
            }
        });
        let listing = self.git.update(cx, |git, _| git.start_branch_listing());
        let root = origin.root.clone();
        let load = cx.background_spawn(async move {
            match Repo::discover(&root)? {
                Some(repo) => repo.branches().map(Some),
                None => Ok(None),
            }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let branches = load.await;
            drop(watch);
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let last = (workbench.git.read(cx)).is_last_branch_listing(listing);
                    if superseded.get() || !workbench.is_current(&origin) || !last {
                        return;
                    }
                    workbench.branches_listed(origin, op, branches, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// The branches listed for `op`, from `origin`.
    fn branches_listed(
        &mut self,
        origin: Origin,
        op: GitOp,
        branches: Result<Option<Vec<Branch>>, fe_git::GitError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match branches {
            Ok(Some(branches)) => self.show_branch_picker(origin, branches, window, cx),
            Ok(None) => self.fail_git_op(&op, no_repo(), cx),
            Err(error) => {
                let toast = Toast::error(tr!("workbench-chrome-branch-cant-list"));
                toast::show(cx, toast.detail(error.to_string()));
            }
        }
    }

    fn show_branch_picker(
        &mut self,
        origin: Origin,
        branches: Vec<Branch>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delegate = BranchPicker {
            workbench: cx.entity().downgrade(),
            origin,
            matches: (0..branches.len()).collect(),
            branches,
            now: now(),
            back: self.return_focus(window, cx),
            _root: None,
        };
        let picker = cx.new(|cx| {
            let ui = ui_theme(cx);
            Picker::new(
                delegate,
                text!("workbench-chrome-branch-pick-placeholder"),
                ui,
                cx,
            )
        });
        self.show_branch_modal(picker, window, cx);
    }

    /// Asks for a new branch's name, then creates it and switches to it.
    /// Focus goes back to `back` once it's answered or dismissed.
    fn ask_branch_name(
        &mut self,
        origin: Origin,
        back: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delegate = NamePicker {
            workbench: cx.entity().downgrade(),
            origin,
            name: String::new(),
            valid: false,
            back,
            _root: None,
        };
        let picker = cx.new(|cx| {
            let ui = ui_theme(cx);
            Picker::new(
                delegate,
                text!("workbench-chrome-branch-name-placeholder"),
                ui,
                cx,
            )
        });
        self.show_branch_modal(picker, window, cx);
    }

    /// Shows `picker`, focused, as [`Palette::Branch`], in place of any
    /// other palette, until it's picked from or dismissed, or the root
    /// changes.
    fn show_branch_modal<D: BranchModal>(
        &mut self,
        picker: Entity<Picker<D>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.show_palette(Palette::Branch, picker, window, cx);
    }

    /// The root branch commands start in now, if there's one.
    fn origin(&self) -> Option<Origin> {
        Some(Origin {
            root: self.root()?.to_owned(),
            git: self.git.entity_id(),
        })
    }

    /// Whether `origin` is still the root.
    fn is_current(&self, origin: &Origin) -> bool {
        self.git.entity_id() == origin.git
    }

    /// Where focus goes back to once a branch picker shown now is done:
    /// where it is, or if a modal shows, where that would have sent it (one
    /// of these pickers'), else the active editor or the workbench.
    fn return_focus(&self, window: &mut Window, cx: &App) -> Option<FocusHandle> {
        let Some(view) = self.modals.read(cx).modal_view().cloned() else {
            return window.focused(cx);
        };
        if let Ok(picker) = view.clone().downcast::<Picker<BranchPicker>>() {
            return picker.read(cx).delegate.back.clone();
        }
        if let Ok(picker) = view.downcast::<Picker<NamePicker>>() {
            return picker.read(cx).delegate.back.clone();
        }
        Some(match self.active_editor(cx) {
            Some(editor) => editor.focus_handle(cx),
            None => self.focus_handle(cx),
        })
    }

    /// Runs `switch` in `origin`'s repository in the background, answering
    /// `op` if it fails (in a toast without one). Nothing if the root's
    /// changed since.
    fn start_switch(
        &mut self,
        origin: Origin,
        switch: Switch,
        op: Option<GitOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current(&origin) {
            return log::info!("not switching to {}: the root changed", switch.name());
        }
        let root = origin.root.clone();
        let run = switch.clone();
        let outcome = cx.background_spawn(async move { run_switch(&root, &run) });
        cx.spawn_in(window, async move |workbench, cx| {
            let outcome = outcome.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.switched(origin, switch, op, outcome, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// A switch from `origin` landed: nothing more if the root's changed
    /// since.
    fn switched(
        &mut self,
        origin: Origin,
        switch: Switch,
        op: Option<GitOp>,
        outcome: Outcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = switch.name();
        if !self.is_current(&origin) {
            return log::info!("switching to {name} landed after the root changed");
        }
        let error = match outcome {
            Outcome::Done => {
                self.git.update(cx, |git, cx| git.refresh(cx));
                return self.check_open_files(window, cx);
            }
            Outcome::NoRepo => return self.answer_branch(op.as_ref(), no_repo(), cx),
            Outcome::Overwrite(paths, resolved) => match switch.target() {
                Some(target) => {
                    // Retries go to the ref the first attempt meant.
                    let target = resolved.map_or_else(|| target.clone(), Target::Ref);
                    return self.ask_overwrite(origin, target, paths, op, window, cx);
                }
                // Creating a branch at HEAD carries every change.
                None => SwitchError::Failed {
                    output: overwrite_detail(&paths),
                },
            },
            Outcome::Failed(error) => error,
        };
        let output = match error {
            SwitchError::InvalidName => {
                return self.answer_branch(op.as_ref(), invalid_name().to_string(), cx);
            }
            SwitchError::NotFound => {
                let answer = tr!("workbench-chrome-branch-not-found", name = &name);
                return self.answer_branch(op.as_ref(), answer.to_string(), cx);
            }
            SwitchError::Failed { output } => output,
            error => error.to_string(),
        };
        let message = match switch {
            Switch::Create(_) => tr!("workbench-chrome-branch-cant-create", name = &name),
            _ => tr!("workbench-chrome-branch-cant-switch", name = &name),
        };
        toast::show(cx, Toast::error(message).detail(output));
    }

    /// Asks whether to stash the local changes to `paths` and switch to
    /// `target`, switch dropping them, or cancel.
    fn ask_overwrite(
        &mut self,
        origin: Origin,
        target: Target,
        paths: Vec<String>,
        op: Option<GitOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let detail = overwrite_detail(&paths);
        let question = tr!("workbench-chrome-branch-overwrite");
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(&detail),
            &overwrite_buttons(),
            cx,
        );
        cx.spawn_in(window, async move |workbench, cx| {
            let switch = match answer.await {
                Ok(STASH) => Switch::Stash(target),
                Ok(FORCE) => Switch::Force(target),
                _ => return,
            };
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.start_switch(origin, switch, op, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// Answers `op` with the failure `text`, else shows it in a toast.
    fn answer_branch(&self, op: Option<&GitOp>, text: String, cx: &mut App) {
        match op {
            Some(op) => self.fail_git_op(op, text, cx),
            None => toast::show(cx, Toast::info(text)),
        }
    }
}

/// `head`'s name, with `*`, `+` and `!` as `marks` say, then `↑` and `↓`
/// with how far it is from `upstream`, each only if it's some way.
fn branch_label(head: &Head, marks: Marks, upstream: Option<&Upstream>) -> String {
    let mut text = head_name(head).to_owned();
    for (set, mark) in [
        (marks.working, '*'),
        (marks.staged, '+'),
        (marks.merging, '!'),
    ] {
        if set {
            text.push(mark);
        }
    }
    if let Some(counts) = crate::git::sync_counts(upstream) {
        text.push(' ');
        text.push_str(&counts);
    }
    text
}

/// HEAD's branch, or its short id when detached.
fn head_name(head: &Head) -> &str {
    match head {
        Head::Branch(name) | Head::Unborn(name) | Head::Detached(name) => name,
    }
}

/// Runs `switch` in the repository containing `root`. Runs on the
/// background executor, and blocks until git exits.
fn run_switch(root: &Path, switch: &Switch) -> Outcome {
    let repo = match Repo::discover(root) {
        Ok(Some(repo)) => repo,
        Ok(None) => return Outcome::NoRepo,
        Err(error) => return Outcome::Failed(error.into()),
    };
    let resolve = |target: &Target| match target {
        Target::Name(name) => repo.resolve_branch(name),
        Target::Ref(target) => Ok(target.clone()),
    };
    let mut resolved = None;
    let ran = match switch {
        Switch::Create(name) => repo.create_branch(name),
        Switch::To(target) | Switch::Force(target) | Switch::Stash(target) => resolve(target)
            .and_then(|branch| {
                let ran = match switch {
                    Switch::Force(_) => repo.switch_to(&branch, true),
                    Switch::Stash(_) => repo.stash_and_switch_to(&branch),
                    _ => repo.switch_to(&branch, false),
                };
                resolved = Some(branch);
                ran
            }),
    };
    match ran {
        Ok(()) => Outcome::Done,
        Err(SwitchError::WouldOverwrite { paths }) => {
            let paths = paths.iter().map(|path| {
                let relative = path.strip_prefix(repo.workdir()).unwrap_or(path);
                relative.display().to_string()
            });
            Outcome::Overwrite(paths.collect(), resolved)
        }
        Err(error) => Outcome::Failed(error),
    }
}

/// The question's detail: the first [`MAX_PATHS`] paths, a line each, then
/// `and N more`.
fn overwrite_detail(paths: &[String]) -> String {
    let mut lines: Vec<String> = paths.iter().take(MAX_PATHS).cloned().collect();
    if paths.len() > MAX_PATHS {
        let more = tr!(
            "workbench-chrome-branch-overwrite-more",
            count = paths.len() - MAX_PATHS
        );
        lines.push(more.to_string());
    }
    lines.join("\n")
}

/// Whether git would take `name` for a new branch, as
/// `git check-ref-format --branch` decides; `Repo::create_branch` asks git
/// itself all the same.
fn is_valid_name(name: &str) -> bool {
    let forbidden = |c: char| c.is_ascii_control() || " ~^:?*[\\".contains(c);
    !(name.is_empty()
        || name == "HEAD"
        || name.starts_with('-')
        || name.ends_with('.')
        || name.contains("..")
        || name.contains("@{")
        || name.chars().any(forbidden))
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

/// Seconds since the epoch.
fn now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs() as i64)
}

/// How long ago `seconds` was: `just now`, `5 minutes ago`, `1 day ago`.
fn ago(seconds: i64) -> SharedString {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let seconds = seconds.max(0);
    if seconds < MINUTE {
        tr!("workbench-chrome-branch-ago-now")
    } else if seconds < HOUR {
        tr!(
            "workbench-chrome-branch-ago-minutes",
            count = seconds / MINUTE
        )
    } else if seconds < DAY {
        tr!("workbench-chrome-branch-ago-hours", count = seconds / HOUR)
    } else if seconds < 14 * DAY {
        tr!("workbench-chrome-branch-ago-days", count = seconds / DAY)
    } else if seconds < 60 * DAY {
        tr!(
            "workbench-chrome-branch-ago-weeks",
            count = seconds / (7 * DAY)
        )
    } else if seconds < 365 * DAY {
        tr!(
            "workbench-chrome-branch-ago-months",
            count = seconds / (30 * DAY)
        )
    } else {
        tr!(
            "workbench-chrome-branch-ago-years",
            count = seconds / (365 * DAY)
        )
    }
}

/// `branch` by the kind of ref it is: a remote-tracking one by its
/// configured remote and the branch on it.
fn branch_ref(branch: &Branch) -> BranchRef {
    let Some(remote) = &branch.remote else {
        return BranchRef::Local(branch.name.clone());
    };
    let on_remote = (branch.name.strip_prefix(remote.as_str()))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(&branch.name);
    BranchRef::Remote {
        remote: remote.clone(),
        branch: on_remote.to_owned(),
    }
}

/// Focuses `back`, if there's one.
fn focus_back(back: Option<FocusHandle>, window: &mut Window, cx: &mut App) {
    if let Some(back) = back {
        window.focus(&back, cx);
    }
}

/// A branch picker: it follows the root, dismissed once it changes.
trait BranchModal: PickerDelegate {
    fn follow_root(&mut self, root_changed: Subscription);
}

/// `git.switch_branch`'s picker: [`CREATE_ROW`], then the branches
/// matching the query.
pub(crate) struct BranchPicker {
    workbench: WeakEntity<Workbench>,
    /// The root it lists the branches of.
    origin: Origin,
    /// Local branches then remote-tracking ones, each newest tip first, as
    /// [`Repo::branches`] lists them.
    branches: Vec<Branch>,
    /// The branches matching the query, as indices into `branches`: rows 1
    /// on.
    matches: Vec<usize>,
    /// When it opened, which the ages are from.
    now: i64,
    /// What had focus as it opened.
    back: Option<FocusHandle>,
    _root: Option<Subscription>,
}

impl BranchModal for BranchPicker {
    fn follow_root(&mut self, root_changed: Subscription) {
        self._root = Some(root_changed);
    }
}

impl BranchPicker {
    /// Row `ix`'s branch: none for the first.
    fn branch(&self, ix: usize) -> Option<&Branch> {
        let branch = *self.matches.get(ix.checked_sub(1)?)?;
        Some(&self.branches[branch])
    }
}

#[cfg(test)]
impl BranchPicker {
    /// Each row's label, with whether it's marked current.
    pub(crate) fn labels(&self) -> Vec<(String, bool)> {
        (0..self.match_count())
            .map(|ix| match self.branch(ix) {
                Some(branch) => (branch.name.clone(), branch.is_current),
                None => (create_row().to_string(), false),
            })
            .collect()
    }

    /// Each row's id.
    pub(crate) fn ids(&self) -> Vec<SharedString> {
        (0..self.match_count())
            .map(|ix| self.match_id(ix))
            .collect()
    }
}

impl PickerDelegate for BranchPicker {
    /// Branches whose name holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.trim().to_lowercase();
        self.matches = (self.branches.iter().enumerate())
            .filter(|(_, branch)| branch.name.to_lowercase().contains(&query))
            .map(|(ix, _)| ix)
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        1 + self.matches.len()
    }

    /// The branch's full ref name, distinct for a local `origin/feat` and
    /// the remote one; `create` for the first row.
    fn match_id(&self, ix: usize) -> SharedString {
        match self.branch(ix) {
            Some(Branch {
                name, remote: None, ..
            }) => format!("refs/heads/{name}").into(),
            Some(branch) => format!("refs/remotes/{}", branch.name).into(),
            None => "create".into(),
        }
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let muted = ui_theme(cx).muted_text;
        let Some(branch) = self.branch(ix) else {
            return div().child(create_row()).into_any_element();
        };
        let current = (branch.is_current).then(|| {
            div()
                .text_color(muted)
                .child(tr!("workbench-chrome-branch-current"))
        });
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_none().child(branch.name.clone()))
            .children(current)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(muted)
                    .child(branch.summary.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(muted)
                    .child(ago(self.now - branch.committed)),
            )
            .into_any_element()
    }

    /// The branch's name, then current if it's checked out; the first row's
    /// text for the first row.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let Some(branch) = self.branch(ix) else {
            return create_row();
        };
        let current = branch
            .is_current
            .then(|| tr!("workbench-chrome-branch-current"));
        let name = SharedString::from(branch.name.clone());
        joined_label([Some(name), current].into_iter().flatten())
    }

    /// Switches to the row's branch, or asks for a new one's name.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let workbench = self.workbench.clone();
        let origin = self.origin.clone();
        let Some(branch) = self.branch(ix) else {
            // Once this picker's gone: the name's picker takes its place,
            // and focus goes back where this one would have sent it.
            let back = self.back.take();
            window.defer(cx, move |window, cx| {
                (workbench.update(cx, |workbench, cx| {
                    workbench.ask_branch_name(origin, back, window, cx)
                }))
                .ok();
            });
            return true;
        };
        let switch = Switch::To(Target::Ref(branch_ref(branch)));
        focus_back(self.back.take(), window, cx);
        window.defer(cx, move |window, cx| {
            (workbench.update(cx, |workbench, cx| {
                workbench.start_switch(origin, switch, None, window, cx)
            }))
            .ok();
        });
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        focus_back(self.back.take(), window, cx);
    }
}

/// `git.create_branch`'s picker: the name typed, and a row that creates it
/// while git would take it.
pub(crate) struct NamePicker {
    workbench: WeakEntity<Workbench>,
    /// The root it creates the branch in.
    origin: Origin,
    /// The query, trimmed.
    name: String,
    /// Whether it's a name git would take.
    valid: bool,
    /// What had focus before it opened.
    back: Option<FocusHandle>,
    _root: Option<Subscription>,
}

impl BranchModal for NamePicker {
    fn follow_root(&mut self, root_changed: Subscription) {
        self._root = Some(root_changed);
    }
}

#[cfg(test)]
impl NamePicker {
    /// Its rows' labels.
    pub(crate) fn labels(&self) -> Vec<String> {
        (0..self.match_count()).map(|_| self.label()).collect()
    }
}

impl NamePicker {
    fn label(&self) -> String {
        tr!("workbench-chrome-branch-create", name = &self.name).to_string()
    }
}

impl PickerDelegate for NamePicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.name = query.trim().to_owned();
        self.valid = is_valid_name(&self.name);
        cx.notify();
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        usize::from(self.valid)
    }

    fn match_id(&self, _: usize) -> SharedString {
        "create".into()
    }

    fn render_match(&self, _: usize, _: bool, _: &App) -> AnyElement {
        div().child(self.label()).into_any_element()
    }

    /// The row's text.
    fn match_label(&self, _: usize, _: &App) -> SharedString {
        self.label().into()
    }

    /// Creates the branch and switches to it.
    fn confirm(&mut self, _: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let workbench = self.workbench.clone();
        let origin = self.origin.clone();
        let switch = Switch::Create(self.name.clone());
        focus_back(self.back.take(), window, cx);
        window.defer(cx, move |window, cx| {
            (workbench.update(cx, |workbench, cx| {
                workbench.start_switch(origin, switch, None, window, cx)
            }))
            .ok();
        });
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        focus_back(self.back.take(), window, cx);
    }

    fn status(&self) -> Option<SharedString> {
        (!self.name.is_empty() && !self.valid).then(invalid_name)
    }
}

#[cfg(test)]
#[path = "git_branch_tests.rs"]
mod git_branch_tests;
#[cfg(test)]
#[path = "git_branch_typed_tests.rs"]
mod git_branch_typed_tests;
