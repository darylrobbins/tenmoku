use std::time::{Duration, Instant};

use fe_buffer::BufferSnapshot;

use crate::{ParseJob, Parsed, Syntax};

/// Runs [`ParseJob`]s off the calling thread. A UI implements it with its
/// executor; tests run the jobs when they choose.
pub trait Spawner {
    /// A job in flight. Dropping it cancels the job.
    type Task;

    /// Starts `job` in the background. Once it has run, its result must be
    /// passed to [`SyntaxDriver::finish`] on the driver's thread.
    fn spawn(&mut self, job: ParseJob) -> Self::Task;
}

/// Keeps a [`Syntax`] up to date with its buffer without blocking the caller
/// for longer than a budget.
///
/// Each [`SyntaxDriver::sync`] edits the tree to match the latest snapshot,
/// then reparses on the calling thread if that fits in the budget. If it
/// doesn't, one parse runs in the background while the edited tree keeps
/// highlighting, and later syncs only edit the tree until it finishes. Its
/// result is edited forward and used, and the next sync reparses whatever
/// changed meanwhile. Dropping the driver cancels the background parse.
pub struct SyntaxDriver<T> {
    syntax: Syntax,
    budget: Duration,
    clock: Box<dyn Fn() -> Instant>,
    /// The background parse, if one is running.
    task: Option<T>,
}

impl<T> SyntaxDriver<T> {
    /// A driver that spends at most `budget` per sync parsing on the calling
    /// thread.
    pub fn new(syntax: Syntax, budget: Duration) -> Self {
        Self {
            syntax,
            budget,
            clock: Box::new(Instant::now),
            task: None,
        }
    }

    /// Measures the budget with `clock` rather than the system clock.
    pub fn with_clock(mut self, clock: impl Fn() -> Instant + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    pub fn syntax(&self) -> &Syntax {
        &self.syntax
    }

    /// Whether a background parse is running.
    pub fn is_parsing(&self) -> bool {
        self.task.is_some()
    }

    /// Catches the tree up with `snapshot`: reparses within the budget, or
    /// else starts a background parse with `spawner` unless one is running.
    pub fn sync<S: Spawner<Task = T>>(&mut self, snapshot: &BufferSnapshot, spawner: &mut S) {
        if snapshot.id() != self.syntax.buffer_id() {
            // A parse of another buffer is no use.
            self.task = None;
        }
        self.syntax.edit(snapshot);
        if self.task.is_some() {
            return;
        }
        let (clock, budget) = (&self.clock, self.budget);
        let started = clock();
        if self.syntax.parse_until(budget, || {
            clock().saturating_duration_since(started) > budget
        }) {
            return;
        }
        self.task = Some(spawner.spawn(self.syntax.parse_job()));
    }

    /// Installs the result of the background parse. Sync again afterwards to
    /// reparse the edits made while it ran.
    pub fn finish(&mut self, parsed: Parsed) {
        if parsed.buffer_id() != self.syntax.buffer_id() {
            // From a cancelled parse of a previous buffer.
            return;
        }
        self.task = None;
        self.syntax.finish(parsed);
    }
}

#[cfg(test)]
mod tests;
