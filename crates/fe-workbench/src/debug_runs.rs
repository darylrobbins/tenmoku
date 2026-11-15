//! A debug session as a run and as an operation (debugger card D7b;
//! `docs/design/design-system.md`, Debugger, "Everywhere else").
//!
//! It follows a window's [`Debugger`] through its [`DebuggerEvent`]s:
//!
//! - **A run** of the kind [`RunKind::Debug`], titled by the configuration,
//!   begins when a start does and runs until the session ends: its summary
//!   says whether it is starting, running or stopped
//!   ([`RunSummary::Debug`]); it ends with the debuggee's exit code if the
//!   adapter told one, else as a finished, stopped or failed session does.
//!   A failure's message, which the toast gives as its detail, is the run's
//!   note. A start given up before its session launched, because the
//!   pre-launch task failed or the start was replaced, ends failed.
//! - **An operation** in the status bar's list, "Debugging ‹configuration›",
//!   from the start until the session is configured, or the start or the
//!   session fails. A running debug run isn't listed as an operation of its
//!   own ([`crate::operations`]): this is its entry.
//!
//! The session's own changes are announced by `debug_session.rs`.

use std::collections::HashMap;

use fe_dap::{EndReason, Ended, SessionState};
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global};
use kiln_l10n::tr;

use crate::{
    Workbench,
    debug_session::DebuggerEvent,
    operations::{OperationId, Operations},
    runs::{DebugPhase, RunEnd, RunId, RunKind, RunSummary, Runs},
};

/// What a window keeps of its debug session's run and operation.
struct DebugRuns {
    runs: Entity<Runs>,
    operations: Entity<Operations>,
    /// The configuration of the start or session under way.
    name: Option<String>,
    run: Option<RunId>,
    /// Whether the session the run follows has launched: until then the
    /// states are the session's before it, ending as the start replaces it.
    launched: bool,
    operation: Option<OperationId>,
}

/// Each window's, by its workbench.
#[derive(Default)]
struct Followers(HashMap<EntityId, Entity<DebugRuns>>);

impl Global for Followers {}

/// Makes each window follow its debugger. Registered after
/// [`crate::debug_session::init`], which makes the debugger.
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workbench: &mut Workbench, _, cx| {
        let Some(debugger) = workbench.debugger(cx) else {
            return;
        };
        let (runs, operations) = (workbench.runs().clone(), workbench.operations().clone());
        let follower = cx.new(|cx| {
            cx.subscribe(&debugger, |this: &mut DebugRuns, _, event, cx| {
                this.heard(event, cx)
            })
            .detach();
            DebugRuns {
                runs,
                operations,
                name: None,
                run: None,
                launched: false,
                operation: None,
            }
        });
        let id = cx.entity_id();
        cx.default_global::<Followers>().0.insert(id, follower);
        cx.on_release(move |_, cx| {
            if cx.has_global::<Followers>() {
                cx.global_mut::<Followers>().0.remove(&id);
            }
        })
        .detach();
    })
    .detach();
}

impl DebugRuns {
    fn heard(&mut self, event: &DebuggerEvent, cx: &mut Context<Self>) {
        match event {
            DebuggerEvent::Starting(name) => self.starting(name, cx),
            DebuggerEvent::StartGivenUp => self.given_up(cx),
            DebuggerEvent::State(state) => self.state(state, cx),
            DebuggerEvent::Stopped(_)
            | DebuggerEvent::Output(_)
            | DebuggerEvent::Breakpoints(_) => {}
        }
    }

    /// A start begins: its run and operation. One still under way, which
    /// the debugger gave up first, has ended already.
    fn starting(&mut self, name: &str, cx: &mut Context<Self>) {
        // The session this replaces, if it's still going, is stopped.
        self.end_run(RunEnd::Cancelled, None, None, cx);
        self.end_operation(cx);
        self.name = Some(name.to_owned());
        let run = self.runs.update(cx, |runs, cx| {
            let run = runs.begin(RunKind::Debug, name, cx);
            runs.update(run, None, RunSummary::Debug(DebugPhase::Starting), cx);
            run
        });
        self.run = Some(run);
        self.launched = false;
        let label = tr!("workbench-debug-operation", name = name).to_string();
        self.operation = Some(self.operations.update(cx, |ops, cx| ops.begin(label, cx)));
    }

    /// The start ended before its session launched.
    fn given_up(&mut self, cx: &mut Context<Self>) {
        self.end_operation(cx);
        self.end_run(RunEnd::Failed { code: None }, None, None, cx);
    }

    fn state(&mut self, state: &SessionState, cx: &mut Context<Self>) {
        use SessionState::*;
        if self.run.is_none() {
            return;
        }
        if !self.launched {
            // The new session's first state: what came before was the old
            // session's.
            if *state != Launching {
                return;
            }
            self.launched = true;
        }
        let phase = match state {
            Launching | Initializing | Configuring => DebugPhase::Starting,
            Running | Resuming | Uncertain | Restarting | Terminating => DebugPhase::Running,
            Stopped => DebugPhase::Stopped,
            Ended(ended) => {
                self.end_operation(cx);
                self.ended(ended, cx);
                return;
            }
        };
        if phase != DebugPhase::Starting {
            // Configured.
            self.end_operation(cx);
        }
        if let Some(run) = self.run {
            self.runs.update(cx, |runs, cx| {
                runs.update(run, None, RunSummary::Debug(phase), cx);
            });
        }
    }

    /// The session ended as `ended` says.
    fn ended(&mut self, ended: &Ended, cx: &mut Context<Self>) {
        let (end, note) = match (&ended.reason, ended.exit_code) {
            (EndReason::Failed(error), _) => {
                (RunEnd::Failed { code: None }, Some(error.to_string()))
            }
            (EndReason::AdapterExited(_), _) => {
                let adapter_gone = tr!("workbench-debug-run-adapter-exited").to_string();
                (RunEnd::Failed { code: None }, Some(adapter_gone))
            }
            (_, Some(0)) => (RunEnd::Passed, None),
            (_, Some(code)) => (
                RunEnd::Failed {
                    code: Some(code as i32),
                },
                None,
            ),
            (EndReason::Stopped, None) => (RunEnd::Cancelled, None),
            (EndReason::Finished, None) => (RunEnd::Passed, None),
        };
        let summary = ended.exit_code.map(|code| RunSummary::Exit(code as i32));
        self.end_run(end, summary, note, cx);
    }

    fn end_run(
        &mut self,
        end: RunEnd,
        summary: Option<RunSummary>,
        note: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.run.take() else {
            return;
        };
        self.name = None;
        // An ending with no exit code leaves no phase to say: the terminal
        // status, or the note, supplies the line.
        let summary = Some(summary.unwrap_or(RunSummary::None));
        self.runs.update(cx, |runs, cx| match note {
            Some(note) => runs.finish_with_note(run, end, summary, note, cx),
            None => runs.finish(run, end, summary, cx),
        });
    }

    fn end_operation(&mut self, cx: &mut Context<Self>) {
        if let Some(operation) = self.operation.take() {
            self.operations
                .update(cx, |ops, cx| ops.end(&operation, cx));
        }
    }
}
