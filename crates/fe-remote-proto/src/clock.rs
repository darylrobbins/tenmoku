//! The time the heartbeat and the bulk window read. A test swaps in a
//! [`ManualClock`] and moves it by hand, so nothing waits on real time to
//! see a ping or a lost link (D § 12: no short sleeps for timing).

use std::sync::{Arc, Condvar, Mutex, Weak};
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
    /// The muxes' timers, rung each time the clock moves.
    alarms: Vec<Weak<Alarm>>,
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
                alarms: Vec::new(),
            }),
        }))
    }

    pub fn now(&self) -> Instant {
        self.0.start + self.elapsed()
    }

    /// How far the clock has moved since it was made.
    pub fn elapsed(&self) -> Duration {
        self.0.state.lock().unwrap().elapsed
    }

    /// Moves the clock on by `by` and wakes every timer waiting on it.
    pub fn advance(&self, by: Duration) {
        let alarms = {
            let mut s = self.0.state.lock().unwrap();
            s.elapsed += by;
            s.alarms.retain(|a| a.strong_count() > 0);
            s.alarms.clone()
        };
        for a in alarms.iter().filter_map(Weak::upgrade) {
            a.poke();
        }
    }

    fn subscribe(&self, alarm: &Arc<Alarm>) {
        self.0
            .state
            .lock()
            .unwrap()
            .alarms
            .push(Arc::downgrade(alarm));
    }
}

/// What a mux's timer thread waits on: a deadline on its clock, or a stop.
pub(crate) struct Alarm {
    state: Mutex<AlarmState>,
    cv: Condvar,
}

#[derive(Default)]
struct AlarmState {
    stopped: bool,
}

impl Alarm {
    pub(crate) fn new(clock: &Clock) -> Arc<Alarm> {
        let alarm = Arc::new(Alarm {
            state: Mutex::default(),
            cv: Condvar::new(),
        });
        if let Clock::Manual(m) = clock {
            m.subscribe(&alarm);
        }
        alarm
    }

    fn poke(&self) {
        // Taking the lock orders this after a waiter's look at the time.
        let _s = self.state.lock().unwrap();
        self.cv.notify_all();
    }

    pub(crate) fn stop(&self) {
        self.state.lock().unwrap().stopped = true;
        self.cv.notify_all();
    }

    /// Waits until `clock` reaches `deadline`. False once stopped.
    pub(crate) fn wait_until(&self, clock: &Clock, deadline: Instant) -> bool {
        let mut s = self.state.lock().unwrap();
        loop {
            if s.stopped {
                return false;
            }
            let now = clock.now();
            if now >= deadline {
                return true;
            }
            s = match clock {
                Clock::System => self.cv.wait_timeout(s, deadline - now).unwrap().0,
                // Only `advance` moves this clock, and it pokes us. We hold
                // the lock from reading the time to waiting, so a poke
                // can't fall between them.
                Clock::Manual(_) => self.cv.wait(s).unwrap(),
            };
        }
    }
}
