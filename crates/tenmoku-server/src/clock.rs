//! The time the grace, the teardown and the daemon's linger read. A test
//! swaps in a [`ManualClock`] and moves it by hand, so nothing waits on real
//! time to see a session end (D § 12: no short sleeps for timing).
//!
//! fe-remote-proto has a manual clock too, but its timers are the mux's own:
//! nothing outside that crate can be woken when it moves.

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub enum Clock {
    #[default]
    System,
    Manual(ManualClock),
}

impl Clock {
    pub fn now(&self) -> Instant {
        match self {
            Clock::System => Instant::now(),
            Clock::Manual(m) => m.now(),
        }
    }

    /// Calls `wake` each time the clock moves, for as long as `wake` lives.
    /// The system clock moves by itself: its waiters set their own timeouts.
    pub(crate) fn subscribe(&self, wake: &Arc<dyn Fn() + Send + Sync>) {
        if let Clock::Manual(m) = self {
            m.0.state.lock().unwrap().woken.push(Arc::downgrade(wake));
        }
    }
}

/// A clock that moves only when [`ManualClock::advance`] moves it.
#[derive(Clone)]
pub struct ManualClock(Arc<Manual>);

struct Manual {
    start: Instant,
    state: Mutex<ManualState>,
}

struct ManualState {
    elapsed: Duration,
    woken: Vec<Weak<dyn Fn() + Send + Sync>>,
}

impl Default for ManualClock {
    fn default() -> ManualClock {
        ManualClock::new()
    }
}

impl ManualClock {
    pub fn new() -> ManualClock {
        ManualClock(Arc::new(Manual {
            start: Instant::now(),
            state: Mutex::new(ManualState {
                elapsed: Duration::ZERO,
                woken: Vec::new(),
            }),
        }))
    }

    pub fn now(&self) -> Instant {
        self.0.start + self.0.state.lock().unwrap().elapsed
    }

    /// Moves the clock on by `by` and wakes everything waiting on it.
    pub fn advance(&self, by: Duration) {
        let woken = {
            let mut s = self.0.state.lock().unwrap();
            s.elapsed += by;
            s.woken.retain(|w| w.strong_count() > 0);
            s.woken.clone()
        };
        for wake in woken.iter().filter_map(Weak::upgrade) {
            wake();
        }
    }
}
