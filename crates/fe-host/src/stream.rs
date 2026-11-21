//! What a host hands back over time: a [`Stream`] of batches that ends with
//! a value and is cancelled by dropping it, and a [`Subscription`] that runs
//! until it is dropped (docs/decisions/7p724bm90r-watch-search-host.md).
//!
//! Both are plain blocking channels with no async runtime behind them: read
//! them off the UI thread, as the code they replace ran.

use std::{
    fmt,
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError},
    },
    time::Duration,
};

/// Items a host sends as it finds them, then the value it ended with:
/// `T` is a batch, `E` how the work went. Dropping the stream cancels the
/// work: the producer's next [`StreamSender::send`] fails, and its cancel
/// flag is set.
pub struct Stream<T, E> {
    items: Receiver<T>,
    shared: Arc<Shared<E>>,
}

/// The producer's side of a [`Stream`], made with [`Stream::channel`].
/// Dropping it ends the stream, with the value [`StreamSender::end`] gave or
/// none.
pub struct StreamSender<T, E> {
    items: SyncSender<T>,
    shared: Arc<Shared<E>>,
}

/// Waits for a [`Stream`]'s producer to end, even after the stream itself is
/// dropped: [`Stream::ended`].
pub struct Ended<E> {
    shared: Arc<Shared<E>>,
}

struct Shared<E> {
    cancel: Arc<AtomicBool>,
    end: Mutex<End<E>>,
    ended: Condvar,
}

struct End<E> {
    value: Option<E>,
    /// The sender is gone.
    done: bool,
}

impl<E> Shared<E> {
    fn new(value: Option<E>, done: bool) -> Arc<Self> {
        Arc::new(Shared {
            cancel: Arc::new(AtomicBool::new(false)),
            end: Mutex::new(End { value, done }),
            ended: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, End<E>> {
        self.end.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The end value, once the sender is gone.
    fn wait(&self) -> Option<E> {
        let mut end = self.lock();
        while !end.done {
            end = (self.ended.wait(end)).unwrap_or_else(PoisonError::into_inner);
        }
        end.value.take()
    }
}

impl<T, E> Stream<T, E> {
    /// A stream and its sender, with at most `bound` items sent and not yet
    /// received: past that [`StreamSender::send`] blocks until the stream
    /// takes one, so a slow reader holds the producer up. A `bound` of 0
    /// hands each item over directly.
    pub fn channel(bound: usize) -> (StreamSender<T, E>, Stream<T, E>) {
        let (tx, items) = mpsc::sync_channel(bound);
        let shared = Shared::new(None, false);
        let sender = StreamSender {
            items: tx,
            shared: shared.clone(),
        };
        (sender, Stream { items, shared })
    }

    /// A stream with no items that has already ended with `value`: what a
    /// host returns for work it refused before starting.
    pub fn ended_with(value: E) -> Stream<T, E> {
        let (_, items) = mpsc::sync_channel(0);
        Stream {
            items,
            shared: Shared::new(Some(value), true),
        }
    }

    /// The next item, waiting for it; `None` once the producer has ended and
    /// every item it sent has been received.
    pub fn recv(&self) -> Option<T> {
        self.items.recv().ok()
    }

    /// The next item if one is waiting: [`TryRecvError::Empty`] if none is
    /// yet, [`TryRecvError::Disconnected`] once the stream has ended.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.items.try_recv()
    }

    /// [`Stream::recv`], waiting at most `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        self.items.recv_timeout(timeout)
    }

    /// Discards the items not yet received, without cancelling, and waits
    /// for the producer's end value; `None` if it ended without one (it
    /// panicked, say). After [`Stream::recv`] returned `None` this doesn't
    /// wait.
    pub fn finish(self) -> Option<E> {
        while self.items.recv().is_ok() {}
        self.shared.wait()
    }

    /// Cancels, as dropping the stream does, then waits for the producer to
    /// end and returns its end value.
    pub fn cancel(self) -> Option<E> {
        let ended = self.ended();
        drop(self);
        ended.wait()
    }

    /// A handle that waits for the producer to end, whether or not the
    /// stream is still held.
    pub fn ended(&self) -> Ended<E> {
        Ended {
            shared: self.shared.clone(),
        }
    }
}

impl<T, E> Iterator for Stream<T, E> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.recv()
    }
}

impl<T, E> Drop for Stream<T, E> {
    fn drop(&mut self) {
        // Then `items` drops, which wakes a sender blocked on a full channel.
        self.shared.cancel.store(true, Ordering::Relaxed);
    }
}

impl<T, E> fmt::Debug for Stream<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream").finish_non_exhaustive()
    }
}

impl<T, E> StreamSender<T, E> {
    /// Sends `item`, waiting while the stream's bound is full. `false` once
    /// the stream is cancelled or dropped: the item wasn't sent, and the
    /// producer should stop.
    pub fn send(&self, item: T) -> bool {
        if self.is_cancelled() {
            return false;
        }
        match self.items.send(item) {
            Ok(()) => true,
            Err(_) => {
                self.shared.cancel.store(true, Ordering::Relaxed);
                false
            }
        }
    }

    /// Whether the stream was dropped or cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::Relaxed)
    }

    /// The flag dropping the stream sets, for work that takes a cancel flag
    /// of its own.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.shared.cancel.clone()
    }

    /// Ends the stream with `value`.
    pub fn end(self, value: E) {
        self.shared.lock().value = Some(value);
    }
}

impl<T, E> Drop for StreamSender<T, E> {
    fn drop(&mut self) {
        // Before `items` drops, so a reader that sees the stream end finds
        // the end value already set.
        self.shared.lock().done = true;
        self.shared.ended.notify_all();
    }
}

impl<T, E> fmt::Debug for StreamSender<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamSender").finish_non_exhaustive()
    }
}

impl<E> Ended<E> {
    /// Waits for the producer to end, and takes its end value: `None` if it
    /// ended without one, or the stream's [`Stream::finish`] took it.
    pub fn wait(self) -> Option<E> {
        self.shared.wait()
    }

    /// Whether the producer has ended.
    pub fn is_done(&self) -> bool {
        self.shared.lock().done
    }
}

impl<E> fmt::Debug for Ended<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ended").finish_non_exhaustive()
    }
}

/// Items a host sends until the subscription is dropped, which stops what
/// sends them. Items are never held up by a slow reader: they queue.
pub struct Subscription<T> {
    items: Receiver<T>,
    /// Dropped after `items`, so what sends into it finds it gone.
    _source: Box<dyn Send>,
}

impl<T> Subscription<T> {
    /// The items `items` receives, for as long as `source`, which sends
    /// them and stops when dropped, is held.
    pub fn new(items: Receiver<T>, source: impl Send + 'static) -> Subscription<T> {
        Subscription {
            items,
            _source: Box::new(source),
        }
    }

    /// The next item, waiting for it; `None` once nothing will send another.
    pub fn recv(&self) -> Option<T> {
        self.items.recv().ok()
    }

    /// The next item if one is waiting.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.items.try_recv()
    }

    /// [`Subscription::recv`], waiting at most `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        self.items.recv_timeout(timeout)
    }
}

impl<T> Iterator for Subscription<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.recv()
    }
}

impl<T> fmt::Debug for Subscription<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
