//! Find in a [`HexViewer`]: [`fe_fileview::find_bytes`] on the background
//! executor, stoppable, landing a match as the selection. The file
//! viewer's search (its `find.rs`) over again, `Hold` test seam and all.

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_fileview::{Direction, FindOutcome, ReadError};
use futures::channel::oneshot;
use gpui::{AppContext as _, Context, Task};

use super::{HexViewer, HexViewerEvent};

/// What to look for: bytes, and whether ASCII letters match their case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ByteQuery {
    pub(crate) needle: Vec<u8>,
    pub(crate) case_sensitive: bool,
}

/// The query, the search running for it, and the match found last.
#[derive(Default)]
pub(super) struct FindState {
    pub(super) query: Option<ByteQuery>,
    running: Option<Running>,
    /// The last match found, while it's still the selection: the next
    /// search starts past it.
    pub(super) current: Option<Range<u64>>,
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

impl HexViewer {
    /// Sets what [`HexViewer::find_next`] and [`HexViewer::find_prev`]
    /// look for, `None` for nothing, and stops any search running.
    pub(crate) fn set_find(&mut self, query: Option<ByteQuery>, cx: &mut Context<Self>) {
        self.find.query = query;
        self.find.stop();
        // The match found last was of the query before.
        self.find.current = None;
        cx.notify();
    }

    /// Selects the first match from the selection, wrapping: past the
    /// match found last while it's selected, else from the caret's byte.
    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        self.search(Direction::Forward, false, cx);
    }

    /// Selects the last match before the selection, wrapping.
    pub fn find_prev(&mut self, cx: &mut Context<Self>) {
        self.search(Direction::Backward, false, cx);
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

    /// Searches in `direction` for the query, stopping the search running.
    /// With `from_selection`, forward starts at the selection's first byte,
    /// so a query typed over a match keeps it.
    pub(super) fn search(
        &mut self,
        direction: Direction,
        from_selection: bool,
        cx: &mut Context<Self>,
    ) {
        self.find.stop();
        let Some(query) = self.find.query.clone() else {
            return;
        };
        let selected = self.selected();
        let past_match = !from_selection
            && (self.find.current.as_ref() == Some(&selected) || selected.end - selected.start > 1);
        let from = match direction {
            Direction::Forward if past_match => selected.end,
            Direction::Forward => selected.start,
            Direction::Backward => selected.start,
        };
        let within = 0..self.view.len();
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
            let outcome = fe_fileview::find_bytes(
                &view,
                &query.needle,
                query.case_sensitive,
                within,
                from,
                direction,
                &flag,
            );
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
        self.find_said = None;
        cx.notify();
    }

    fn found(&mut self, outcome: Result<FindOutcome, ReadError>, cx: &mut Context<Self>) {
        self.find.running = None;
        match outcome {
            Ok(FindOutcome::Found { range, wrapped }) => {
                self.select(range.start, range.end - 1);
                self.find.current = Some(range);
                self.find_said = wrapped.then_some(super::FindSaid::Wrapped);
                cx.emit(HexViewerEvent::Found { wrapped });
            }
            Ok(FindOutcome::NotFound) => {
                self.find.current = None;
                self.find_said = Some(super::FindSaid::NotFound);
                cx.emit(HexViewerEvent::NotFound);
            }
            Ok(FindOutcome::Stopped) => {}
            Err(_) => self.mark_changed(cx),
        }
        cx.notify();
    }
}
