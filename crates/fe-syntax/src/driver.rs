use std::{
    ops::Range,
    time::{Duration, Instant},
};

use fe_buffer::{BufferSnapshot, Version};

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
///
/// A file over its language's parse limit is highlighted through a window around the
/// visible text (see [`Syntax::window_job`]), always parsed in the
/// background. While one window parses, the last keeps highlighting, and
/// syncs only record the viewport; when it lands, [`SyntaxDriver::finish`]
/// starts one parse for the latest viewport, if that still needs one.
pub struct SyntaxDriver<T> {
    syntax: Syntax,
    budget: Duration,
    clock: Box<dyn Fn() -> Instant>,
    /// The background parse, if one is running: of the whole file, or of a
    /// window for a viewport. Only one runs at a time, of either kind.
    task: Option<(T, Option<WindowRequest>)>,
    /// What the installed window was parsed for.
    window: Option<WindowRequest>,
    /// The viewport at the last sync.
    visible: Range<usize>,
}

/// A window parse: the version it parses and the viewport it's for.
struct WindowRequest {
    version: Version,
    visible: Range<usize>,
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
            window: None,
            visible: 0..0,
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
    /// For a file over the language's parse limit, starts a window parse around
    /// `visible`, the byte range on screen, unless the window covers it.
    pub fn sync<S: Spawner<Task = T>>(
        &mut self,
        snapshot: &BufferSnapshot,
        visible: Range<usize>,
        spawner: &mut S,
    ) {
        if snapshot.id() != self.syntax.buffer_id() {
            // Nothing of another buffer is any use: not its parse, its
            // window, or that it failed.
            self.task = None;
            self.window = None;
            self.syntax = Syntax::new(self.syntax.language(), snapshot);
        }
        self.syntax.edit(snapshot);
        self.visible = visible;
        if self.task.is_some() {
            return;
        }
        if snapshot.len() > self.syntax.language().parse_limit() {
            self.sync_window(spawner);
            return;
        }
        let (clock, budget) = (&self.clock, self.budget);
        let started = clock();
        if self.syntax.parse_until(budget, || {
            clock().saturating_duration_since(started) > budget
        }) {
            return;
        }
        self.task = Some((spawner.spawn(self.syntax.parse_job()), None));
    }

    /// Starts a window parse for the latest viewport unless the window is up
    /// to date and covers it, or was parsed for it already: a viewport wider
    /// than a window is never covered. Nothing for a file under the limit.
    fn sync_window<S: Spawner<Task = T>>(&mut self, spawner: &mut S) {
        let visible = self.visible.clone();
        let version = self.syntax.version();
        let current = self.syntax.window().zip(
            self.window
                .as_ref()
                .filter(|parsed| parsed.version == version),
        );
        if let Some((window, parsed)) = current
            && (window.start <= visible.start && visible.end <= window.end
                || parsed.visible == visible)
        {
            return;
        }
        if let Some(job) = self.syntax.window_job(visible.clone()) {
            let request = WindowRequest { version, visible };
            self.task = Some((spawner.spawn(job), Some(request)));
        }
    }

    /// Installs the result of the background parse. For a file over the
    /// limit, then starts one window parse for the latest viewport if it
    /// needs one. Sync again afterwards to reparse the edits made while it
    /// ran.
    pub fn finish<S: Spawner<Task = T>>(&mut self, parsed: Parsed, spawner: &mut S) {
        if parsed.buffer_id() != self.syntax.buffer_id() {
            // From a cancelled parse of a previous buffer.
            return;
        }
        if let Some((_, Some(request))) = self.task.take() {
            self.window = Some(request);
        }
        self.syntax.finish(parsed);
        self.sync_window(spawner);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod window_tests;
