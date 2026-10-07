//! Time, injected. Every timer in fe-lsp goes through a [`Clock`], so tests
//! drive timeouts (and later card 4's backoff) by hand.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

/// A source of time and timers.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    /// Resolves once `duration` has passed on this clock.
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// Real time, on `async_io::Timer`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let timer = async_io::Timer::after(duration);
        Box::pin(async move {
            timer.await;
        })
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use manual::ManualClock;

#[cfg(any(test, feature = "test-support"))]
mod manual {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    use futures::channel::oneshot;

    use super::Clock;

    /// A clock that moves only when a test calls [`ManualClock::advance`].
    pub struct ManualClock {
        state: Mutex<State>,
        changed: Condvar,
    }

    struct State {
        start: Instant,
        elapsed: Duration,
        sleepers: Vec<(Duration, oneshot::Sender<()>)>,
    }

    impl Default for ManualClock {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ManualClock {
        pub fn new() -> Self {
            Self {
                state: Mutex::new(State {
                    start: Instant::now(),
                    elapsed: Duration::ZERO,
                    sleepers: Vec::new(),
                }),
                changed: Condvar::new(),
            }
        }

        fn lock(&self) -> MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Moves time forward, waking every sleep that has come due.
        pub fn advance(&self, by: Duration) {
            let mut state = self.lock();
            state.elapsed += by;
            let now = state.elapsed;
            let (due, waiting) = std::mem::take(&mut state.sleepers)
                .into_iter()
                .partition(|(deadline, _)| *deadline <= now);
            state.sleepers = waiting;
            drop(state);
            for (_, wake) in due {
                let _ = wake.send(());
            }
        }

        /// The time left on every sleep still being waited for.
        pub fn sleeping(&self) -> Vec<Duration> {
            let mut state = self.lock();
            state.sleepers.retain(|(_, wake)| !wake.is_canceled());
            let now = state.elapsed;
            state
                .sleepers
                .iter()
                .map(|(deadline, _)| *deadline - now)
                .collect()
        }

        /// Blocks until something sleeps for exactly `left` from now, so a
        /// test can [`advance`](Self::advance) knowing the timer is armed.
        /// Panics after 10 s of real time.
        pub fn wait_for_sleep(&self, left: Duration) {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut state = self.lock();
            loop {
                let now = state.elapsed;
                if state
                    .sleepers
                    .iter()
                    .any(|(at, wake)| *at - now == left && !wake.is_canceled())
                {
                    return;
                }
                let real = Instant::now();
                assert!(real < deadline, "nothing slept for {left:?}");
                state = self
                    .changed
                    .wait_timeout(state, (deadline - real).min(Duration::from_millis(10)))
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            let state = self.lock();
            state.start + state.elapsed
        }

        fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            let (wake, woken) = oneshot::channel();
            let mut state = self.lock();
            if duration.is_zero() {
                let _ = wake.send(());
            } else {
                let deadline = state.elapsed + duration;
                state.sleepers.push((deadline, wake));
            }
            drop(state);
            self.changed.notify_all();
            Box::pin(async move {
                // A dropped clock never fires: sleep forever, like real time
                // would for a timer nobody drives.
                if woken.await.is_err() {
                    futures::future::pending::<()>().await;
                }
            })
        }
    }
}
