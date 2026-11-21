//! Pre-commit checks above the Changes panel's commit message
//! (`docs/design/design-system.md`, "Changes and commit": "Above the
//! message: live pre-commit checks from Runs"): the project tasks
//! `git.pre_commit_checks` lists, by label, run each time the staged set
//! changes.
//!
//! The staged set is every file staged in the repository, with how it's
//! staged, as the panel last listed it ([`GitPanel`]), and what's staged of
//! each: its mode and blob in the index, as git lists them
//! ([`staged_content`]), so a file staged again with new text, from the
//! panel, a command or a terminal, changes it too, though it lists the
//! same. It's read again each time the panel lists the status, which it
//! does once the index changes. Once it changes, the round running is
//! cancelled, its run
//! cancelled and its program stopped, and the settings (the user's file and
//! the root's project file, as they are then) and the root's tasks
//! ([`crate::task_defs`]) are read, each listed check pending. Once the set
//! has held still for [`SETTLE`], a new round, in a trusted root only, runs
//! each listed task, one at a time in the list's order, as a
//! [`RunKind::Script`] run in the workbench's [`Runs`] through the
//! background runner ([`RunProcess`]). A label no task has is unavailable and skipped, not an
//! error; a task whose own folder the runner refuses (one not trusted
//! inside the trusted root) is skipped as not trusted, and the next runs.
//! A round ends with its last check; a change to the set before
//! then, a settings file saved, or the root's trust changing, starts over.
//!
//! Each check is pending, running, passed, failed or cancelled (its run
//! stopped from elsewhere), as its run ended. With nothing staged nothing
//! runs and each row says so; in a root that isn't trusted nothing runs
//! either, and each row says that. A check failing doesn't disable the
//! commit button: the checks inform, they don't block (hooks still run on
//! commit).
//!
//! The panel draws one row per check above the message box
//! ([`PreCommitChecks::render`]): its status icon, its label in mono, its
//! status in words, and, once it has a run, a link that shows the Runs
//! panel with the run selected.
//!
//! Its text is the catalogue's (`l10n/en-US/changes.ftl`).

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use fe_git::ChangeKind;
use fe_ui::{ActiveTheme as _, FamilySlot, FontFamilies, Icon};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Task, WeakEntity, Window, div, px, svg,
};
use kiln_l10n::tr;

use crate::{
    GitPanel, TrustChanged, Workbench,
    dock::DockOp,
    git::GitState,
    run_process::{LineSink, RunProcess, RunSpec, RunUpdate, Stream},
    runs::{RunId, RunKind, RunStatus, Runs, RunsEvent},
    task_commands::Files,
    task_defs::{self, Task as ProjectTask},
    terminal_panel::task_options,
};

/// How long the staged set must hold still before a round starts.
pub(crate) const SETTLE: Duration = Duration::from_secs(2);

/// Where a check is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckStatus {
    /// Nothing is staged, so nothing runs.
    NothingStaged,
    /// The root isn't trusted, so nothing runs.
    Untrusted,
    /// No task has its label.
    Unavailable,
    /// Waiting for the set to hold still, or for the checks before it.
    Pending,
    Running,
    Passed,
    Failed,
    /// Its run was stopped from elsewhere.
    Cancelled,
}

impl CheckStatus {
    /// What its row says of it.
    pub(crate) fn text(self) -> SharedString {
        match self {
            Self::NothingStaged => tr!("workbench-changes-check-nothing-staged"),
            Self::Untrusted => tr!("workbench-changes-check-untrusted"),
            Self::Unavailable => tr!("workbench-changes-check-unavailable"),
            Self::Pending => tr!("workbench-changes-check-pending"),
            Self::Running => tr!("workbench-changes-check-running"),
            Self::Passed => tr!("workbench-changes-check-passed"),
            Self::Failed => tr!("workbench-changes-check-failed"),
            Self::Cancelled => tr!("workbench-changes-check-cancelled"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Running => Icon::LoaderCircle,
            Self::Passed => Icon::Check,
            Self::Failed => Icon::X,
            Self::Pending => Icon::CircleDot,
            Self::NothingStaged | Self::Untrusted | Self::Unavailable | Self::Cancelled => {
                Icon::Minus
            }
        }
    }
}

/// One listed check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Check {
    /// The task's label, as the setting lists it.
    pub(crate) label: String,
    pub(crate) status: CheckStatus,
    /// Its run in this round, once it has one.
    pub(crate) run: Option<RunId>,
}

/// Every staged file in the repository, with how it's staged.
pub(crate) type StagedSet = Vec<(PathBuf, ChangeKind)>;

/// The staged set as a round runs on it: its files, and what's staged of
/// them ([`staged_content`]), `None` if git couldn't say.
#[derive(Debug, PartialEq, Eq)]
struct Staged {
    files: StagedSet,
    content: Option<Vec<u8>>,
}

/// The check running.
struct Current {
    /// Its index in the checks.
    at: usize,
    /// Its run, once it's started.
    run: Option<RunId>,
    /// Its program, which dropping stops.
    _process: Option<Entity<RunProcess>>,
}

/// What a round reads before it runs: the labels listed and the root's
/// tasks.
struct Read {
    labels: Vec<String>,
    tasks: Vec<ProjectTask>,
}

/// What the tests run checks through: each check begins a run in
/// [`Runs`] and is recorded, its label and run, for the test to finish.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct FakeCheckRunner(pub(crate) std::rc::Rc<std::cell::RefCell<Vec<(String, RunId)>>>);

#[cfg(test)]
impl gpui::Global for FakeCheckRunner {}

/// A check's output isn't read: its run's exit says how it went.
struct Ignore;

impl LineSink for Ignore {
    fn line(&mut self, _: Stream, _: &str, _: Duration) -> Vec<RunUpdate> {
        Vec::new()
    }
}

/// The Changes panel's pre-commit checks: see the module's docs.
pub(crate) struct PreCommitChecks {
    root: Option<PathBuf>,
    checks: Vec<Check>,
    /// The tasks each check runs, as this round resolved them.
    tasks: Vec<Option<ProjectTask>>,
    /// The staged set, once it's been listed and read.
    staged: Option<Staged>,
    /// Reading what's staged of the set last listed: dropped, it does
    /// nothing more.
    identifying: Option<Task<()>>,
    /// Counts rounds, so a round's late work is dropped once another has
    /// started.
    round: u64,
    /// The wait for the set to hold still, the read before a round, or a
    /// check starting: dropped, it does nothing more.
    work: Option<Task<()>>,
    current: Option<Current>,
    /// What the panel reads the status through, which knows the workbench.
    git: WeakEntity<GitState>,
    workbench: Option<WeakEntity<Workbench>>,
    /// Following the workbench's trust, once it's known.
    trust_followed: Option<Subscription>,
    /// Following the settings files, from the first listing on.
    settings_followed: Option<Subscription>,
    /// The workbench's runs, and following them, once a check has started.
    runs: Option<Entity<Runs>>,
    _runs: Option<Subscription>,
    /// The rows last drawn: each label and status as they read.
    #[cfg(test)]
    drawn: std::cell::RefCell<Vec<(SharedString, SharedString)>>,
}

impl PreCommitChecks {
    pub(crate) fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            checks: Vec::new(),
            tasks: Vec::new(),
            staged: None,
            identifying: None,
            round: 0,
            work: None,
            current: None,
            git: WeakEntity::new_invalid(),
            workbench: None,
            trust_followed: None,
            settings_followed: None,
            runs: None,
            _runs: None,
            #[cfg(test)]
            drawn: Default::default(),
        }
    }

    /// The checks, in order.
    #[cfg(test)]
    pub(crate) fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// The panel listed `files` as staged, its status read through `git`:
    /// reads what's staged of them, off the main thread, then starts over
    /// if the set differs from the last, or if it's the first. A listing
    /// before that read lands replaces it. It may be listed as `git`
    /// updates, so nothing here reads it.
    pub(crate) fn staged(
        &mut self,
        files: StagedSet,
        git: WeakEntity<GitState>,
        cx: &mut Context<GitPanel>,
    ) {
        self.git = git;
        self.follow_settings(cx);
        let reading = identify(self.root.clone(), files, cx);
        self.identifying = Some(cx.spawn(async move |panel, cx| {
            let staged = reading.await;
            let read = panel.update(cx, |panel, cx| {
                let checks = &mut panel.pre_commit;
                if checks.staged.as_ref() != Some(&staged) {
                    checks.staged = Some(staged);
                    checks.restart(cx);
                }
            });
            read.ok();
        }));
    }

    /// Starts over, once a staged set has been listed: a settings file
    /// saved, or the root's trust changed.
    fn restart_if_listed(&mut self, cx: &mut Context<GitPanel>) {
        if self.staged.is_some() {
            self.restart(cx);
        }
    }

    /// Follows the settings files, from the first listing on.
    fn follow_settings(&mut self, cx: &mut Context<GitPanel>) {
        if self.settings_followed.is_some() {
            return;
        }
        let followed = cx.observe_global::<kiln::platform::config::ChangedFiles>(|panel, cx| {
            let checks = &mut panel.pre_commit;
            let Some(root) = checks.root.clone() else {
                return;
            };
            let user = crate::open::settings_path(cx);
            let project = fe_config::project_settings_path(&root);
            let changed = kiln::platform::config::changed_files(cx);
            let ours = |file: &PathBuf| *file == project || user.as_ref() == Some(file);
            if changed.iter().any(ours) {
                checks.restart_if_listed(cx);
            }
        });
        self.settings_followed = Some(followed);
    }

    /// The workbench, following its trust from the first time it's asked
    /// for. It reads the git state, so it's only asked once nothing's
    /// updating that: from the round's own tasks, a run's change or a
    /// click.
    fn workbench(&mut self, cx: &mut Context<GitPanel>) -> Option<Entity<Workbench>> {
        if let Some(workbench) = self.workbench.as_ref().and_then(WeakEntity::upgrade) {
            return Some(workbench);
        }
        let workbench = self.git.upgrade()?.read(cx).workbench().upgrade()?;
        self.workbench = Some(workbench.downgrade());
        self.trust_followed = Some(cx.subscribe(&workbench, |panel, _, _: &TrustChanged, cx| {
            panel.pre_commit.restart_if_listed(cx)
        }));
        Some(workbench)
    }

    /// Cancels the round running, and waits for the set to hold still
    /// before the next, or, with nothing staged, says so.
    fn restart(&mut self, cx: &mut Context<GitPanel>) {
        self.round += 1;
        self.work = None;
        self.cancel(cx);
        cx.notify();
        let Some(root) = self.root.clone() else {
            return;
        };
        let round = self.round;
        let nothing_staged = (self.staged.as_ref()).is_none_or(|staged| staged.files.is_empty());
        for check in &mut self.checks {
            check.status = match nothing_staged {
                true => CheckStatus::NothingStaged,
                false => CheckStatus::Pending,
            };
            check.run = None;
        }
        let reading = read(&root, cx);
        let executor = cx.background_executor().clone();
        self.work = Some(cx.spawn(async move |panel, cx| {
            let read = reading.await;
            let status = match nothing_staged {
                true => CheckStatus::NothingStaged,
                false => CheckStatus::Pending,
            };
            let listed = panel.update(cx, |panel, cx| {
                let checks = &mut panel.pre_commit;
                if checks.round != round {
                    return false;
                }
                // Following its trust from now on.
                checks.workbench(cx);
                checks.list(&read, status);
                cx.notify();
                !checks.checks.is_empty()
            });
            if nothing_staged || !listed.unwrap_or(false) {
                return;
            }
            executor.timer(SETTLE).await;
            let begun = panel.update(cx, |panel, cx| {
                let checks = &mut panel.pre_commit;
                if checks.round == round {
                    checks.begin(&read, cx);
                }
            });
            begun.ok();
        }));
    }

    /// Lists `read`'s labels as checks, each `status`, but those no task
    /// has unavailable where `status` is pending.
    fn list(&mut self, read: &Read, status: CheckStatus) {
        self.tasks = (read.labels.iter())
            .map(|label| read.tasks.iter().find(|task| task.label == *label).cloned())
            .collect();
        self.checks = (read.labels.iter().zip(&self.tasks))
            .map(|(label, task)| Check {
                label: label.clone(),
                status: match (status, task) {
                    (CheckStatus::Pending, None) => CheckStatus::Unavailable,
                    _ => status,
                },
                run: None,
            })
            .collect();
    }

    /// Starts a round of `read`'s checks, if the root's trusted.
    fn begin(&mut self, read: &Read, cx: &mut Context<GitPanel>) {
        let trusted =
            (self.workbench(cx)).is_some_and(|workbench| workbench.read(cx).root_trusted());
        if !trusted {
            self.list(read, CheckStatus::Untrusted);
            return cx.notify();
        }
        self.list(read, CheckStatus::Pending);
        self.next(0, cx);
    }

    /// Starts the first pending check from `from` on; with none, the round
    /// is over.
    fn next(&mut self, from: usize, cx: &mut Context<GitPanel>) {
        let at = (self.checks.iter().enumerate().skip(from))
            .find(|(_, check)| check.status == CheckStatus::Pending)
            .map(|(at, _)| at);
        match at {
            Some(at) => self.start(at, cx),
            None => self.current = None,
        }
        cx.notify();
    }

    /// Starts check `at`'s task as a run.
    fn start(&mut self, at: usize, cx: &mut Context<GitPanel>) {
        let (Some(runs), Some(task)) = (self.runs(cx), self.tasks.get(at).cloned().flatten())
        else {
            self.current = None;
            return;
        };
        self.checks[at].status = CheckStatus::Running;
        #[cfg(test)]
        if let Some(fake) = cx.try_global::<FakeCheckRunner>().cloned() {
            let label = task.label.clone();
            let run = runs.update(cx, |runs, cx| {
                runs.begin(RunKind::Script, label.clone(), cx)
            });
            fake.0.borrow_mut().push((label, run));
            self.checks[at].run = Some(run);
            self.current = Some(Current {
                at,
                run: Some(run),
                _process: None,
            });
            return;
        }
        let spec = match spec(&task) {
            Some(spec) => spec,
            None => {
                self.checks[at].status = CheckStatus::Failed;
                return self.next(at + 1, cx);
            }
        };
        self.current = Some(Current {
            at,
            run: None,
            _process: None,
        });
        let starting = RunProcess::start(spec, Ignore, runs, cx);
        let round = self.round;
        self.work = Some(cx.spawn(async move |panel, cx| {
            let started = starting.await;
            let kept = panel.update(cx, |panel, cx| {
                let checks = &mut panel.pre_commit;
                // A newer round drops the process, which stops it.
                if checks.round != round {
                    return;
                }
                match started {
                    Ok(process) => {
                        let run = process.read(cx).run();
                        checks.checks[at].run = Some(run);
                        checks.current = Some(Current {
                            at,
                            run: Some(run),
                            _process: Some(process),
                        });
                        // It may have ended already.
                        if let Some(runs) = checks.runs(cx) {
                            checks.run_changed(run, &runs, cx);
                        }
                    }
                    // Its own folder isn't trusted: only it's skipped. Were
                    // the root's trust lost, the round would start over.
                    Err(_) => {
                        checks.current = None;
                        checks.checks[at].status = CheckStatus::Untrusted;
                        checks.next(at + 1, cx);
                    }
                }
                cx.notify();
            });
            kept.ok();
        }));
    }

    /// Run `id` changed: if it's the running check's and it's ended, the
    /// check has too, and the next starts.
    fn run_changed(&mut self, id: RunId, runs: &Entity<Runs>, cx: &mut Context<GitPanel>) {
        let Some(current) = &self.current else {
            return;
        };
        if current.run != Some(id) {
            return;
        }
        let status = match runs.read(cx).get(id).map(|run| run.status) {
            Some(RunStatus::Running { .. }) => return,
            Some(RunStatus::Passed) => CheckStatus::Passed,
            Some(RunStatus::Failed { .. }) => CheckStatus::Failed,
            // Gone from the list, it can't be shown either way.
            Some(RunStatus::Cancelled) | None => CheckStatus::Cancelled,
        };
        let at = current.at;
        self.current = None;
        self.checks[at].status = status;
        self.next(at + 1, cx);
    }

    /// Stops the running check's run, if any.
    fn cancel(&mut self, cx: &mut Context<GitPanel>) {
        let Some(current) = self.current.take() else {
            return;
        };
        let Some(run) = current.run else {
            return;
        };
        if let Some(process) = &current._process {
            process.update(cx, |process, cx| process.cancel(cx));
        } else if let Some(runs) = &self.runs {
            let cancelled = crate::runs::RunEnd::Cancelled;
            runs.update(cx, |runs, cx| runs.finish(run, cancelled, None, cx));
        }
    }

    /// The workbench's runs, followed from the first check on. It reads
    /// the workbench, so it's only asked once nothing's updating it: as a
    /// round begins, or a check's run changes or starts.
    fn runs(&mut self, cx: &mut Context<GitPanel>) -> Option<Entity<Runs>> {
        if self.runs.is_none() {
            let workbench = self.workbench(cx)?;
            let runs = workbench.read(cx).runs().clone();
            self._runs = Some(cx.subscribe(&runs, |panel, runs, event: &RunsEvent, cx| {
                let RunsEvent::Changed(id) = *event;
                panel.pre_commit.run_changed(id, &runs, cx);
            }));
            self.runs = Some(runs);
        }
        self.runs.clone()
    }

    /// One row per check, above the message box; none with none listed.
    pub(crate) fn render(&self, cx: &mut Context<GitPanel>) -> Option<AnyElement> {
        if self.checks.is_empty() {
            return None;
        }
        #[cfg(test)]
        self.drawn.borrow_mut().clear();
        let ui = fe_ui::ui_theme(cx);
        let colors = cx.theme().colors;
        let mono = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let rows = self.checks.iter().enumerate().map(|(ix, check)| {
            let status = check.status.text();
            #[cfg(test)]
            (self.drawn.borrow_mut()).push((check.label.clone().into(), status.clone()));
            let colour = match check.status {
                CheckStatus::Passed => colors.status.success,
                CheckStatus::Failed => colors.status.error,
                CheckStatus::Running => colors.text.primary,
                _ => colors.text.muted,
            };
            let link = check.run.map(|run| {
                div()
                    .id(("pre-commit-check-run", ix))
                    .debug_selector(move || check_selector("run", ix))
                    .flex_none()
                    .text_color(ui.accent)
                    .cursor_pointer()
                    .hover(|link| link.underline())
                    .child(tr!("workbench-changes-check-show-run"))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        cx.stop_propagation();
                        panel.show_check_run(run, window, cx);
                    }))
            });
            div()
                .debug_selector(move || check_selector("row", ix))
                .flex()
                .items_center()
                .gap_2()
                .h(px(20.))
                .child(
                    svg()
                        .flex_none()
                        .size(px(14.))
                        .path(check.status.icon().path())
                        .text_color(colour),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(mono.clone())
                        .child(check.label.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(ui.muted_text)
                        .child(status),
                )
                .children(link)
        });
        let element = div()
            .debug_selector(|| "pre-commit-checks".into())
            .flex()
            .flex_col()
            .children(rows);
        Some(element.into_any_element())
    }

    /// The rows last drawn: each check's label and what its status read.
    #[cfg(test)]
    pub(crate) fn drawn(&self) -> Vec<(SharedString, SharedString)> {
        self.drawn.borrow().clone()
    }
}

impl GitPanel {
    /// Shows the Runs panel with `run` selected: a check's link.
    fn show_check_run(&mut self, run: RunId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workbench) = self.pre_commit.workbench(cx) else {
            return;
        };
        window.defer(cx, move |window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.run_dock_op(DockOp::ShowRuns, window, cx);
                workbench.select_run(run, cx);
            });
        });
    }
}

/// The test selectors' prefix: a check's row, and its link, with the row's
/// index after a colon.
const CHECK_SELECTOR: &str = "pre-commit-check";

/// Check row `ix`'s part's test selector, `what` its part (`row`, `run`).
pub(crate) fn check_selector(what: &str, ix: usize) -> String {
    format!("{CHECK_SELECTOR}-{what}:{ix}")
}

/// Reads, off the main thread, what's staged of `files` in `root`'s
/// repository: nothing with none, or no root.
fn identify(root: Option<PathBuf>, files: StagedSet, cx: &App) -> Task<Staged> {
    cx.background_spawn(async move {
        let content = match root {
            Some(root) if !files.is_empty() => staged_content(&root),
            _ => None,
        };
        Staged { files, content }
    })
}

/// What's staged in `root`'s repository, as git lists it: each entry the
/// index has apart from HEAD, with its modes and blob ids before and after
/// (`git diff-index --cached`), or with no HEAD yet the whole index
/// (`git ls-files --stage`). None if git can't say. Blocks until git
/// exits. It reads the index and objects only: no lock taken, no
/// filesystem monitor run, whatever the repository's config says.
fn staged_content(root: &Path) -> Option<Vec<u8>> {
    let read = |args: &[&str]| {
        let output = crate::process_group::no_window(&mut Command::new("git"))
            .current_dir(root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["-c", "core.fsmonitor=false"])
            .args(args)
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    };
    read(&["diff-index", "--cached", "--no-abbrev", "-z", "HEAD", "--"])
        .or_else(|| read(&["ls-files", "--stage", "-z"]))
}

/// Reads, off the main thread, the labels the settings files list now and
/// `root`'s tasks.
fn read(root: &Path, cx: &App) -> Task<Read> {
    let user = crate::open::settings_path(cx);
    let project = fe_config::project_settings_path(root);
    let files = files(cx);
    let root = root.to_owned();
    cx.background_spawn(async move {
        let store = fe_config::SettingsStore::load(user, Some(project));
        let labels = store.settings().git.pre_commit_checks.clone();
        let tasks = match files {
            Some(files) if !labels.is_empty() => task_defs::load(&*files, &root).0,
            _ => Vec::new(),
        };
        Read { labels, tasks }
    })
}

/// What the project's tasks are read through: under test, the fake files
/// a test gives, and none (so no tasks) where it gives none.
fn files(cx: &App) -> Option<Files> {
    #[cfg(test)]
    return (cx.try_global::<crate::task_commands::TestFiles>()).map(|files| files.0.clone());
    #[cfg(not(test))]
    Some(crate::task_commands::project_files(cx))
}

/// What runs `task` as a check: its command as a task tab would run it,
/// through `/bin/sh` for a shell line, with stdout and stderr as one. None
/// if it can't run: its folder isn't one, or it names no program.
fn spec(task: &ProjectTask) -> Option<RunSpec> {
    let options = task_options(task, 0).ok()?;
    let (program, args) = options.shell?;
    let os = |text: &String| OsString::from(text);
    Some(RunSpec {
        kind: RunKind::Script,
        title: task.label.clone(),
        program: program.into(),
        args: args.iter().map(os).collect(),
        cwd: task.cwd.clone(),
        env: (task.env.iter()).map(|(k, v)| (os(k), os(v))).collect(),
        merged: true,
    })
}
