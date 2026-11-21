//! Pre-commit checks above the commit message ([`crate::pre_commit_checks`]):
//! the tasks `git.pre_commit_checks` lists, run one at a time as runs once
//! the staged set has held still for 2 s, on repositories built with the
//! `git` CLI in temporary folders. The checks run through a fake runner,
//! whose runs the tests end, on the test clock, but for one through the
//! real runner.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::{
    GitPanel, Workbench,
    git_panel::set_box_text,
    pre_commit_checks::{CheckStatus, FakeCheckRunner, SETTLE, check_selector},
    runs::{RunEnd, RunId, RunKind, RunStatus},
    task_commands::TestFiles,
    task_defs::ProjectFiles,
    tests::open_root,
};

/// The disk, as far as the project's folder.
struct Disk(PathBuf);

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.starts_with(&self.0) && path.is_file()
    }
}

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The project's tasks: `fmt`, `lint` and `test`, none of which the fake
/// runner runs.
const TASKS: &str = "\
[[task]]
label = \"fmt\"
command = \"true\"

[[task]]
label = \"lint\"
command = \"true\"

[[task]]
label = \"test\"
command = \"true\"
";

/// A repository with `a.txt`, `b.txt` and `c.txt` committed, then all three
/// changed and none staged, whose project settings list `checks`. The
/// project's own files are excluded from git.
fn fixture(checks: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
    ] {
        git(root, &["config", key, value]);
    }
    fs::write(root.join(".git/info/exclude"), ".tenmoku/\n").unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.join(name), "head\n").unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.join(name), "changed\n").unwrap();
    }
    let settings = fe_config::project_settings_path(root);
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let listed: Vec<String> = checks.iter().map(|check| format!("{check:?}")).collect();
    let listed = listed.join(", ");
    fs::write(settings, format!("[git]\npre_commit_checks = [{listed}]\n")).unwrap();
    fs::write(root.join(crate::task_defs::TASKS_FILE), TASKS).unwrap();
    dir
}

struct Project<'a> {
    workbench: Entity<Workbench>,
    panel: Entity<GitPanel>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
    fake: FakeCheckRunner,
}

/// A workbench on [`fixture`]`(checks)`, showing the Changes panel, its
/// root trusted if `trusted`.
fn open<'a>(cx: &'a mut TestAppContext, checks: &[&str], trusted: bool) -> Project<'a> {
    let dir = fixture(checks);
    let fake = FakeCheckRunner::default();
    cx.update(|cx| {
        cx.set_global(TestFiles(Arc::new(Disk(dir.path().to_owned()))));
        cx.set_global(fake.clone());
    });
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    if trusted {
        workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    }
    draw(cx);
    let run = RunCommand(
        Invocation::of("git.show_changes")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    let mut project = Project {
        workbench,
        panel,
        cx,
        dir,
        fake,
    };
    // Whatever the first listing waits for is over.
    project.wait(SETTLE * 2);
    project
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

impl Project<'_> {
    /// Stages `name` and has the panel read the status again.
    fn stage(&mut self, name: &str) {
        git(self.dir.path(), &["add", name]);
        self.refresh();
    }

    /// Unstages `name` and has the panel read the status again.
    fn unstage(&mut self, name: &str) {
        git(self.dir.path(), &["reset", "-q", "--", name]);
        self.refresh();
    }

    fn refresh(&mut self) {
        (self.panel).update(self.cx, |panel, cx| panel.refresh(cx));
        draw(self.cx);
    }

    /// Moves the test clock on by `by`.
    fn wait(&mut self, by: Duration) {
        self.cx.executor().advance_clock(by);
        draw(self.cx);
    }

    /// The labels the fake runner has started, in order.
    fn started(&self) -> Vec<String> {
        let started = self.fake.0.borrow();
        started.iter().map(|(label, _)| label.clone()).collect()
    }

    /// The run of the `nth` check the fake runner started.
    fn run(&self, nth: usize) -> RunId {
        self.fake.0.borrow()[nth].1
    }

    /// Ends the `nth` check started as `end` says.
    fn end(&mut self, nth: usize, end: RunEnd) {
        let run = self.run(nth);
        let runs = self.workbench.read_with(self.cx, |w, _| w.runs().clone());
        runs.update(self.cx, |runs, cx| runs.finish(run, end, None, cx));
        draw(self.cx);
    }

    /// Run `run`'s status.
    fn run_status(&mut self, run: RunId) -> RunStatus {
        let runs = self.workbench.read_with(self.cx, |w, _| w.runs().clone());
        runs.read_with(self.cx, |runs, _| runs.get(run).expect("kept").status)
    }

    /// Each check's label and status.
    fn checks(&mut self) -> Vec<(String, CheckStatus)> {
        self.panel.read_with(self.cx, |panel, _| {
            (panel.pre_commit.checks().iter())
                .map(|check| (check.label.clone(), check.status))
                .collect()
        })
    }

    /// The rows last drawn: each label and its status as it reads.
    fn drawn(&mut self) -> Vec<(String, String)> {
        self.panel.read_with(self.cx, |panel, _| {
            (panel.pre_commit.drawn().into_iter())
                .map(|(label, status)| (label.to_string(), status.to_string()))
                .collect()
        })
    }
}

fn row(label: &str, status: CheckStatus) -> (String, CheckStatus) {
    (label.to_owned(), status)
}

#[gpui::test]
fn listed_tasks_run_in_order_after_the_staged_set_changes_and_holds_still_for_2s(
    cx: &mut TestAppContext,
) {
    let mut project = open(cx, &["fmt", "lint", "test"], true);
    assert_eq!(project.started(), Vec::<String>::new());
    project.stage("a.txt");
    use CheckStatus::*;
    assert_eq!(
        project.checks(),
        [
            row("fmt", Pending),
            row("lint", Pending),
            row("test", Pending)
        ]
    );
    project.wait(Duration::from_secs(1));
    // The set changes again before it has held still for 2 s.
    project.stage("b.txt");
    project.wait(Duration::from_millis(1500));
    assert_eq!(
        project.started(),
        Vec::<String>::new(),
        "2 s after a, 1.5 after b"
    );
    project.wait(Duration::from_millis(600));
    assert_eq!(project.started(), ["fmt"], "one at a time");
    assert_eq!(
        project.checks(),
        [
            row("fmt", Running),
            row("lint", Pending),
            row("test", Pending)
        ]
    );
    // Each a script run, titled by its label.
    let runs = project
        .workbench
        .read_with(project.cx, |w, _| w.runs().clone());
    let fmt = project.run(0);
    let (kind, title) = runs.read_with(project.cx, |runs, _| {
        let run = runs.get(fmt).unwrap();
        (run.kind, run.title.clone())
    });
    assert_eq!((kind, title.as_str()), (RunKind::Script, "fmt"));

    project.end(0, RunEnd::Passed);
    assert_eq!(project.started(), ["fmt", "lint"]);
    project.end(1, RunEnd::Passed);
    assert_eq!(project.started(), ["fmt", "lint", "test"]);
    project.end(2, RunEnd::Passed);
    assert_eq!(
        project.checks(),
        [row("fmt", Passed), row("lint", Passed), row("test", Passed)]
    );
    let passed = tr!("workbench-changes-check-passed").to_string();
    let drawn = project.drawn();
    let labels: Vec<&str> = drawn.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(labels, ["fmt", "lint", "test"]);
    assert!(
        drawn.iter().all(|(_, status)| *status == passed),
        "{drawn:?}"
    );

    // A check's link shows its run in the Runs panel.
    let link = check_selector("run", 0);
    let bounds = project.cx.debug_bounds(link.leak()).expect("drawn");
    (project.cx).simulate_click(bounds.center(), Modifiers::none());
    draw(project.cx);
    let selected = project.workbench.read_with(project.cx, |workbench, cx| {
        workbench.runs_panel().read(cx).selected_run(cx)
    });
    assert_eq!(selected, Some(fmt));

    // The round is over: with the set unchanged, nothing more runs.
    project.refresh();
    project.wait(SETTLE * 2);
    assert_eq!(project.started().len(), 3);
}

#[gpui::test]
fn a_new_change_cancels_the_previous_round(cx: &mut TestAppContext) {
    let mut project = open(cx, &["fmt", "lint"], true);
    project.stage("a.txt");
    project.wait(SETTLE + Duration::from_millis(100));
    assert_eq!(project.started(), ["fmt"]);
    let first = project.run(0);

    project.stage("b.txt");
    assert_eq!(project.run_status(first), RunStatus::Cancelled);
    use CheckStatus::*;
    assert_eq!(
        project.checks(),
        [row("fmt", Pending), row("lint", Pending)]
    );
    project.wait(Duration::from_millis(1900));
    assert_eq!(project.started(), ["fmt"], "the new round waits its 2 s");
    project.wait(Duration::from_millis(200));
    assert_eq!(project.started(), ["fmt", "fmt"]);
    assert_ne!(project.run(1), first, "a run of its own");
    // The cancelled run's end, heard late, isn't the new round's.
    assert_eq!(
        project.checks(),
        [row("fmt", Running), row("lint", Pending)]
    );
    project.end(1, RunEnd::Passed);
    assert_eq!(project.started(), ["fmt", "fmt", "lint"]);
}

#[gpui::test]
fn a_missing_label_shows_as_unavailable(cx: &mut TestAppContext) {
    let mut project = open(cx, &["fmt", "nope", "test"], true);
    project.stage("a.txt");
    use CheckStatus::*;
    assert_eq!(
        project.checks(),
        [
            row("fmt", Pending),
            row("nope", Unavailable),
            row("test", Pending)
        ]
    );
    project.wait(SETTLE + Duration::from_millis(100));
    project.end(0, RunEnd::Passed);
    assert_eq!(project.started(), ["fmt", "test"], "skipped, not an error");
    project.end(1, RunEnd::Passed);
    assert_eq!(
        project.checks(),
        [
            row("fmt", Passed),
            row("nope", Unavailable),
            row("test", Passed)
        ]
    );
    let unavailable = tr!("workbench-changes-check-unavailable").to_string();
    assert_eq!(project.drawn()[1], ("nope".to_owned(), unavailable));
}

#[gpui::test]
fn nothing_staged_runs_nothing_and_says_so(cx: &mut TestAppContext) {
    let mut project = open(cx, &["fmt", "lint"], true);
    let says = tr!("workbench-changes-check-nothing-staged").to_string();
    let nothing = [("fmt".to_owned(), says.clone()), ("lint".to_owned(), says)];
    assert_eq!(project.drawn(), nothing);
    project.wait(SETTLE * 2);
    assert_eq!(project.started(), Vec::<String>::new());

    // Staged and unstaged again before a round starts: still nothing.
    project.stage("a.txt");
    project.wait(Duration::from_secs(1));
    project.unstage("a.txt");
    project.wait(SETTLE * 2);
    assert_eq!(project.started(), Vec::<String>::new());
    assert_eq!(project.drawn(), nothing);
    let runs = project
        .workbench
        .read_with(project.cx, |w, _| w.runs().clone());
    assert_eq!(runs.read_with(project.cx, |runs, _| runs.list().count()), 0);
}

#[gpui::test]
fn a_failing_check_does_not_disable_commit(cx: &mut TestAppContext) {
    let mut project = open(cx, &["fmt", "lint"], true);
    let commit_box = project.panel.read_with(project.cx, |panel, _| {
        panel
            .commit_box()
            .cloned()
            .expect("the panel's drawn with its box")
    });
    project
        .cx
        .update(|_, cx| set_box_text(&commit_box, "Change a", cx));
    project.stage("a.txt");
    project.wait(SETTLE + Duration::from_millis(100));
    project.end(0, RunEnd::Failed { code: Some(1) });
    use CheckStatus::*;
    assert_eq!(project.checks(), [row("fmt", Failed), row("lint", Running)]);
    let failed = tr!("workbench-changes-check-failed").to_string();
    assert_eq!(project.drawn()[0], ("fmt".to_owned(), failed));
    let button = project.panel.read_with(project.cx, |panel, cx| {
        panel.commit_button(cx).expect("the box's button")
    });
    assert!(button.enabled, "a failed check doesn't block the commit");
}

#[gpui::test]
fn an_untrusted_root_runs_nothing(cx: &mut TestAppContext) {
    let mut project = open(cx, &["fmt", "lint"], false);
    project.stage("a.txt");
    project.wait(SETTLE * 2);
    assert_eq!(project.started(), Vec::<String>::new());
    use CheckStatus::*;
    assert_eq!(
        project.checks(),
        [row("fmt", Untrusted), row("lint", Untrusted)]
    );
    let says = tr!("workbench-changes-check-untrusted").to_string();
    assert_eq!(project.drawn()[0], ("fmt".to_owned(), says));
    let runs = project
        .workbench
        .read_with(project.cx, |w, _| w.runs().clone());
    assert_eq!(runs.read_with(project.cx, |runs, _| runs.list().count()), 0);

    // Trusted, the checks run once the set has held still.
    (project.workbench).update(project.cx, |workbench, cx| {
        workbench.set_root_trust(true, cx)
    });
    project.wait(SETTLE + Duration::from_millis(100));
    assert_eq!(project.started(), ["fmt"]);
}

#[gpui::test]
fn restaging_new_text_under_the_same_status_starts_over(cx: &mut TestAppContext) {
    let mut project = open(cx, &["fmt", "lint"], true);
    project.stage("a.txt");
    project.wait(SETTLE + Duration::from_millis(100));
    project.end(0, RunEnd::Passed);
    project.end(1, RunEnd::Passed);
    use CheckStatus::*;
    assert_eq!(project.checks(), [row("fmt", Passed), row("lint", Passed)]);

    // From a terminal: a.txt is still the one file staged, as Modified.
    let a = project.dir.path().join("a.txt");
    fs::write(&a, "changed again\n").unwrap();
    project.stage("a.txt");
    assert_eq!(
        project.checks(),
        [row("fmt", Pending), row("lint", Pending)],
        "not still passed"
    );
    project.wait(SETTLE + Duration::from_millis(100));
    assert_eq!(project.started(), ["fmt", "lint", "fmt"]);
    let running = project.run(2);

    // Mid-round, through the workbench's own staging, which writes the
    // index off the main thread: the round running is cancelled.
    fs::write(&a, "and again\n").unwrap();
    (project.workbench).update(project.cx, |workbench, cx| {
        workbench.stage_files(vec![a.clone()], true, cx)
    });
    // A read asked for before the staging lands, and the one after.
    project.refresh();
    assert_eq!(project.run_status(running), RunStatus::Cancelled);
    assert_eq!(
        project.checks(),
        [row("fmt", Pending), row("lint", Pending)]
    );
    project.wait(SETTLE + Duration::from_millis(100));
    assert_eq!(project.started(), ["fmt", "lint", "fmt", "fmt"]);

    // Read again with nothing new staged: the round goes on.
    project.refresh();
    project.wait(SETTLE * 2);
    assert_eq!(project.started().len(), 4);
    assert_eq!(project.checks()[0], row("fmt", Running));
}

#[gpui::test]
fn a_task_in_a_folder_not_trusted_is_skipped_and_the_next_runs(cx: &mut TestAppContext) {
    let mut project = open(cx, &["inside", "fmt"], true);
    // The real runner, which checks each task's own folder, and whose
    // threads the app hears from.
    project
        .cx
        .update(|_, cx| cx.remove_global::<FakeCheckRunner>());
    project.cx.executor().allow_parking();
    let root = project.dir.path();
    fs::create_dir(root.join("sub")).unwrap();
    let tasks =
        format!("{TASKS}\n[[task]]\nlabel = \"inside\"\ncommand = \"true\"\ncwd = \"sub\"\n");
    fs::write(root.join(crate::task_defs::TASKS_FILE), tasks).unwrap();
    let sub = dunce::canonicalize(root.join("sub")).unwrap();
    project
        .cx
        .update(|_, cx| kiln_shell::trust::decide(&sub, false, cx));

    project.stage("a.txt");
    project.wait(SETTLE + Duration::from_millis(100));
    use CheckStatus::*;
    let give_up = Instant::now() + Duration::from_secs(30);
    while matches!(project.checks()[1].1, Pending | Running) {
        assert!(Instant::now() < give_up, "{:?}", project.checks());
        std::thread::sleep(Duration::from_millis(5));
        draw(project.cx);
    }
    let checks = project.checks();
    assert_eq!(checks[0], row("inside", Untrusted));
    // It ran: `true` passes, where there's a POSIX shell to run it.
    assert!(matches!(checks[1].1, Passed | Failed), "{checks:?}");
    #[cfg(unix)]
    assert_eq!(checks[1], row("fmt", Passed));
    let runs = project
        .workbench
        .read_with(project.cx, |w, _| w.runs().clone());
    assert_eq!(
        runs.read_with(project.cx, |runs, _| runs.list().count()),
        1,
        "the refused task began no run"
    );
}
