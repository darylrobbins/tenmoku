//! Find in a [`FileViewer`]: [`fe_fileview::find`] on the background
//! executor, stoppable, landing a match as the selection.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use fe_fileview::{Direction, FindOutcome, FindQuery, ReadError};
use futures::channel::oneshot;
use gpui::{AppContext as _, Context, Task};

use super::{FileViewer, FileViewerEvent};

/// The query, and the search running for it.
#[derive(Default)]
pub(super) struct FindState {
    query: Option<FindQuery>,
    running: Option<Running>,
    /// Holds scans, and records how they end.
    #[cfg(test)]
    pub(super) hold: Arc<Hold>,
}

/// A pause point for tests: a scan that starts waits for the next release
/// queued, so it is provably running when the test cancels it.
#[cfg(test)]
#[derive(Default)]
pub(super) struct Hold {
    /// The scans that have begun.
    pub(super) begun: std::sync::atomic::AtomicUsize,
    /// What each scan found, in the order they ended.
    pub(super) ended: std::sync::Mutex<Vec<Option<FindOutcome>>>,
    /// Scans take one each as they begin, and wait for it to be sent.
    pub(super) releases: std::sync::Mutex<std::collections::VecDeque<oneshot::Receiver<()>>>,
}

#[cfg(test)]
impl Hold {
    async fn begin(&self) {
        self.begun.fetch_add(1, Ordering::SeqCst);
        let release = self.releases.lock().unwrap().pop_front();
        if let Some(release) = release {
            release.await.ok();
        }
    }
}

/// A search: set to stop it at its next block.
struct Running {
    stop: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl FindState {
    /// Stops the search running, if any.
    pub(super) fn stop(&mut self) {
        self.running = None;
    }
}

impl FileViewer {
    /// Sets what [`FileViewer::find_next`] and [`FileViewer::find_prev`]
    /// look for, `None` for nothing, and stops any search running.
    pub fn set_find(&mut self, query: Option<FindQuery>, cx: &mut Context<Self>) {
        self.find.query = query;
        self.find.stop();
        cx.notify();
    }

    /// Selects the first match from the selection's end, wrapping.
    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        self.search(Direction::Forward, cx);
    }

    /// Selects the last match before the selection's start, wrapping.
    pub fn find_prev(&mut self, cx: &mut Context<Self>) {
        self.search(Direction::Backward, cx);
    }

    /// Whether a search is running.
    pub fn is_finding(&self) -> bool {
        self.find.running.is_some()
    }

    /// The running search's stop flag.
    #[cfg(test)]
    pub(super) fn stop_flag(&self) -> Option<Arc<AtomicBool>> {
        self.find
            .running
            .as_ref()
            .map(|running| running.stop.clone())
    }

    fn search(&mut self, direction: Direction, cx: &mut Context<Self>) {
        self.find.stop();
        let Some(query) = self.find.query.clone() else {
            return;
        };
        let selection = self.model.selection();
        let from = match direction {
            Direction::Forward => selection.end,
            Direction::Backward => selection.start,
        } as u64;
        // Only the rows found so far, while the index builds.
        let within = 0..self.model.text().len() as u64;
        let view = self.view.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        #[cfg(test)]
        let hold = self.find.hold.clone();
        // Detached: a stopped scan ends by its flag, at its next block, and
        // its result goes nowhere once the receiver is dropped.
        let (sender, receiver) = oneshot::channel();
        cx.background_spawn(async move {
            #[cfg(test)]
            hold.begin().await;
            let outcome = fe_fileview::find(&view, &query, within, from, direction, &flag);
            #[cfg(test)]
            hold.ended
                .lock()
                .unwrap()
                .push(outcome.as_ref().ok().cloned());
            sender.send(outcome).ok();
        })
        .detach();
        let task = cx.spawn(async move |this, cx| {
            let Ok(outcome) = receiver.await else {
                return;
            };
            this.update(cx, |viewer, cx| viewer.found(outcome, cx)).ok();
        });
        self.find.running = Some(Running { stop, _task: task });
        cx.notify();
    }

    fn found(&mut self, outcome: Result<FindOutcome, ReadError>, cx: &mut Context<Self>) {
        self.find.running = None;
        match outcome {
            Ok(FindOutcome::Found { range, wrapped }) => {
                self.model
                    .set_selection(range.start as usize, range.end as usize);
                self.restore = None;
                self.autoscroll = true;
                cx.emit(FileViewerEvent::Found { wrapped });
            }
            Ok(FindOutcome::NotFound) => cx.emit(FileViewerEvent::NotFound),
            Ok(FindOutcome::Stopped) => {}
            Err(_) => self.mark_changed(cx),
        }
        cx.notify();
    }
}
