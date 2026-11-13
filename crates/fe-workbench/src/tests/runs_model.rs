//! The workbench's runs ([`crate::runs`]), as task tabs on the terminal panel
//! feed them. Sessions here are fake: no process starts, and each test emits
//! the exit it wants.

use std::{cell::RefCell, collections::BTreeMap, fs, path::Path, rc::Rc};

use fe_buffer::Buffer;
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch, TerminalPanel, Workbench,
    runs::{MAX_FINISHED, RunEnd, RunId, RunKind, RunStatus, RunSummary, Runs, RunsEvent},
    task_defs::{Group, Source, Task, TaskCommand},
    tests::open,
};

/// A trusted workbench on a temporary project whose terminal panel starts no
/// process.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn task(label: &str, group: Option<Group>, cwd: &Path) -> Task {
    Task {
        label: label.to_owned(),
        command: TaskCommand::Shell("true".to_owned()),
        cwd: cwd.to_owned(),
        env: BTreeMap::new(),
        group,
        default: false,
        source: Source::File,
        problem_matcher: None,
    }
}

fn run_task(workbench: &Entity<Workbench>, task: Task, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx));
    });
    draw(cx);
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<TerminalView>> {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

/// Ends `view`'s fake session.
fn exit(view: &Entity<TerminalView>, code: Option<i32>, cx: &mut VisualTestContext) {
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(code)));
    draw(cx);
}

fn close_tab(workbench: &Entity<Workbench>, index: usize, cx: &mut VisualTestContext) {
    terminal(workbench, cx).update_in(cx, |terminal, window, cx| {
        terminal.activate(index, window, cx);
        terminal.close_active(window, cx);
    });
    draw(cx);
}

fn runs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Runs> {
    workbench.read_with(cx, |workbench, _| workbench.runs().clone())
}

/// Each run, newest first: its title, kind, status and summary.
fn listed(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(String, RunKind, RunStatus, RunSummary)> {
    runs(workbench, cx).read_with(cx, |runs, _| {
        (runs.list())
            .map(|run| (run.title.clone(), run.kind, run.status, run.summary))
            .collect()
    })
}

const RUNNING: RunStatus = RunStatus::Running { progress: None };

fn script(
    title: &str,
    status: RunStatus,
    summary: RunSummary,
) -> (String, RunKind, RunStatus, RunSummary) {
    (title.to_owned(), RunKind::Script, status, summary)
}

#[gpui::test]
fn a_task_run_appears_running_and_finishes_from_its_exit_code(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    assert_eq!(listed(&workbench, cx), []);
    let heard = Rc::new(RefCell::new(Vec::new()));
    let model = runs(&workbench, cx);
    let _changes = cx.update({
        let (heard, runs) = (heard.clone(), model);
        move |_, cx| {
            cx.subscribe(&runs, move |_, event: &RunsEvent, _| {
                heard.borrow_mut().push(*event)
            })
        }
    });

    run_task(&workbench, task("pass", None, dir.path()), cx);
    assert_eq!(
        listed(&workbench, cx),
        [script("pass", RUNNING, RunSummary::None)]
    );
    let first = runs(&workbench, cx).read_with(cx, |runs, _| runs.list().next().unwrap().clone());
    assert_eq!(first.finished, None);
    assert_eq!(heard.borrow().as_slice(), [RunsEvent::Changed(first.id)]);

    let passed = views(&workbench, cx).remove(0);
    exit(&passed, Some(0), cx);
    assert_eq!(
        listed(&workbench, cx),
        [script("pass", RunStatus::Passed, RunSummary::Exit(0))]
    );
    let finished =
        runs(&workbench, cx).read_with(cx, |runs, _| runs.get(first.id).unwrap().clone());
    assert!(finished.finished.is_some_and(|at| at >= finished.started));
    assert_eq!(heard.borrow().len(), 2, "told it finished");

    // Told once: a second exit changes nothing.
    exit(&passed, Some(3), cx);
    assert_eq!(
        listed(&workbench, cx),
        [script("pass", RunStatus::Passed, RunSummary::Exit(0))]
    );
    assert_eq!(heard.borrow().len(), 2);

    run_task(&workbench, task("fail", None, dir.path()), cx);
    let failed = views(&workbench, cx).remove(1);
    exit(&failed, Some(2), cx);
    run_task(&workbench, task("killed", None, dir.path()), cx);
    let killed = views(&workbench, cx).remove(2);
    exit(&killed, None, cx);
    assert_eq!(
        listed(&workbench, cx),
        [
            script("killed", RunStatus::Failed { code: None }, RunSummary::None),
            script(
                "fail",
                RunStatus::Failed { code: Some(2) },
                RunSummary::Exit(2)
            ),
            script("pass", RunStatus::Passed, RunSummary::Exit(0)),
        ]
    );
}

#[gpui::test]
fn closing_a_running_tab_cancels_its_run(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    run_task(&workbench, task("done", None, dir.path()), cx);
    run_task(&workbench, task("going", None, dir.path()), cx);
    run_task(&workbench, task("kept", None, dir.path()), cx);
    let done = views(&workbench, cx).remove(0);
    exit(&done, Some(0), cx);

    close_tab(&workbench, 1, cx);
    assert_eq!(
        listed(&workbench, cx),
        [
            script("kept", RUNNING, RunSummary::None),
            script("going", RunStatus::Cancelled, RunSummary::None),
            script("done", RunStatus::Passed, RunSummary::Exit(0)),
        ]
    );
    let going = runs(&workbench, cx).read_with(cx, |runs, _| runs.list().nth(1).unwrap().clone());
    assert!(going.finished.is_some());

    // A finished one's tab closing leaves it as it ended.
    close_tab(&workbench, 0, cx);
    assert_eq!(
        listed(&workbench, cx),
        [
            script("kept", RUNNING, RunSummary::None),
            script("going", RunStatus::Cancelled, RunSummary::None),
            script("done", RunStatus::Passed, RunSummary::Exit(0)),
        ]
    );
}

#[gpui::test]
fn a_rerun_is_a_new_run(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    let cargo = task("cargo test", Some(Group::Test), dir.path());
    run_task(&workbench, cargo.clone(), cx);
    let first = views(&workbench, cx).remove(0);
    exit(&first, Some(0), cx);

    run_task(&workbench, cargo.clone(), cx);
    let again = views(&workbench, cx);
    assert_eq!(again.len(), 1, "the same tab");
    let test = |status, summary| ("cargo test".to_owned(), RunKind::Test, status, summary);
    assert_eq!(
        listed(&workbench, cx),
        [
            test(RUNNING, RunSummary::None),
            test(RunStatus::Passed, RunSummary::Exit(0)),
        ]
    );
    let ids: Vec<RunId> =
        runs(&workbench, cx).read_with(cx, |runs, _| runs.list().map(|run| run.id).collect());
    assert_ne!(ids[0], ids[1]);

    // The old session's end is no longer heard.
    exit(&first, Some(2), cx);
    assert_eq!(
        listed(&workbench, cx),
        [
            test(RUNNING, RunSummary::None),
            test(RunStatus::Passed, RunSummary::Exit(0)),
        ]
    );

    exit(&again[0], Some(101), cx);
    assert_eq!(
        listed(&workbench, cx),
        [
            test(RunStatus::Failed { code: Some(101) }, RunSummary::Exit(101)),
            test(RunStatus::Passed, RunSummary::Exit(0)),
        ]
    );
}

#[gpui::test]
fn kind_follows_the_task_group(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    run_task(
        &workbench,
        task("build", Some(Group::Build), dir.path()),
        cx,
    );
    run_task(&workbench, task("test", Some(Group::Test), dir.path()), cx);
    run_task(&workbench, task("run", Some(Group::Run), dir.path()), cx);
    run_task(&workbench, task("other", None, dir.path()), cx);
    let kinds: Vec<(String, RunKind)> = runs(&workbench, cx).read_with(cx, |runs, _| {
        (runs.list())
            .map(|run| (run.title.clone(), run.kind))
            .collect()
    });
    let expected = [
        ("other", RunKind::Script),
        ("run", RunKind::Script),
        ("test", RunKind::Test),
        ("build", RunKind::Build),
    ];
    assert_eq!(
        kinds,
        expected.map(|(title, kind)| (title.to_owned(), kind))
    );
}

#[gpui::test]
fn finished_runs_are_capped_at_100_and_running_ones_stay(cx: &mut TestAppContext) {
    let model = cx.new(|_| Runs::default());
    model.update(cx, |runs, cx| {
        let oldest = runs.begin(RunKind::Agent, "lane", cx);
        let mut finished = Vec::new();
        for n in 0..MAX_FINISHED + 5 {
            let id = runs.begin(RunKind::Build, format!("build {n}"), cx);
            assert!(runs.finish(id, RunEnd::Passed, None, cx));
            finished.push(id);
        }
        assert_eq!(MAX_FINISHED, 100);
        assert_eq!(runs.list().count(), MAX_FINISHED + 1);
        let kept: Vec<RunId> = runs.list().map(|run| run.id).collect();
        assert_eq!(kept.last(), Some(&oldest), "running: never dropped");
        let mut newest: Vec<RunId> = finished[5..].to_vec();
        newest.reverse();
        assert_eq!(kept[..MAX_FINISHED], newest, "the oldest finished went");
        assert!(runs.get(finished[0]).is_none());

        // A dropped run can't be finished, nor a finished one again.
        assert!(!runs.finish(finished[0], RunEnd::Cancelled, None, cx));
        assert!(!runs.finish(finished[6], RunEnd::Cancelled, None, cx));
        assert_eq!(runs.get(finished[6]).unwrap().status, RunStatus::Passed);
        assert!(!runs.update(finished[6], Some(0.5), RunSummary::None, cx));

        // The running one still updates, its progress clamped.
        let summary = RunSummary::Tests {
            passed: 3,
            failed: 1,
            total: Some(10),
        };
        assert!(runs.update(oldest, Some(1.5), summary, cx));
        let run = runs.get(oldest).unwrap();
        assert_eq!(
            run.status,
            RunStatus::Running {
                progress: Some(1.0)
            }
        );
        assert_eq!(run.summary, summary);
        assert!(runs.update(oldest, Some(f32::NAN), summary, cx));
        assert_eq!(runs.get(oldest).unwrap().status, RUNNING);

        // Finishing it keeps its summary, and drops the one that finished
        // longest ago.
        assert!(runs.finish(oldest, RunEnd::Failed { code: Some(1) }, None, cx));
        assert_eq!(runs.list().count(), MAX_FINISHED);
        let run = runs
            .get(oldest)
            .expect("the newest finished, though begun first");
        assert_eq!(run.status, RunStatus::Failed { code: Some(1) });
        assert_eq!(run.summary, summary);
        assert!(runs.get(finished[5]).is_none());
    });
}

#[gpui::test]
fn the_list_is_newest_first(cx: &mut TestAppContext) {
    let model = cx.new(|_| Runs::default());
    model.update(cx, |runs, cx| {
        let a = runs.begin(RunKind::Build, "a", cx);
        let b = runs.begin(RunKind::Test, "b", cx);
        let c = runs.begin(RunKind::Script, "c", cx);
        // Finishing doesn't reorder: the list is by when each began.
        runs.finish(a, RunEnd::Passed, None, cx);
        runs.finish(c, RunEnd::Cancelled, None, cx);
        let order: Vec<RunId> = runs.list().map(|run| run.id).collect();
        assert_eq!(order, [c, b, a]);
    });

    let (workbench, cx, dir) = opened(cx);
    for label in ["one", "two", "three"] {
        run_task(&workbench, task(label, None, dir.path()), cx);
    }
    let titles: Vec<String> = runs(&workbench, cx).read_with(cx, |runs, _| {
        runs.list().map(|run| run.title.clone()).collect()
    });
    assert_eq!(titles, ["three", "two", "one"]);
}
