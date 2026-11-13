//! What's going on in the background, as one list: the language servers'
//! open work-done progress (fed by lsp.rs), the running task runs
//! ([`crate::runs`]), and any other work begun with [`Operations::begin`].
//!
//! Ordered by when each started, by the real clock: a run's start, a
//! progress token's begin as fe-lsp heard it, the call to `begin`. That's
//! never when it's heard of here: the runs' events come after the update
//! that began them, and a server lists only its 64 oldest tokens, so one
//! may come to light long after it began. The primary one, which the
//! status bar shows, is the oldest still running, and the rest are its
//! "+N" (docs/design/design-system.md, "Operation in progress"). An
//! operation is listed only while it runs: one that ends goes.

use std::time::Instant;

use fe_lsp::{ServerId, WorkProgress};
use gpui::{Context, Entity, Subscription};
use lsp_types::NumberOrString;

use crate::Workbench;
use crate::runs::{RunId, RunStatus, Runs, RunsEvent};

/// One operation's identity, unique in its [`Operations`] while it runs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OperationId {
    /// A language server's work-done progress `token`.
    Lsp {
        server: ServerId,
        token: NumberOrString,
    },
    /// A task run.
    Run(RunId),
    /// Work begun with [`Operations::begin`].
    Other(u64),
}

/// Where an operation comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationSource {
    /// The language server `name` (`server`).
    LanguageServer { server: ServerId, name: String },
    /// A task run, as [`Runs`] lists it.
    Run(RunId),
    /// Other background work.
    Other,
}

/// One thing running in the background.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    pub id: OperationId,
    /// What it's doing: a progress's title, a task's label.
    pub label: String,
    /// What it says it's at, if anything: a progress's latest message.
    pub detail: Option<String>,
    /// How far along it is, from 0 to 1, if known.
    pub fraction: Option<f32>,
    pub source: OperationSource,
    /// When it started, which orders the list.
    pub started: Instant,
}

/// The workbench's operations, oldest started first (those started at
/// once in the order heard of). Observe it for changes.
pub struct Operations {
    list: Vec<Operation>,
    next_other: u64,
    /// To the runs, to list the running ones.
    _runs: Subscription,
}

impl Operations {
    /// The operations of the runs in `runs` and nothing else yet, following
    /// `runs` from now on.
    pub(crate) fn new(runs: &Entity<Runs>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(runs, |operations, runs, _: &RunsEvent, cx| {
            let running = running(runs.read(cx));
            operations.set_runs(running, cx);
        });
        let mut operations = Self {
            list: Vec::new(),
            next_other: 0,
            _runs: subscription,
        };
        for op in running(runs.read(cx)) {
            operations.insert(op);
        }
        operations
    }

    /// Every operation running, oldest started first.
    pub fn list(&self) -> &[Operation] {
        &self.list
    }

    /// The oldest operation still running, which the status bar shows.
    pub fn primary(&self) -> Option<&Operation> {
        self.list.first()
    }

    /// How many run besides the [`primary`](Self::primary): its "+N".
    pub fn others(&self) -> usize {
        self.list.len().saturating_sub(1)
    }

    /// Lists background work labelled `label`, until it's
    /// [`end`](Self::end)ed.
    pub fn begin(&mut self, label: impl Into<String>, cx: &mut Context<Self>) -> OperationId {
        let id = OperationId::Other(self.next_other);
        self.next_other += 1;
        self.insert(Operation {
            id: id.clone(),
            label: label.into(),
            detail: None,
            fraction: None,
            source: OperationSource::Other,
            started: Instant::now(),
        });
        cx.notify();
        id
    }

    /// Says how far along the operation `id`, begun with
    /// [`begin`](Self::begin), is: `fraction` clamped to 0 to 1, and what
    /// it's at. False if it's not running, or not one begun so.
    pub fn update(
        &mut self,
        id: &OperationId,
        fraction: Option<f32>,
        detail: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(id, OperationId::Other(_)) {
            return false;
        }
        let Some(operation) = self.list.iter_mut().find(|op| op.id == *id) else {
            return false;
        };
        operation.fraction = clamp(fraction);
        operation.detail = detail;
        cx.notify();
        true
    }

    /// Ends the operation `id`, begun with [`begin`](Self::begin): it's
    /// listed no more. False if it wasn't running, or isn't one begun so:
    /// the others end where they come from.
    pub fn end(&mut self, id: &OperationId, cx: &mut Context<Self>) -> bool {
        if !matches!(id, OperationId::Other(_)) {
            return false;
        }
        let before = self.list.len();
        self.list.retain(|op| op.id != *id);
        let ended = self.list.len() != before;
        if ended {
            cx.notify();
        }
        ended
    }

    /// Lists `server`'s open `work` in place of what it listed before:
    /// what's gone goes, and what's new goes in at its start.
    pub(crate) fn set_server(
        &mut self,
        server: ServerId,
        name: &str,
        work: &[WorkProgress],
        cx: &mut Context<Self>,
    ) {
        let operations = work.iter().map(|work| Operation {
            id: OperationId::Lsp {
                server,
                token: work.token.clone(),
            },
            label: work.title.clone(),
            detail: work.message.clone(),
            fraction: work.fraction(),
            source: OperationSource::LanguageServer {
                server,
                name: name.to_owned(),
            },
            started: work.started,
        });
        let theirs =
            |op: &Operation| matches!(&op.id, OperationId::Lsp { server: s, .. } if *s == server);
        self.replace(theirs, operations.collect(), cx);
    }

    /// Lists no language server's work: their manager has gone.
    pub(crate) fn clear_servers(&mut self, cx: &mut Context<Self>) {
        self.replace(
            |op| matches!(op.id, OperationId::Lsp { .. }),
            Vec::new(),
            cx,
        );
    }

    /// Lists the `running` runs in place of those listed before.
    fn set_runs(&mut self, running: Vec<Operation>, cx: &mut Context<Self>) {
        self.replace(|op| matches!(op.id, OperationId::Run(_)), running, cx);
    }

    /// Replaces the operations `owned` picks with `fresh`: one already
    /// listed takes what `fresh` says of it, the rest go in at their start.
    fn replace(
        &mut self,
        owned: impl Fn(&Operation) -> bool,
        fresh: Vec<Operation>,
        cx: &mut Context<Self>,
    ) {
        let before = std::mem::take(&mut self.list);
        let mut fresh: Vec<Option<Operation>> = fresh.into_iter().map(Some).collect();
        for op in &before {
            if !owned(op) {
                self.list.push(op.clone());
                continue;
            }
            let same = fresh
                .iter_mut()
                .find(|new| new.as_ref().is_some_and(|new| new.id == op.id));
            if let Some(new) = same.and_then(Option::take) {
                self.insert(new);
            }
        }
        for new in fresh.into_iter().flatten() {
            self.insert(new);
        }
        if self.list != before {
            cx.notify();
        }
    }

    /// Puts `op` in the list after every one started no later.
    fn insert(&mut self, op: Operation) {
        let at = self
            .list
            .partition_point(|listed| listed.started <= op.started);
        self.list.insert(at, op);
    }
}

impl Workbench {
    /// What's going on in the background, for the status bar to show and
    /// observe, and for other work to [`begin`](Operations::begin) in.
    pub fn operations(&self) -> &Entity<Operations> {
        &self.operations
    }
}

/// The running runs in `runs`, as operations, oldest first.
fn running(runs: &Runs) -> Vec<Operation> {
    // `list` is newest first.
    let mut running: Vec<Operation> = (runs.list())
        .filter_map(|run| {
            let RunStatus::Running { progress } = run.status else {
                return None;
            };
            Some(Operation {
                id: OperationId::Run(run.id),
                label: run.title.clone(),
                detail: None,
                fraction: clamp(progress),
                source: OperationSource::Run(run.id),
                started: run.started,
            })
        })
        .collect();
    running.reverse();
    running
}

fn clamp(fraction: Option<f32>) -> Option<f32> {
    fraction.filter(|f| !f.is_nan()).map(|f| f.clamp(0.0, 1.0))
}
