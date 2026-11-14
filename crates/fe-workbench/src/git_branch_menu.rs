//! The branches menu, `git.branches_menu`, which the title bar's branch
//! widget runs, as JetBrains' Git branches popup: drawn under the widget (at
//! the top of the window with no title bar), over everything, as
//! [`Palette::Branch`].
//!
//! - **Search.** Its first row, the `Search branches` field, has focus and
//!   takes what's typed, composed or pasted, with a text field's editing
//!   keys. Everything below keeps only what holds the
//!   text, ignoring case: the actions by title, the branches by name.
//! - **Actions.** Update Project (`git.update_project`: fetch, then
//!   fast-forward), Commit… (the Changes panel and its message box), Push
//!   (Publish Branch with no upstream) and Fetch; then New Branch…, which
//!   asks for the name of a branch to make at HEAD and switch to.
//! - **Branches.** Recent, the last five local branches checked out (from
//!   the reflog), newest first; Local, newest tip first; and Remote, under
//!   each remote's name. Each row shows the branch's name, a check on the
//!   one checked out, how far a local branch is from its upstream as the
//!   title bar does (`↑2 ↓1`), its tip's summary and age, and a chevron.
//! - **Branch actions.** A click on a branch, Enter or Right on it, or the
//!   pointer resting on its chevron, opens its actions beside it: Checkout,
//!   New Branch from 'x'… (asks for a name, then makes it at x's tip and
//!   switches to it), Merge 'x' into 'y' (while HEAD is on a branch),
//!   Rename… (a local branch: asks for the new name, the old one to start
//!   with) and Delete (a local branch but the current one: a branch that
//!   isn't fully merged asks first). The current branch has New Branch
//!   from, Rename, Push, Pull and Update instead; a remote-tracking one
//!   Checkout (a local branch tracking it), New Branch from and Merge.
//!   Left or Escape closes them.
//!
//! The widget's click, or `git.branches_menu` from a key, closes it while
//! it shows. Up and Down move the highlight, wrapping, over the actions and the
//! branches, or the open branch's actions; Enter chooses it. Escape, a press
//! outside it or focus leaving closes the menu; so does a new root. Each
//! action closes it, then runs; how it went is a toast, and git is read
//! again after each.

use std::{cell::Cell, path::Path, rc::Rc};

use fe_git::{Branch, BranchError, BranchRef, MergeError, Repo, Upstream};
use fe_ui::{
    ActiveTheme as _, Icon, MenuEntry, MenuPanel, Role, States, TextInput, TextInputEvent,
};
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseDownEvent, ParentElement as _, Pixels, Point, PromptButton, PromptLevel, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    WeakEntity, Window, anchored, canvas, deferred, div, point, prelude::FluentBuilder as _, px,
    svg,
};
use kiln_l10n::{text, tr, verbatim};
use kiln_shell::ModalView;

use super::{
    NameFor, Origin, Palette, RootChanged, Switch, Target, Workbench, ago, branch_ref, now,
    toast::{self, Toast},
};
use fe_gpui::commands::RunCommand;

use crate::{
    dock::DockOp,
    picker::{self, PaletteOp},
    workbench::git_commands::{GitCommand, git_remote::git_read},
};

/// The menu's key context: no binding names it, so the keys it takes reach
/// it.
const KEY_CONTEXT: &str = verbatim!("git_branches_menu");

/// How wide the menu is.
const WIDTH: Pixels = px(460.);

/// How tall its rows get before they scroll.
const MAX_ROWS_HEIGHT: Pixels = px(480.);

/// How many recent branches it lists.
const RECENT: usize = 5;

/// How many reflog entries are read for them.
const REFLOG_DEPTH: &str = "500";

/// The local branches checked out last, newest first, as indices into
/// `branches`: from HEAD's reflog, each `checkout: moving from a to b` naming
/// b, then a. Runs on the background executor, and blocks until git exits.
pub(super) fn recent_branches(repo: &Repo, branches: &[Branch]) -> Vec<usize> {
    let args = [
        "reflog",
        "show",
        "--format=%gs",
        "-n",
        REFLOG_DEPTH,
        "HEAD",
        "--",
    ];
    // A repository with no commits has no reflog.
    let Ok(log) = git_read(repo, &args) else {
        return Vec::new();
    };
    let mut recent = Vec::new();
    for line in log.lines() {
        let Some(moved) = line.strip_prefix("checkout: moving from ") else {
            continue;
        };
        let Some((from, to)) = moved.rsplit_once(verbatim!(" to ")) else {
            continue;
        };
        for name in [to, from] {
            let local = (branches.iter()).position(|b| b.remote.is_none() && b.name == name);
            if let Some(ix) = local
                && !recent.contains(&ix)
            {
                recent.push(ix);
                if recent.len() == RECENT {
                    return recent;
                }
            }
        }
    }
    recent
}

/// A row of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Action(Action),
    Separator,
    Title(Section),
    /// A remote's name, over its branches.
    Remote(String),
    /// A branch in a section, by its index in the listing.
    Branch(Section, usize),
    /// What shows when nothing holds the search.
    NoMatch,
}

impl Row {
    /// Whether the highlight stops on it.
    fn is_item(&self) -> bool {
        matches!(self, Row::Action(_) | Row::Branch(..))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Recent,
    Local,
    Remote,
}

impl Section {
    fn title(self) -> SharedString {
        match self {
            Section::Recent => tr!("workbench-git-branches-recent"),
            Section::Local => tr!("workbench-git-branches-local"),
            Section::Remote => tr!("workbench-git-branches-remote"),
        }
    }
}

/// The actions over the branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Update,
    Commit,
    Push,
    Fetch,
    NewBranch,
}

/// A branch's actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BranchAction {
    Checkout,
    NewBranchFrom,
    Merge,
    Rename,
    Delete,
    Push,
    Pull,
    Update,
}

/// A branch's actions, open beside its row.
struct Submenu {
    /// The branch's row.
    row: usize,
    items: Vec<BranchAction>,
    highlighted: Option<usize>,
    /// Where it was drawn last, which a press in doesn't close the menu.
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

/// The open branches menu, a modal of the workbench's.
pub(crate) struct BranchMenu {
    workbench: WeakEntity<Workbench>,
    /// The root it lists the branches of.
    origin: Origin,
    /// The search field, which has focus while it shows.
    input: Entity<TextInput>,
    /// Its top left corner, in the window.
    position: Point<Pixels>,
    /// Local branches then remote-tracking ones, as [`Repo::branches`]
    /// lists them.
    branches: Vec<Branch>,
    /// The recent branches, newest first, as indices into `branches`.
    recent: Vec<usize>,
    /// When it opened, which the ages are from.
    now: i64,
    /// What's typed in the search field.
    query: String,
    /// What it shows for `query`.
    rows: Vec<Row>,
    /// The row Up and Down moved to, or the pointer is over.
    highlighted: Option<usize>,
    submenu: Option<Submenu>,
    scroll: ScrollHandle,
    /// Where focus goes back to once an action it runs is done with.
    back: Option<FocusHandle>,
    _root: Subscription,
    _input: Subscription,
}

impl EventEmitter<DismissEvent> for BranchMenu {}

impl Focusable for BranchMenu {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl ModalView for BranchMenu {
    /// As Escape: see [`BranchMenu::close`].
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }
}

impl Workbench {
    /// Shows the branches menu for `origin`'s `branches`, under the title
    /// bar's branch widget if it's drawn, in place of any other palette.
    pub(super) fn show_branches_menu(
        &mut self,
        origin: Origin,
        branches: Vec<Branch>,
        recent: Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = match crate::title_bar::branch_anchor(window, cx) {
            Some(widget) => widget.bottom_left() + point(px(0.), px(2.)),
            None => {
                let width = window.viewport_size().width;
                point(((width - WIDTH) / 2.).max(px(0.)), px(48.))
            }
        };
        let back = self.return_focus(window, cx);
        let workbench = cx.entity();
        let menu = cx.new(|cx| {
            let root = cx.subscribe_in(
                &workbench,
                window,
                |menu: &mut BranchMenu, _, _: &RootChanged, window, cx| menu.close(window, cx),
            );
            let input = cx.new(|cx| {
                let placeholder = text!("workbench-git-branches-search");
                TextInput::new(placeholder, &fe_ui::ui_theme(cx), cx).search()
            });
            let typed = cx.subscribe(&input, |menu: &mut BranchMenu, input, event, cx| {
                if *event == TextInputEvent::Changed {
                    menu.query = input.read(cx).text().to_owned();
                    menu.requery(cx);
                }
            });
            let mut menu = BranchMenu {
                workbench: workbench.downgrade(),
                origin,
                input,
                position,
                branches,
                recent,
                now: now(),
                query: String::new(),
                rows: Vec::new(),
                highlighted: None,
                submenu: None,
                scroll: ScrollHandle::new(),
                back,
                _root: root,
                _input: typed,
            };
            menu.rows = menu.matching_rows();
            menu
        });
        self.modals.update(cx, |layer, cx| {
            layer.show_modal(Palette::Branch.id(), menu, window, cx)
        });
        self.palette = Some(Palette::Branch);
        cx.notify();
    }

    /// Closes the branches menu if it shows: `git.branches_menu` while it
    /// does, as a second click on the widget. Whether it did.
    pub(super) fn close_branches_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let view = self.modals.read(cx).modal_view().cloned();
        let Some(menu) = view.and_then(|view| view.downcast::<BranchMenu>().ok()) else {
            return false;
        };
        menu.update(cx, |menu, cx| menu.close(window, cx));
        true
    }

    /// Merges `source` into HEAD's branch in `origin`'s repository, in the
    /// background, in its turn among the work that changes it.
    fn start_merge(
        &mut self,
        origin: Origin,
        source: BranchRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current(&origin) {
            return;
        }
        let (root, merged) = (origin.root.clone(), source.clone());
        self.queue_branch_work(
            move || {
                let Some(repo) = Repo::discover(&root)? else {
                    return Ok(None);
                };
                let into = repo.current_branch()?;
                repo.merge_into_current(&merged)?;
                Ok(Some(super::head_name(&into).to_owned()))
            },
            move |workbench, outcome, window, cx| {
                workbench.merged(&origin, &source, outcome, window, cx)
            },
            window,
            cx,
        );
    }

    /// A merge of `source` from `origin` landed: how it went is said, and
    /// the open files are checked, whatever the root is by then, as a new
    /// root keeps the tabs but not the watcher that would have noticed; git
    /// is only read again if the root's still the one it ran in.
    fn merged(
        &mut self,
        origin: &Origin,
        source: &BranchRef,
        outcome: Result<Option<String>, MergeError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = Target::Ref(source.clone()).name();
        let toast = match outcome {
            Ok(Some(into)) => Toast::info(tr!(
                "workbench-git-branches-merged",
                name = name.as_str(),
                into = into
            )),
            Ok(None) => Toast::info(super::no_repo()),
            Err(MergeError::Conflicts { paths }) => Toast::error(tr!(
                "workbench-git-branches-merge-conflicts",
                count = paths.len()
            ))
            .detail(relative(&paths, &origin.root)),
            Err(MergeError::WouldOverwrite { paths }) => Toast::error(tr!(
                "workbench-git-branches-merge-overwrite",
                name = name.as_str()
            ))
            .detail(relative(&paths, &origin.root)),
            Err(error) => Toast::error(tr!(
                "workbench-git-branches-cant-merge",
                name = name.as_str()
            ))
            .detail(error.to_string()),
        };
        self.branches_changed(self.is_current(origin), true, window, cx);
        toast::show(cx, toast);
    }

    /// Renames local branch `old` to `new` in `origin`'s repository, in the
    /// background, in its turn among the work that changes it.
    pub(super) fn start_rename(
        &mut self,
        origin: Origin,
        old: String,
        new: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current(&origin) {
            return;
        }
        let root = origin.root.clone();
        let (from, to) = (old.clone(), new.clone());
        self.queue_branch_work(
            move || match Repo::discover(&root)? {
                Some(repo) => repo.rename_branch(&from, &to).map(Some),
                None => Ok(None),
            },
            move |workbench, outcome, window, cx| {
                if !workbench.is_current(&origin) {
                    return log::info!("renaming {old} landed after the root changed");
                }
                let (old, new) = (old.as_str(), new.as_str());
                let toast = match outcome {
                    Ok(Some(())) => {
                        Toast::info(tr!("workbench-git-branches-renamed", old = old, name = new))
                    }
                    Ok(None) => Toast::info(super::no_repo()),
                    Err(error) => {
                        Toast::error(tr!("workbench-git-branches-cant-rename", old = old))
                            .detail(error.to_string())
                    }
                };
                workbench.branches_changed(true, false, window, cx);
                toast::show(cx, toast);
            },
            window,
            cx,
        );
    }

    /// Deletes local branch `name` in `origin`'s repository, in the
    /// background, in its turn among the work that changes it. One that
    /// isn't fully merged asks first; with `confirmed`, the tip a yes was
    /// given for, it's deleted all the same, unless its tip has moved since,
    /// which asks again.
    fn start_delete(
        &mut self,
        origin: Origin,
        name: String,
        confirmed: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current(&origin) {
            return;
        }
        let (root, deleted) = (origin.root.clone(), name.clone());
        self.queue_branch_work(
            move || run_delete(&root, &deleted, confirmed.as_deref()),
            move |workbench, outcome, window, cx| {
                workbench.deleted(origin, name, outcome, window, cx)
            },
            window,
            cx,
        );
    }

    /// A delete of `name` from `origin` landed.
    fn deleted(
        &mut self,
        origin: Origin,
        name: String,
        outcome: Deleted,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_current(&origin) {
            return log::info!("deleting {name} landed after the root changed");
        }
        let toast = match outcome {
            Deleted::Done => {
                Toast::info(tr!("workbench-git-branches-deleted", name = name.as_str()))
            }
            Deleted::NoRepo => Toast::info(super::no_repo()),
            Deleted::NotMerged(tip) => {
                return self.ask_delete_unmerged(origin, name, tip, window, cx);
            }
            // As if asked for now: a question for its new tip, if it still
            // isn't merged.
            Deleted::Moved => return self.start_delete(origin, name, None, window, cx),
            Deleted::Failed(error) => Toast::error(tr!(
                "workbench-git-branches-cant-delete",
                name = name.as_str()
            ))
            .detail(error.to_string()),
        };
        self.branches_changed(true, false, window, cx);
        toast::show(cx, toast);
    }

    /// Asks whether to delete `name`, which isn't fully merged, at `tip`,
    /// then does on a yes, if that's still its tip.
    fn ask_delete_unmerged(
        &mut self,
        origin: Origin,
        name: String,
        tip: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let question = tr!("workbench-git-branches-not-merged", name = name.as_str());
        let detail = tr!("workbench-git-branches-not-merged-detail");
        let buttons = [
            PromptButton::Ok(tr!("workbench-git-branches-delete-anyway")),
            PromptButton::Cancel(tr!("workbench-git-branches-cancel")),
        ];
        let answer = window.prompt(PromptLevel::Warning, &question, Some(&detail), &buttons, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            if answer.await != Ok(0) {
                return;
            }
            (workbench.update_in(cx, |workbench, window, cx| {
                workbench.start_delete(origin, name, Some(tip), window, cx)
            }))
            .ok();
        })
        .detach();
    }
}

/// How a delete went.
enum Deleted {
    Done,
    NoRepo,
    /// The branch isn't fully merged; its tip, which a yes is given for.
    NotMerged(String),
    /// Its tip isn't the one a yes was given for any more: nothing was
    /// deleted.
    Moved,
    Failed(BranchError),
}

/// Deletes local branch `name` in the repository containing `root`; with
/// `confirmed`, even if it isn't fully merged, if its tip is still that.
/// Runs on the background executor, and blocks until git exits.
fn run_delete(root: &Path, name: &str, confirmed: Option<&str>) -> Deleted {
    let repo = match Repo::discover(root) {
        Ok(Some(repo)) => repo,
        Ok(None) => return Deleted::NoRepo,
        Err(error) => return Deleted::Failed(error.into()),
    };
    if let Some(confirmed) = confirmed
        && tip(&repo, name).as_deref() != Some(confirmed)
    {
        return Deleted::Moved;
    }
    match repo.delete_branch(name, confirmed.is_some()) {
        Ok(()) => Deleted::Done,
        Err(BranchError::NotMerged) => match tip(&repo, name) {
            Some(tip) => Deleted::NotMerged(tip),
            None => Deleted::Moved,
        },
        Err(error) => Deleted::Failed(error),
    }
}

/// Local branch `name`'s commit, if it has one. Blocks until git exits.
fn tip(repo: &Repo, name: &str) -> Option<String> {
    let full = format!("refs/heads/{name}^{{commit}}");
    let tip = git_read(repo, &["rev-parse", "--verify", "-q", &full]).ok()?;
    (!tip.is_empty()).then_some(tip)
}

/// `paths`, a line each, relative to `root` where they're in it.
fn relative(paths: &[std::path::PathBuf], root: &Path) -> String {
    (paths.iter())
        .map(|path| {
            let shown = path.strip_prefix(root).unwrap_or(path);
            shown.display().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl BranchMenu {
    /// Where focus goes back to once an action it runs is done with.
    pub(super) fn back(&self) -> Option<FocusHandle> {
        self.back.clone()
    }

    /// Closes it, focus going back to where it was as it opened, unless
    /// something else has taken it since.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.focus_handle(cx).contains_focused(window, cx)
            && let Some(back) = &self.back
        {
            window.focus(back, cx);
        }
        cx.emit(DismissEvent);
    }

    /// The branch checked out, if HEAD's on one.
    fn current(&self) -> Option<&Branch> {
        (self.branches.iter()).find(|branch| branch.is_current && branch.remote.is_none())
    }

    fn title(&self, action: Action) -> SharedString {
        match action {
            Action::Update => tr!("workbench-git-branches-update"),
            Action::Commit => tr!("workbench-git-branches-commit"),
            Action::Push if self.current().is_some_and(|b| b.upstream.is_none()) => {
                tr!("workbench-git-branches-publish")
            }
            Action::Push => tr!("workbench-git-branches-push"),
            Action::Fetch => tr!("workbench-git-branches-fetch"),
            Action::NewBranch => tr!("workbench-git-branches-new"),
        }
    }

    /// The rows for the query: the actions whose title holds it, then each
    /// section's branches whose name does, sections with none left out.
    fn matching_rows(&self) -> Vec<Row> {
        let query = self.query.trim().to_lowercase();
        let holds = |text: &str| text.to_lowercase().contains(&query);
        let mut rows = Vec::new();
        for action in [Action::Update, Action::Commit, Action::Push, Action::Fetch] {
            if holds(&self.title(action)) {
                rows.push(Row::Action(action));
            }
        }
        if holds(&self.title(Action::NewBranch)) {
            if !rows.is_empty() {
                rows.push(Row::Separator);
            }
            rows.push(Row::Action(Action::NewBranch));
        }
        let branch_matches = |ix: &usize| holds(&self.branches[*ix].name);
        let recent: Vec<usize> = self.recent.iter().copied().filter(branch_matches).collect();
        let local: Vec<usize> = (0..self.branches.len())
            .filter(|&ix| self.branches[ix].remote.is_none())
            .filter(branch_matches)
            .collect();
        let mut remotes: Vec<(&str, Vec<usize>)> = Vec::new();
        for ix in (0..self.branches.len()).filter(branch_matches) {
            let Some(remote) = self.branches[ix].remote.as_deref() else {
                continue;
            };
            match remotes.iter_mut().find(|(name, _)| *name == remote) {
                Some((_, branches)) => branches.push(ix),
                None => remotes.push((remote, vec![ix])),
            }
        }
        remotes.sort_by(|a, b| a.0.cmp(b.0));
        for (section, branches) in [(Section::Recent, recent), (Section::Local, local)] {
            if branches.is_empty() {
                continue;
            }
            if !rows.is_empty() {
                rows.push(Row::Separator);
            }
            rows.push(Row::Title(section));
            rows.extend(branches.into_iter().map(|ix| Row::Branch(section, ix)));
        }
        if !remotes.is_empty() {
            if !rows.is_empty() {
                rows.push(Row::Separator);
            }
            rows.push(Row::Title(Section::Remote));
            for (remote, branches) in remotes {
                rows.push(Row::Remote(remote.to_owned()));
                rows.extend(
                    branches
                        .into_iter()
                        .map(|ix| Row::Branch(Section::Remote, ix)),
                );
            }
        }
        if rows.is_empty() {
            rows.push(Row::NoMatch);
        }
        rows
    }

    /// Shows the rows for the query, the first branch highlighted, else
    /// the first action, with no branch's actions open.
    fn requery(&mut self, cx: &mut Context<Self>) {
        self.rows = self.matching_rows();
        self.submenu = None;
        let first_branch = (self.rows.iter()).position(|row| matches!(row, Row::Branch(..)));
        self.highlighted = first_branch.or_else(|| self.rows.iter().position(Row::is_item));
        self.scroll_to_highlighted();
        cx.notify();
    }

    fn scroll_to_highlighted(&self) {
        if let Some(ix) = self.highlighted {
            self.scroll.scroll_to_item(ix);
        }
    }

    /// The search field's editing commands (`text_input.backspace`,
    /// `text_input.paste` and the like, which the input leaves to its
    /// host); others go on.
    fn on_input_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = picker::op(run, cx).and_then(PaletteOp::edit) else {
            return cx.propagate();
        };
        self.input.update(cx, edit);
    }

    /// The keys it takes: see the module docs. Others, and any with a
    /// modifier but Shift, go on to the bindings.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        match keystroke.key.as_str() {
            "escape" if self.submenu.is_some() => self.close_submenu(cx),
            "escape" => self.close(window, cx),
            "up" | "down" => self.move_highlight(keystroke.key == "down", cx),
            "right" => self.open_highlighted_submenu(cx),
            "left" => self.close_submenu(cx),
            "enter" => self.choose(window, cx),
            // The rest is the search field's.
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Moves the highlight to the next item, or with `down` false the one
    /// before, wrapping, in the open branch's actions if they're open.
    fn move_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        if let Some(submenu) = &mut self.submenu {
            let count = submenu.items.len();
            submenu.highlighted = Some(step(submenu.highlighted, down, count));
            return cx.notify();
        }
        let items: Vec<usize> = (self.rows.iter().enumerate())
            .filter(|(_, row)| row.is_item())
            .map(|(ix, _)| ix)
            .collect();
        if items.is_empty() {
            return;
        }
        let at = (self.highlighted).and_then(|ix| items.iter().position(|&item| item == ix));
        self.highlighted = Some(items[step(at, down, items.len())]);
        self.scroll_to_highlighted();
        cx.notify();
    }

    /// Opens the actions of the highlighted branch, the first highlighted.
    fn open_highlighted_submenu(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.highlighted {
            self.open_submenu(row, Some(0), cx);
        }
    }

    /// Opens the actions of the branch on `row`, if it is one, with
    /// `highlighted` highlighted; nothing if they're open already.
    fn open_submenu(&mut self, row: usize, highlighted: Option<usize>, cx: &mut Context<Self>) {
        if self
            .submenu
            .as_ref()
            .is_some_and(|submenu| submenu.row == row)
        {
            return;
        }
        let Some(Row::Branch(_, branch)) = self.rows.get(row) else {
            return;
        };
        let items = self.branch_actions(&self.branches[*branch]);
        self.highlighted = Some(row);
        self.submenu = Some(Submenu {
            row,
            items,
            highlighted,
            bounds: Rc::default(),
        });
        cx.notify();
    }

    fn close_submenu(&mut self, cx: &mut Context<Self>) {
        if self.submenu.take().is_some() {
            cx.notify();
        }
    }

    /// What `branch` offers: see the module docs.
    fn branch_actions(&self, branch: &Branch) -> Vec<BranchAction> {
        use BranchAction::*;
        let on_branch = self.current().is_some();
        if branch.remote.is_some() {
            let mut actions = vec![Checkout, NewBranchFrom];
            actions.extend(on_branch.then_some(Merge));
            return actions;
        }
        if branch.is_current {
            return vec![NewBranchFrom, Rename, Push, Pull, Update];
        }
        let mut actions = vec![Checkout, NewBranchFrom];
        actions.extend(on_branch.then_some(Merge));
        actions.extend([Rename, Delete]);
        actions
    }

    fn branch_action_title(&self, action: BranchAction, branch: &Branch) -> SharedString {
        let name = branch.name.as_str();
        match action {
            BranchAction::Checkout => tr!("workbench-git-branches-checkout"),
            BranchAction::NewBranchFrom => tr!("workbench-git-branches-new-from", name = name),
            BranchAction::Merge => {
                let into = self.current().map_or("", |current| current.name.as_str());
                tr!("workbench-git-branches-merge", name = name, into = into)
            }
            BranchAction::Rename => tr!("workbench-git-branches-rename"),
            BranchAction::Delete => tr!("workbench-git-branches-delete"),
            BranchAction::Push => tr!("workbench-git-branches-push"),
            BranchAction::Pull => tr!("workbench-git-branches-pull"),
            BranchAction::Update => tr!("workbench-git-branches-update-branch"),
        }
    }

    /// Enter: runs the highlighted action, or opens the highlighted
    /// branch's actions.
    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(submenu) = &self.submenu {
            if let Some(ix) = submenu.highlighted {
                self.run_branch_action(ix, window, cx);
            }
            return;
        }
        match self.highlighted.and_then(|ix| self.rows.get(ix)) {
            Some(Row::Action(action)) => self.run_action(*action, window, cx),
            Some(Row::Branch(..)) => self.open_highlighted_submenu(cx),
            _ => {}
        }
    }

    /// Closes the menu, then runs `action`.
    fn run_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let (origin, back) = (self.origin.clone(), self.back.clone());
        self.then(window, cx, move |workbench, window, cx| match action {
            Action::Update => workbench.run_git_command(GitCommand::UpdateProject, window, cx),
            Action::Commit => workbench.run_dock_op(DockOp::ShowChanges, window, cx),
            Action::Push => workbench.run_git_command(GitCommand::Push, window, cx),
            Action::Fetch => workbench.run_git_command(GitCommand::Fetch, window, cx),
            Action::NewBranch => workbench.ask_branch_name(origin, NameFor::Head, back, window, cx),
        });
    }

    /// Closes the menu, then runs the open branch's action `ix`.
    fn run_branch_action(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(submenu) = &self.submenu else {
            return;
        };
        let Some(&action) = submenu.items.get(ix) else {
            return;
        };
        let Some(Row::Branch(_, branch)) = self.rows.get(submenu.row) else {
            return;
        };
        let branch = &self.branches[*branch];
        let (target, name) = (branch_ref(branch), branch.name.clone());
        let (origin, back) = (self.origin.clone(), self.back.clone());
        self.then(window, cx, move |workbench, window, cx| match action {
            BranchAction::Checkout => {
                let switch = Switch::To(Target::Ref(target));
                workbench.start_switch(origin, switch, None, window, cx)
            }
            BranchAction::NewBranchFrom => {
                workbench.ask_branch_name(origin, NameFor::From(target), back, window, cx)
            }
            BranchAction::Merge => workbench.start_merge(origin, target, window, cx),
            BranchAction::Rename => {
                workbench.ask_branch_name(origin, NameFor::Rename(name), back, window, cx)
            }
            BranchAction::Delete => workbench.start_delete(origin, name, None, window, cx),
            BranchAction::Push => workbench.run_git_command(GitCommand::Push, window, cx),
            BranchAction::Pull => workbench.run_git_command(GitCommand::Pull, window, cx),
            BranchAction::Update => {
                workbench.run_git_command(GitCommand::UpdateProject, window, cx)
            }
        });
    }

    /// Closes the menu, then, once it's gone and focus is back, runs `run`
    /// on the workbench.
    fn then(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        run: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
    ) {
        let workbench = self.workbench.clone();
        self.close(window, cx);
        window.defer(cx, move |window, cx| {
            (workbench.update(cx, |workbench, cx| run(workbench, window, cx))).ok();
        });
    }

    /// The pointer over row `ix`: highlights it, closing another branch's
    /// actions.
    fn hover(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.highlighted == Some(ix) || !self.rows.get(ix).is_some_and(Row::is_item) {
            return;
        }
        self.highlighted = Some(ix);
        if self
            .submenu
            .as_ref()
            .is_some_and(|submenu| submenu.row != ix)
        {
            self.submenu = None;
        }
        cx.notify();
    }

    /// A click on row `ix`: runs its action, or opens its branch's actions.
    fn click(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.rows.get(ix) {
            Some(Row::Action(action)) => self.run_action(*action, window, cx),
            Some(Row::Branch(..)) => self.open_submenu(ix, None, cx),
            _ => {}
        }
    }

    /// A press outside the menu, which closes it, unless it's in the open
    /// branch's actions, drawn beside it, or on the branch widget, whose
    /// click closes it instead (`git.branches_menu` while it shows).
    fn pressed_outside(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let in_submenu = (self.submenu.as_ref())
            .and_then(|submenu| submenu.bounds.get())
            .is_some_and(|bounds| bounds.contains(&event.position));
        let on_widget = crate::title_bar::branch_anchor(window, cx)
            .is_some_and(|widget| widget.contains(&event.position));
        if !in_submenu && !on_widget {
            self.close(window, cx);
        }
    }

    /// The search field, over the rows.
    fn render_search(&self, cx: &App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        div()
            .debug_selector(|| "git-branches-search".into())
            .pb_1()
            .mb_1()
            .border_b_1()
            .border_color(colors.border.default)
            .child(self.input.clone())
    }

    /// Row `ix`, an item: `leading`, a check's width, then `label`.
    fn item(
        &self,
        ix: usize,
        leading: gpui::Div,
        label: SharedString,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let colors = cx.theme().colors;
        div()
            .id(("git-branches-row", ix))
            .debug_selector(move || format!("git-branches-row:{ix}"))
            .flex()
            .items_center()
            .gap_2()
            .h(px(24.))
            .px_2()
            .rounded_sm()
            .when(self.highlighted == Some(ix), |row| {
                row.bg(colors.list.row_selected)
                    .text_color(colors.tab.active_text)
            })
            .on_mouse_move(cx.listener(move |menu, _, _, cx| menu.hover(ix, cx)))
            .on_click(cx.listener(move |menu, _, window, cx| {
                cx.stop_propagation();
                menu.click(ix, window, cx)
            }))
            .child(leading.size(px(14.)).flex_none())
            .child(div().flex_none().max_w(px(220.)).truncate().child(label))
    }

    fn render_row(&self, ix: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let muted = colors.text.muted;
        let title = |text: SharedString| {
            div()
                .flex()
                .items_center()
                .h(px(22.))
                .px_3()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(text)
                .into_any_element()
        };
        match &self.rows[ix] {
            Row::Separator => div()
                .my_1()
                .h_px()
                .bg(colors.border.default)
                .into_any_element(),
            Row::Title(section) => title(section.title()),
            Row::Remote(remote) => title(SharedString::from(remote.clone())),
            Row::NoMatch => div()
                .px_3()
                .h(px(24.))
                .text_color(muted)
                .child(tr!("workbench-git-branches-no-match"))
                .into_any_element(),
            Row::Action(action) => {
                let title = self.title(*action);
                let row = self.item(ix, div(), title.clone(), cx);
                fe_ui::node(row, window, cx, Role::MenuItem, title).into_any_element()
            }
            Row::Branch(section, branch) => {
                let branch = &self.branches[*branch];
                self.render_branch(ix, *section, branch, window, cx)
            }
        }
    }

    fn render_branch(
        &self,
        ix: usize,
        section: Section,
        branch: &Branch,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let muted = colors.text.muted;
        // Under its remote's name, a remote-tracking branch by the branch on
        // it.
        let shown = match (&branch.remote, section) {
            (Some(remote), Section::Remote) => (branch.name.strip_prefix(remote.as_str()))
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or(&branch.name),
            _ => &branch.name,
        };
        let counts = branch_counts(branch);
        let check = div().when(branch.is_current, |check| {
            check.child(
                svg()
                    .path(Icon::Check.path())
                    .size(px(14.))
                    .text_color(colors.text.primary),
            )
        });
        let open = self.submenu.as_ref().filter(|submenu| submenu.row == ix);
        let chevron = div()
            .id(("git-branches-chevron", ix))
            .debug_selector(move || format!("git-branches-chevron:{ix}"))
            .flex_none()
            .on_mouse_move(cx.listener(move |menu, _, _, cx| menu.open_submenu(ix, None, cx)))
            .child(
                svg()
                    .path(Icon::ChevronRight.path())
                    .size(px(14.))
                    .text_color(muted),
            );
        let row = (self.item(ix, check, SharedString::from(shown.to_owned()), cx))
            .relative()
            .children(
                counts
                    .clone()
                    .map(|counts| div().flex_none().text_color(muted).child(counts)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(muted)
                    .child(SharedString::from(branch.summary.clone())),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(muted)
                    .child(ago(self.now - branch.committed)),
            )
            .child(chevron)
            .children(open.map(|submenu| self.render_submenu(submenu, branch, cx)));
        let mut states = States {
            expanded: Some(open.is_some()),
            ..States::default()
        };
        let current = branch
            .is_current
            .then(|| tr!("workbench-chrome-branch-current"));
        let description: Vec<SharedString> = [current, counts.map(SharedString::from)]
            .into_iter()
            .flatten()
            .collect();
        if !description.is_empty() {
            states.description = Some(kiln_shell::picker::joined_label(description));
        }
        let label = SharedString::from(branch.name.clone());
        fe_ui::node_with(row, window, cx, Role::MenuItem, label, states).into_any_element()
    }

    /// The open branch's actions, beside its row.
    fn render_submenu(
        &self,
        submenu: &Submenu,
        branch: &Branch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entries = (submenu.items.iter())
            .map(|&action| MenuEntry::Item {
                title: self.branch_action_title(action, branch),
                shortcut: None,
            })
            .collect();
        let ui = fe_ui::ui_theme(cx);
        let panel = MenuPanel::new(entries, &ui)
            .highlighted(submenu.highlighted)
            .on_click({
                let menu = cx.weak_entity();
                move |ix, window, cx| {
                    (menu.update(cx, |menu, cx| menu.run_branch_action(ix, window, cx))).ok();
                }
            })
            .on_hover({
                let menu = cx.weak_entity();
                move |ix, _, cx| {
                    (menu.update(cx, |menu, cx| {
                        if let Some(submenu) = &mut menu.submenu
                            && submenu.highlighted != Some(ix)
                        {
                            submenu.highlighted = Some(ix);
                            cx.notify();
                        }
                    }))
                    .ok();
                }
            });
        let bounds = submenu.bounds.clone();
        let panel = div()
            .debug_selector(|| "git-branches-actions".into())
            .relative()
            .child(panel)
            .child(
                canvas(move |drawn, _, _| bounds.set(Some(drawn)), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            );
        // At the row's top right corner, over the menu.
        div()
            .absolute()
            .top_0()
            .right_0()
            .child(
                deferred(
                    anchored()
                        .offset(point(px(4.), px(-4.)))
                        .snap_to_window()
                        .child(panel),
                )
                .with_priority(1),
            )
            .into_any_element()
    }
}

/// How far a local branch is from its upstream, as the title bar says it:
/// `↑2 ↓1`; nothing for one level with it, or with none.
fn branch_counts(branch: &Branch) -> Option<String> {
    let upstream = Upstream {
        name: branch.upstream.clone()?,
        ahead: branch.ahead?,
        behind: branch.behind?,
    };
    crate::git::sync_counts(Some(&upstream))
}

/// From item `at` of `count`, the next with `down`, else the one before,
/// wrapping; from none, the first or the last.
fn step(at: Option<usize>, down: bool, count: usize) -> usize {
    let last = count.saturating_sub(1);
    match (at, down) {
        (None, true) => 0,
        (None, false) => last,
        (Some(at), true) if at >= last => 0,
        (Some(at), true) => at + 1,
        (Some(at), false) => at.checked_sub(1).unwrap_or(last),
    }
}

impl Render for BranchMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let rows: Vec<AnyElement> = (0..self.rows.len())
            .map(|ix| self.render_row(ix, window, cx))
            .collect();
        let panel = div()
            .id("git-branches-menu")
            .debug_selector(|| "git-branches-menu".into())
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_input_command))
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down_out(
                cx.listener(|menu, event, window, cx| menu.pressed_outside(event, window, cx)),
            )
            .occlude()
            .w(WIDTH)
            .flex()
            .flex_col()
            .p_1()
            .bg(colors.surface.panel)
            .border_1()
            .border_color(colors.border.default)
            .rounded_md()
            .shadow_lg()
            .text_color(colors.text.primary)
            .child(self.render_search(cx))
            .child(
                div()
                    .id("git-branches-rows")
                    .flex()
                    .flex_col()
                    .max_h(MAX_ROWS_HEIGHT)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows),
            );
        let panel = fe_ui::node(
            panel,
            window,
            cx,
            Role::Menu,
            tr!("workbench-git-branches-menu"),
        );
        anchored()
            .position(self.position)
            .snap_to_window()
            .child(panel)
    }
}

#[cfg(test)]
impl Workbench {
    /// The rows of the branch name's picker, while it shows.
    pub(crate) fn branch_name_rows(&self, cx: &App) -> Option<Vec<String>> {
        let view = self.modals.read(cx).modal_view()?.clone();
        let picker = view
            .downcast::<crate::picker::Picker<super::NamePicker>>()
            .ok()?;
        Some(picker.read(cx).delegate.labels())
    }

    /// The branches menu, while it shows.
    pub(crate) fn branches_menu(&self, cx: &App) -> Option<gpui::Entity<BranchMenu>> {
        let view = self.modals.read(cx).modal_view()?.clone();
        view.downcast::<BranchMenu>().ok()
    }
}

#[cfg(test)]
impl BranchMenu {
    /// Its rows as text: an action's title; `-` for a separator; `# ` and a
    /// section's title; `## ` and a remote's name; a branch's name as shown,
    /// indented, then `*` if it's checked out and its counts; or the line
    /// saying nothing matches.
    pub(crate) fn lines(&self) -> Vec<String> {
        (0..self.rows.len()).map(|ix| self.line(ix)).collect()
    }

    fn line(&self, ix: usize) -> String {
        match &self.rows[ix] {
            Row::Action(action) => self.title(*action).to_string(),
            Row::Separator => "-".to_owned(),
            Row::Title(section) => format!("# {}", section.title()),
            Row::Remote(remote) => format!("## {remote}"),
            Row::NoMatch => tr!("workbench-git-branches-no-match").to_string(),
            Row::Branch(section, branch) => {
                let branch = &self.branches[*branch];
                let name = match (&branch.remote, section) {
                    (Some(remote), Section::Remote) => (branch.name.strip_prefix(remote.as_str()))
                        .and_then(|rest| rest.strip_prefix('/'))
                        .unwrap_or(&branch.name),
                    _ => &branch.name,
                };
                let mut line = format!("  {name}");
                if branch.is_current {
                    line.push_str(" *");
                }
                if let Some(counts) = branch_counts(branch) {
                    line.push(' ');
                    line.push_str(&counts);
                }
                line
            }
        }
    }

    /// What's in the search field.
    pub(crate) fn search_text(&self, cx: &App) -> String {
        self.input.read(cx).text().to_owned()
    }

    /// The highlighted row, as [`Self::lines`] has it.
    pub(crate) fn highlighted_line(&self) -> Option<String> {
        Some(self.line(self.highlighted?))
    }

    /// The open branch's actions' titles, and the highlighted one's.
    pub(crate) fn actions(&self) -> Option<(Vec<String>, Option<String>)> {
        let submenu = self.submenu.as_ref()?;
        let Row::Branch(_, branch) = &self.rows[submenu.row] else {
            return None;
        };
        let branch = &self.branches[*branch];
        let titles: Vec<String> = (submenu.items.iter())
            .map(|&action| self.branch_action_title(action, branch).to_string())
            .collect();
        let highlighted = submenu.highlighted.map(|ix| titles[ix].clone());
        Some((titles, highlighted))
    }

    /// The row a branch named `name` is shown on in `section`'s rows, as
    /// [`Self::lines`] indents it, after the section's title.
    pub(crate) fn row_of(&self, title: &str, name: &str) -> Option<usize> {
        let lines = self.lines();
        let start = lines
            .iter()
            .position(|line| line == &format!("# {title}"))?;
        (lines.iter().enumerate().skip(start + 1))
            .take_while(|(_, line)| !line.starts_with("# "))
            .find(|(_, line)| {
                let line = line.trim_start();
                line == name || line.starts_with(&format!("{name} "))
            })
            .map(|(ix, _)| ix)
    }
}
