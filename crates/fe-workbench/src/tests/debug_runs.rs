//! A debug session as a run and as an operation (`debug_runs.rs`, debugger
//! card D7b): the window's debugger tells of a start and a session, and the
//! runs and the operations list follow. The debugger is made to emit the
//! events a session's states give, with no adapter.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_dap::{EndReason, Ended, SessionError, SessionState};
use fe_gpui::commands::RunCommand;
use fe_ui::Icon;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::{
    Launch, Workbench,
    debug_session::{Debugger, DebuggerEvent},
    runs::{DebugPhase, Run, RunKind, RunStatus, RunSummary},
    runs_panel::{Row, StatusIcon, row_selector},
    tests::open,
};

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
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn debugger(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Debugger> {
    workbench
        .update(cx, |workbench, cx| workbench.debugger(cx))
        .expect("the window's debugger")
}

fn tell(debugger: &Entity<Debugger>, event: DebuggerEvent, cx: &mut VisualTestContext) {
    debugger.update(cx, |_, cx| cx.emit(event));
    draw(cx);
}

fn state(debugger: &Entity<Debugger>, state: SessionState, cx: &mut VisualTestContext) {
    tell(debugger, DebuggerEvent::State(state), cx);
}

fn ended(code: Option<i64>, reason: EndReason) -> SessionState {
    SessionState::Ended(Ended {
        exit_code: code,
        reason,
    })
}

fn runs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Run> {
    let runs = workbench.read_with(cx, |workbench, _| workbench.runs().clone());
    runs.read_with(cx, |runs, _| runs.list().cloned().collect())
}

fn labels(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let operations = workbench.read_with(cx, |workbench, _| workbench.operations().clone());
    operations.read_with(cx, |ops, _| {
        ops.list().iter().map(|op| op.label.clone()).collect()
    })
}

/// C1: a start adds a run titled by its configuration, which runs while the
/// session does, stopped or not, and ends with the debuggee's exit code.
#[gpui::test]
fn a_session_is_a_run_from_start_to_end(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let debugger = debugger(&workbench, cx);
    assert_eq!(runs(&workbench, cx), []);

    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    let [run] = runs(&workbench, cx).try_into().unwrap();
    assert_eq!((run.kind, run.title.as_str()), (RunKind::Debug, "main"));
    assert!(run.status.is_running());
    assert_eq!(run.summary, RunSummary::Debug(DebugPhase::Starting));

    for (session, phase) in [
        (SessionState::Launching, DebugPhase::Starting),
        (SessionState::Configuring, DebugPhase::Starting),
        (SessionState::Running, DebugPhase::Running),
        (SessionState::Stopped, DebugPhase::Stopped),
        (SessionState::Resuming, DebugPhase::Running),
    ] {
        state(&debugger, session, cx);
        let runs = runs(&workbench, cx);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].status.is_running());
        assert_eq!(runs[0].summary, RunSummary::Debug(phase));
    }

    state(&debugger, ended(Some(3), EndReason::Finished), cx);
    let [run] = runs(&workbench, cx).try_into().unwrap();
    assert_eq!(run.status, RunStatus::Failed { code: Some(3) });
    assert_eq!(run.summary, RunSummary::Exit(3));

    // Another session: exit 0 passes, a stop without a code is cancelled.
    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    state(&debugger, SessionState::Launching, cx);
    state(&debugger, ended(Some(0), EndReason::Finished), cx);
    assert_eq!(runs(&workbench, cx)[0].status, RunStatus::Passed);
    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    state(&debugger, SessionState::Launching, cx);
    state(&debugger, SessionState::Running, cx);
    state(&debugger, ended(None, EndReason::Stopped), cx);
    let all = runs(&workbench, cx);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].status, RunStatus::Cancelled);
}

/// A session replaced by a start: its late end isn't the new run's.
#[gpui::test]
fn a_replaced_sessions_end_is_not_the_new_runs(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let debugger = debugger(&workbench, cx);
    tell(&debugger, DebuggerEvent::Starting("a".into()), cx);
    state(&debugger, SessionState::Launching, cx);
    state(&debugger, SessionState::Running, cx);
    tell(&debugger, DebuggerEvent::Starting("b".into()), cx);
    state(&debugger, ended(None, EndReason::Stopped), cx);
    let all = runs(&workbench, cx);
    assert_eq!(
        (all[0].title.as_str(), all[0].status.is_running()),
        ("b", true)
    );
    assert_eq!(all[1].status, RunStatus::Cancelled);
    state(&debugger, SessionState::Launching, cx);
    state(&debugger, SessionState::Running, cx);
    assert_eq!(
        runs(&workbench, cx)[0].summary,
        RunSummary::Debug(DebugPhase::Running)
    );
}

/// C2: a launch that fails ends its run failed, with the failure's message
/// as its summary.
#[gpui::test]
fn a_failed_launch_ends_its_run_failed(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let debugger = debugger(&workbench, cx);
    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    state(&debugger, SessionState::Launching, cx);
    let error = SessionError::StartFailed(Some("no such program".into()));
    state(&debugger, ended(None, EndReason::Failed(error.clone())), cx);
    let [run] = runs(&workbench, cx).try_into().unwrap();
    assert_eq!(run.status, RunStatus::Failed { code: None });
    let rows = rows(&run);
    assert_eq!(rows.summary, error.to_string());
    assert!(rows.summary.contains("no such program"));

    // A start given up before it launched fails its run too.
    tell(&debugger, DebuggerEvent::Starting("built".into()), cx);
    tell(&debugger, DebuggerEvent::StartGivenUp, cx);
    let all = runs(&workbench, cx);
    assert_eq!(all[0].status, RunStatus::Failed { code: None });
    assert_eq!(all[0].summary, RunSummary::None, "not Starting…");
    assert_eq!(
        Row::of(&all[0]).summary,
        tr!("workbench-runs-summary-no-code")
    );
}

/// A session that ends without an exit code says how it ended, not where
/// it was.
#[gpui::test]
fn an_ending_without_a_code_forgets_its_phase(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let debugger = debugger(&workbench, cx);
    for (reason, line) in [
        (EndReason::Stopped, tr!("workbench-runs-summary-cancelled")),
        (EndReason::Finished, tr!("workbench-runs-summary-passed")),
    ] {
        tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
        state(&debugger, SessionState::Launching, cx);
        state(&debugger, SessionState::Stopped, cx);
        state(&debugger, ended(None, reason), cx);
        let run = runs(&workbench, cx).remove(0);
        assert_eq!(run.summary, RunSummary::None);
        assert_eq!(Row::of(&run).summary, line);
    }
    // A replaced session is cancelled and says so.
    tell(&debugger, DebuggerEvent::Starting("a".into()), cx);
    state(&debugger, SessionState::Launching, cx);
    state(&debugger, SessionState::Running, cx);
    tell(&debugger, DebuggerEvent::Starting("b".into()), cx);
    let all = runs(&workbench, cx);
    assert_eq!(all[1].summary, RunSummary::None);
}

fn rows(run: &Run) -> Row {
    Row::of(run)
}

/// C3: from the start until the session is configured it's one operation,
/// "Debugging ‹configuration›", which a failure ends too.
#[gpui::test]
fn launching_is_an_operation_until_configured(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let debugger = debugger(&workbench, cx);
    let label = tr!("workbench-debug-operation", name = "main").to_string();
    assert_eq!(label, "Debugging main");
    assert_eq!(labels(&workbench, cx), Vec::<String>::new());

    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    assert_eq!(
        labels(&workbench, cx),
        std::slice::from_ref(&label),
        "the pre-launch task"
    );
    for launching in [
        SessionState::Launching,
        SessionState::Initializing,
        SessionState::Configuring,
    ] {
        state(&debugger, launching, cx);
        assert_eq!(labels(&workbench, cx), std::slice::from_ref(&label));
    }
    state(&debugger, SessionState::Running, cx);
    assert_eq!(labels(&workbench, cx), Vec::<String>::new(), "configured");

    // Failures end it: a start given up, a session that fails.
    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    assert_eq!(labels(&workbench, cx), std::slice::from_ref(&label));
    tell(&debugger, DebuggerEvent::StartGivenUp, cx);
    assert_eq!(labels(&workbench, cx), Vec::<String>::new());
    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    state(&debugger, SessionState::Launching, cx);
    let error = SessionError::Launch("gone".into());
    state(&debugger, ended(None, EndReason::Failed(error)), cx);
    assert_eq!(labels(&workbench, cx), Vec::<String>::new());
}

/// C4: the Runs panel draws a debug run with the `bug` icon, paused in a
/// muted colour while stopped, and a name that says it's a debug session.
#[gpui::test]
fn the_runs_panel_draws_a_debug_run(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let debugger = debugger(&workbench, cx);
    tell(&debugger, DebuggerEvent::Starting("main".into()), cx);
    state(&debugger, SessionState::Launching, cx);
    state(&debugger, SessionState::Running, cx);
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let show = RunCommand(Invocation::of("runs.show").unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(show), cx));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, _| workbench.runs_panel().clone());
    let rows = |cx: &mut VisualTestContext| -> Vec<Row> {
        panel.read_with(cx, |panel, cx| {
            panel.list().read(cx).delegate().rows().to_vec()
        })
    };
    let [row] = rows(cx).try_into().unwrap();
    assert_eq!(row.icon, StatusIcon::Debugging);
    assert_eq!(row.icon.icon(), Icon::Bug);
    assert_eq!(row.summary, tr!("workbench-debug-run-running"));
    assert!(cx.debug_bounds(row_selector(0).leak()).is_some(), "drawn");
    let name = row.name(std::time::Instant::now()).to_string();
    assert!(name.starts_with("Debug session main, Running, "), "{name}");
    let said = |status: &str| format!("Debug session main, {status}, ");
    state(&debugger, SessionState::Stopped, cx);
    let [row] = rows(cx).try_into().unwrap();
    let name = row.name(std::time::Instant::now()).to_string();
    assert!(name.starts_with(&said("Stopped")), "{name}");
    state(&debugger, SessionState::Running, cx);

    state(&debugger, SessionState::Stopped, cx);
    let [row] = rows(cx).try_into().unwrap();
    assert_eq!(row.icon, StatusIcon::Paused);
    assert_eq!(row.icon.icon(), Icon::Pause);
    assert_eq!(row.summary, tr!("workbench-debug-run-stopped"));

    state(&debugger, ended(Some(0), EndReason::Finished), cx);
    let [row] = rows(cx).try_into().unwrap();
    assert_eq!(row.icon, StatusIcon::Passed);
}
