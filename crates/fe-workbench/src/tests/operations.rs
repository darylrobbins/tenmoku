//! The workbench's operations ([`crate::operations`]): language servers'
//! open progress, running task runs and other background work, as one list
//! ordered by start.

use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_lsp::{ServerId, WorkProgress, fake::FakeServer};
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use lsp_types::{
    NumberOrString, ProgressParams, ProgressParamsValue, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressEnd, WorkDoneProgressReport, notification::Progress,
};

use crate::{
    Launch, Workbench,
    operations::{Operation, OperationId, OperationSource, Operations},
    runs::{RunEnd, RunKind},
    task_defs::{Source, Task, TaskCommand},
    tests::{lsp::Lsp, open},
};

fn token(name: &str) -> NumberOrString {
    NumberOrString::String(name.into())
}

fn notify(fake: &FakeServer, name: &str, value: WorkDoneProgress) {
    fake.notify::<Progress>(ProgressParams {
        token: token(name),
        value: ProgressParamsValue::WorkDone(value),
    });
}

fn begin(fake: &FakeServer, name: &str, title: &str, message: Option<&str>, percentage: u32) {
    let begin = WorkDoneProgressBegin {
        title: title.into(),
        message: message.map(Into::into),
        percentage: Some(percentage),
        ..Default::default()
    };
    notify(fake, name, WorkDoneProgress::Begin(begin));
}

fn report(fake: &FakeServer, name: &str, message: &str, percentage: u32) {
    let report = WorkDoneProgressReport {
        message: Some(message.into()),
        percentage: Some(percentage),
        ..Default::default()
    };
    notify(fake, name, WorkDoneProgress::Report(report));
}

fn end(fake: &FakeServer, name: &str) {
    let end = WorkDoneProgressEnd { message: None };
    notify(fake, name, WorkDoneProgress::End(end));
}

fn task(label: &str, cwd: &Path) -> Task {
    Task {
        label: label.to_owned(),
        command: TaskCommand::Shell("true".to_owned()),
        cwd: cwd.to_owned(),
        env: BTreeMap::new(),
        group: None,
        default: false,
        source: Source::File,
        problem_matcher: None,
    }
}

fn operations(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Operations> {
    workbench.read_with(cx, |workbench, _| workbench.operations().clone())
}

/// Each operation: its id, label, detail, fraction and source.
fn listed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Operation> {
    operations(workbench, cx).read_with(cx, |operations, _| operations.list().to_vec())
}

fn labels(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    (listed(workbench, cx).into_iter())
        .map(|op| op.label)
        .collect()
}

fn primary(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(String, usize)> {
    operations(workbench, cx).read_with(cx, |operations, _| {
        let primary = operations.primary()?;
        Some((primary.label.clone(), operations.others()))
    })
}

fn begin_other(
    workbench: &Entity<Workbench>,
    label: &str,
    cx: &mut VisualTestContext,
) -> OperationId {
    operations(workbench, cx).update(cx, |operations, cx| operations.begin(label, cx))
}

fn end_other(workbench: &Entity<Workbench>, id: &OperationId, cx: &mut VisualTestContext) -> bool {
    operations(workbench, cx).update(cx, |operations, cx| operations.end(id, cx))
}

fn terminal_views(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Entity<TerminalView>> {
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

fn run_task(workbench: &Entity<Workbench>, task: Task, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx));
    });
    cx.run_until_parked();
}

/// A trusted workbench on a temporary project whose terminal panel starts
/// no process, with no language server.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    cx.run_until_parked();
    (workbench, cx, dir)
}

#[gpui::test]
fn lsp_progress_and_running_tasks_aggregate(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let terminal = s
        .workbench
        .read_with(&s.cx, |workbench, _| workbench.terminal().clone());
    terminal.update(&mut s.cx, |terminal, _| terminal.fake_sessions = true);
    s.open("src/main.rs");
    s.fake.wait_initialized();
    assert_eq!(listed(&s.workbench, &mut s.cx), []);

    // rust-analyzer's progress, with its message and percentage.
    let before = Instant::now();
    begin(&s.fake, "index", "Indexing", Some("1/4"), 25);
    s.until("the progress listed", |s| {
        labels(&s.workbench, &mut s.cx) == ["Indexing"]
    });
    // Started when fe-lsp heard its begin.
    let started = listed(&s.workbench, &mut s.cx)[0].started;
    assert!(before <= started && started <= Instant::now());
    let server = s
        .active_server()
        .flatten()
        .expect("rust-analyzer serves it");
    let indexing = |detail: &str, fraction: f32| Operation {
        id: OperationId::Lsp {
            server,
            token: token("index"),
        },
        label: "Indexing".into(),
        detail: Some(detail.into()),
        fraction: Some(fraction),
        source: OperationSource::LanguageServer {
            server,
            name: "rust-analyzer".into(),
        },
        started,
    };
    assert_eq!(listed(&s.workbench, &mut s.cx), [indexing("1/4", 0.25)]);

    // A task run, then other work: after it.
    let root = s.root.clone();
    run_task(&s.workbench, task("cargo build", &root), &mut s.cx);
    let other = begin_other(&s.workbench, "Saving", &mut s.cx);
    let ops = listed(&s.workbench, &mut s.cx);
    assert_eq!(
        (ops.iter().map(|op| op.label.as_str())).collect::<Vec<_>>(),
        ["Indexing", "cargo build", "Saving"]
    );
    let OperationSource::Run(run) = ops[1].source else {
        panic!("{:?}", ops[1]);
    };
    assert_eq!(ops[1].id, OperationId::Run(run));
    assert_eq!(ops[1].fraction, None);
    assert_eq!(ops[2].id, other);
    assert_eq!(ops[2].source, OperationSource::Other);

    // A report updates the server's in its place; a second token goes last.
    report(&s.fake, "index", "3/4", 75);
    begin(&s.fake, "check", "Checking", None, 0);
    s.until("the report and the second token", |s| {
        labels(&s.workbench, &mut s.cx).len() == 4
            && listed(&s.workbench, &mut s.cx)[0].fraction == Some(0.75)
    });
    let ops = listed(&s.workbench, &mut s.cx);
    assert_eq!(ops[0], indexing("3/4", 0.75));
    assert_eq!(
        (ops.iter().map(|op| op.label.as_str())).collect::<Vec<_>>(),
        ["Indexing", "cargo build", "Saving", "Checking"]
    );
    assert_eq!(ops[3].detail, None);
    assert_eq!(ops[3].fraction, Some(0.0));

    // The run's progress shows as the runs say it.
    let runs = s.workbench.read_with(&s.cx, |w, _| w.runs().clone());
    runs.update(&mut s.cx, |runs, cx| {
        runs.update(run, Some(0.5), Default::default(), cx)
    });
    assert_eq!(listed(&s.workbench, &mut s.cx)[1].fraction, Some(0.5));

    // The task's session exiting ends its run, and its operation.
    let view = terminal_views(&s.workbench, &mut s.cx).pop().unwrap();
    view.update(&mut s.cx, |_, cx| {
        cx.emit(TerminalViewEvent::Exited(Some(0)))
    });
    s.draw();
    assert_eq!(
        labels(&s.workbench, &mut s.cx),
        ["Indexing", "Saving", "Checking"]
    );

    // Ended progress goes; a crash takes the rest of the server's along.
    end(&s.fake, "index");
    s.until("the end", |s| {
        labels(&s.workbench, &mut s.cx) == ["Saving", "Checking"]
    });
    s.fake.crash_at(1);
    s.until("the crash", |s| {
        labels(&s.workbench, &mut s.cx) == ["Saving"]
    });

    // A new manager (here, the root's trust taken back) lists none of the
    // old one's.
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.fake.wait_initialized_at(2);
    begin(&s.fake, "again", "Again", None, 10);
    s.until("the restarted server's", |s| {
        labels(&s.workbench, &mut s.cx) == ["Saving", "Again"]
    });
    s.set_trust(false);
    s.until("the manager's gone", |s| {
        labels(&s.workbench, &mut s.cx) == ["Saving"]
    });
}

#[gpui::test]
fn the_primary_is_the_oldest_running(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    assert_eq!(primary(&workbench, cx), None);

    let first = begin_other(&workbench, "First", cx);
    assert_eq!(primary(&workbench, cx), Some(("First".into(), 0)));

    // A server's progress and a task begun later don't take its place.
    let server = ServerId::for_tests(7);
    let work = |name: &str, title: &str, percentage, started| {
        let mut work = WorkProgress::for_tests(token(name), title, started);
        work.percentage = percentage;
        work
    };
    let set_server = |work: &[WorkProgress], cx: &mut VisualTestContext| {
        operations(&workbench, cx).update(cx, |operations, cx| {
            operations.set_server(server, "ra", work, cx);
        });
    };
    let a_started = Instant::now();
    set_server(&[work("a", "Indexing", Some(10), a_started)], cx);
    let hidden_started = Instant::now();
    run_task(&workbench, task("test", dir.path()), cx);
    let second = begin_other(&workbench, "Second", cx);
    assert_eq!(
        labels(&workbench, cx),
        ["First", "Indexing", "test", "Second"]
    );
    assert_eq!(primary(&workbench, cx), Some(("First".into(), 3)));

    // Reported again, with a token begun since, the server's keeps its
    // place, and the new one goes in at its begin.
    let b_started = Instant::now();
    let a = work("a", "Indexing", Some(60), a_started);
    let b = work("b", "Checking", None, b_started);
    set_server(&[a.clone(), b.clone()], cx);
    assert_eq!(
        labels(&workbench, cx),
        ["First", "Indexing", "test", "Second", "Checking"]
    );
    assert_eq!(listed(&workbench, cx)[1].fraction, Some(0.6));
    assert_eq!(listed(&workbench, cx)[1].started, a_started);

    // A token the server's 64-token cap kept hidden until now goes in at
    // its begin, before the task begun after it, not after newer work.
    let hidden = work("h", "Hidden", None, hidden_started);
    set_server(&[a.clone(), hidden, b.clone()], cx);
    assert_eq!(
        labels(&workbench, cx),
        ["First", "Indexing", "Hidden", "test", "Second", "Checking"]
    );
    set_server(&[a.clone(), b.clone()], cx);

    // The runs tell of a run after the update that began it: one begun
    // before other work in the same update is still listed first.
    let (run, after) = workbench.update(cx, |workbench, cx| {
        let runs = workbench.runs().clone();
        let run = runs.update(cx, |runs, cx| runs.begin(RunKind::Build, "build", cx));
        let after = (workbench.operations().clone())
            .update(cx, |operations, cx| operations.begin("After", cx));
        (run, after)
    });
    cx.run_until_parked();
    assert_eq!(
        labels(&workbench, cx),
        [
            "First", "Indexing", "test", "Second", "Checking", "build", "After"
        ]
    );
    assert!(end_other(&workbench, &after, cx));
    let runs = workbench.read_with(cx, |workbench, _| workbench.runs().clone());
    runs.update(cx, |runs, cx| runs.finish(run, RunEnd::Passed, None, cx));
    cx.run_until_parked();

    // The oldest ending makes the next oldest still running the primary.
    assert!(end_other(&workbench, &first, cx));
    assert_eq!(primary(&workbench, cx), Some(("Indexing".into(), 3)));
    set_server(&[b], cx);
    assert_eq!(primary(&workbench, cx), Some(("test".into(), 2)));
    let view = terminal_views(&workbench, cx).pop().unwrap();
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(Some(1))));
    cx.run_until_parked();
    assert_eq!(primary(&workbench, cx), Some(("Second".into(), 1)));
    assert!(end_other(&workbench, &second, cx));
    assert_eq!(primary(&workbench, cx), Some(("Checking".into(), 0)));
    operations(&workbench, cx).update(cx, |operations, cx| operations.clear_servers(cx));
    assert_eq!(primary(&workbench, cx), None);
}

#[gpui::test]
fn end_removes_an_operation(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let ops = operations(&workbench, cx);
    let changes = std::rc::Rc::new(std::cell::Cell::new(0));
    let _observed = cx.update({
        let changes = changes.clone();
        |_, cx| cx.observe(&ops, move |_, _| changes.set(changes.get() + 1))
    });

    let a = begin_other(&workbench, "A", cx);
    let b = begin_other(&workbench, "B", cx);
    let c = begin_other(&workbench, "C", cx);
    assert_ne!(a, b);
    assert_eq!(labels(&workbench, cx), ["A", "B", "C"]);
    cx.run_until_parked();
    let before = changes.get();
    assert!(before > 0);

    // Ending one removes it alone, and tells those observing.
    assert!(end_other(&workbench, &b, cx));
    cx.run_until_parked();
    assert_eq!(labels(&workbench, cx), ["A", "C"]);
    assert!(changes.get() > before);

    // Once only: its id isn't reused, and updating it does nothing.
    let quiet = changes.get();
    assert!(!end_other(&workbench, &b, cx));
    let updated = ops.update(cx, |ops, cx| ops.update(&b, Some(0.5), None, cx));
    assert!(!updated);
    cx.run_until_parked();
    assert_eq!(changes.get(), quiet);
    let d = begin_other(&workbench, "D", cx);
    assert!(![&a, &b, &c].contains(&&d));

    // A running one's progress, clamped; then it ends too.
    let updated = ops.update(cx, |ops, cx| {
        ops.update(&c, Some(1.5), Some("most".into()), cx)
    });
    assert!(updated);
    let listed_c = listed(&workbench, cx)[1].clone();
    assert_eq!(
        (listed_c.fraction, listed_c.detail),
        (Some(1.0), Some("most".into()))
    );
    assert!(end_other(&workbench, &c, cx));
    assert_eq!(labels(&workbench, cx), ["A", "D"]);

    // Others end only where they come from.
    let server = ServerId::for_tests(1);
    let lsp = OperationId::Lsp {
        server,
        token: token("t"),
    };
    ops.update(cx, |ops, cx| {
        let work = [WorkProgress::for_tests(
            token("t"),
            "Indexing",
            Instant::now(),
        )];
        ops.set_server(server, "ra", &work, cx);
    });
    assert!(!end_other(&workbench, &lsp, cx));
    assert_eq!(labels(&workbench, cx), ["A", "D", "Indexing"]);
    ops.update(cx, |ops, cx| ops.set_server(server, "ra", &[], cx));
    assert_eq!(labels(&workbench, cx), ["A", "D"]);

    // A run finishing ends its operation, whatever the end.
    let runs = workbench.read_with(cx, |w, _| w.runs().clone());
    let run = runs.update(cx, |runs, cx| runs.begin(RunKind::Build, "build", cx));
    assert!(!end_other(&workbench, &OperationId::Run(run), cx));
    assert_eq!(labels(&workbench, cx), ["A", "D", "build"]);
    runs.update(cx, |runs, cx| runs.finish(run, RunEnd::Cancelled, None, cx));
    assert_eq!(labels(&workbench, cx), ["A", "D"]);

    assert!(end_other(&workbench, &a, cx));
    assert!(end_other(&workbench, &d, cx));
    assert_eq!(listed(&workbench, cx), []);
}
