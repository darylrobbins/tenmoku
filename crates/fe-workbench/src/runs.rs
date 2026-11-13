//! The runs one workbench knows: builds, tests and scripts, and later agent
//! tasks, in one list of one shape (`docs/design/design-system.md`, "Runs:
//! builds, tests and agent tasks in one list"). Each has a kind, a title, a
//! status, a one-line summary and the times that give its age; a running one
//! may know how far along it is.
//!
//! Titles and summaries are data: whatever lists them formats the text. Task
//! runs come here from [`crate::task_run`]; nothing makes an
//! [`RunKind::Agent`] run yet.

use std::{collections::VecDeque, time::Instant};

use gpui::{Context, EventEmitter};

/// How many finished runs are kept: those that finished longest ago go
/// first. Running ones are never dropped.
pub const MAX_FINISHED: usize = 100;

/// One run's identity, unique in its [`Runs`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(u64);

/// What a run does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    Build,
    Test,
    /// Anything else a task runs.
    Script,
    /// An agent's task, for the lanes.
    Agent,
}

/// Where a run is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunStatus {
    /// Still going, `progress` of the way there, from 0 to 1, if known.
    Running {
        progress: Option<f32>,
    },
    Passed,
    /// Ended badly, with its exit code if it had one: none when a signal
    /// ended it or it couldn't start.
    Failed {
        code: Option<i32>,
    },
    /// Stopped before it ended.
    Cancelled,
}

impl RunStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

/// How a run ended, for [`Runs::finish`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunEnd {
    Passed,
    Failed { code: Option<i32> },
    Cancelled,
}

impl From<RunEnd> for RunStatus {
    fn from(end: RunEnd) -> Self {
        match end {
            RunEnd::Passed => Self::Passed,
            RunEnd::Failed { code } => Self::Failed { code },
            RunEnd::Cancelled => Self::Cancelled,
        }
    }
}

/// What a run's one line says, as data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunSummary {
    /// Nothing yet.
    #[default]
    None,
    /// It exited with this code.
    Exit(i32),
    /// Tests counted so far: `total` once it's known.
    Tests {
        passed: usize,
        failed: usize,
        total: Option<usize>,
    },
    /// A build's crates built so far, and what they said.
    Build {
        crates: usize,
        warnings: usize,
        errors: usize,
    },
}

/// One build, test run, script or agent task.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub id: RunId,
    pub kind: RunKind,
    /// What it runs, as a task's label: shown in mono.
    pub title: String,
    pub status: RunStatus,
    pub summary: RunSummary,
    pub started: Instant,
    /// When it stopped running: none while it runs.
    pub finished: Option<Instant>,
}

impl Run {
    /// How long it's been since it stopped, or, while it runs, since it
    /// started.
    pub fn age(&self, now: Instant) -> std::time::Duration {
        now.saturating_duration_since(self.finished.unwrap_or(self.started))
    }
}

/// What [`Runs`] tells those that observe it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunsEvent {
    /// This run began, changed or finished: it may have pushed the oldest
    /// finished ones out.
    Changed(RunId),
}

/// The runs, oldest first, kept by the workbench.
#[derive(Debug, Default)]
pub struct Runs {
    runs: VecDeque<Run>,
    /// The finished runs kept, in the order they finished.
    finished: VecDeque<RunId>,
    next_id: u64,
}

impl EventEmitter<RunsEvent> for Runs {}

impl Runs {
    /// Adds a running run of `kind` named `title`, with no progress known.
    pub fn begin(
        &mut self,
        kind: RunKind,
        title: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> RunId {
        let id = RunId(self.next_id);
        self.next_id += 1;
        self.runs.push_back(Run {
            id,
            kind,
            title: title.into(),
            status: RunStatus::Running { progress: None },
            summary: RunSummary::None,
            started: Instant::now(),
            finished: None,
        });
        self.changed(id, cx);
        id
    }

    /// Says how far along the running run `id` is, `progress` clamped to 0
    /// to 1, and what it's done so far. One that's finished, or gone, is
    /// left as it was: false then.
    pub fn update(
        &mut self,
        id: RunId,
        progress: Option<f32>,
        summary: RunSummary,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(run) = self.running_mut(id) else {
            return false;
        };
        let progress = progress.filter(|p| !p.is_nan()).map(|p| p.clamp(0.0, 1.0));
        run.status = RunStatus::Running { progress };
        run.summary = summary;
        self.changed(id, cx);
        true
    }

    /// Ends the running run `id` as `end` says, with `summary` if it's
    /// given, else what it said last. One already finished, or gone, is left
    /// as it was: false then. The runs that finished longest ago, past
    /// [`MAX_FINISHED`], go.
    pub fn finish(
        &mut self,
        id: RunId,
        end: RunEnd,
        summary: Option<RunSummary>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(run) = self.running_mut(id) else {
            return false;
        };
        run.status = end.into();
        run.finished = Some(Instant::now());
        if let Some(summary) = summary {
            run.summary = summary;
        }
        self.finished.push_back(id);
        self.evict();
        self.changed(id, cx);
        true
    }

    /// The runs, newest first.
    pub fn list(&self) -> impl Iterator<Item = &Run> {
        self.runs.iter().rev()
    }

    /// The run `id`, if it's kept.
    pub fn get(&self, id: RunId) -> Option<&Run> {
        self.runs.iter().find(|run| run.id == id)
    }

    fn running_mut(&mut self, id: RunId) -> Option<&mut Run> {
        (self.runs.iter_mut()).find(|run| run.id == id && run.status.is_running())
    }

    /// Drops the runs that finished longest ago, past [`MAX_FINISHED`].
    fn evict(&mut self) {
        if self.finished.len() <= MAX_FINISHED {
            return;
        }
        let excess = self.finished.len() - MAX_FINISHED;
        let gone: Vec<RunId> = self.finished.drain(..excess).collect();
        self.runs.retain(|run| !gone.contains(&run.id));
    }

    fn changed(&mut self, id: RunId, cx: &mut Context<Self>) {
        cx.emit(RunsEvent::Changed(id));
        cx.notify();
    }
}
