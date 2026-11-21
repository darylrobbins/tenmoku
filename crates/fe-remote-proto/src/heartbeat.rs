//! The heartbeat (D § 2.5) as a state machine over a clock's instants: a
//! ping every interval, the round trip of each pong, and the link counted
//! lost after [`MISSED_PONGS`] pings in a row go unanswered. The mux runs
//! it on its timer thread; nothing here reads a clock itself.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often a peer pings.
pub const HEARTBEAT: Duration = Duration::from_secs(10);
/// Unanswered pings in a row that count the link as lost: 20 s at
/// [`HEARTBEAT`].
pub const MISSED_PONGS: u32 = 2;

/// The heartbeat's round trips, for the pill's tooltip and the bulk window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rtt {
    /// The last pong's round trip.
    pub latest: Duration,
    /// The shortest round trip since the link opened. The bulk window uses
    /// it rather than the latest, which a bulk stream queued ahead of the
    /// pong on the wire inflates.
    pub min: Duration,
}

/// What a tick asks of the mux.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beat {
    /// Send a ping with this nonce.
    Ping(u32),
    /// [`MISSED_PONGS`] pings in a row went unanswered.
    Lost,
}

pub struct Heartbeat {
    interval: Duration,
    due: Instant,
    /// Pings not answered yet, oldest first, with when each was sent.
    waiting: VecDeque<(u32, Instant)>,
    missed: u32,
    next_nonce: u32,
    /// Set on every nonce: `SERVER_ORIGINATED` for the server's pings.
    origin: u32,
    rtt: Option<Rtt>,
}

impl Heartbeat {
    /// The first ping is due at `start`, so the round trip is known from
    /// the start of the link. An interval under a millisecond is taken as
    /// one.
    pub fn new(start: Instant, interval: Duration, origin: u32) -> Heartbeat {
        Heartbeat {
            interval: interval.max(Duration::from_millis(1)),
            due: start,
            waiting: VecDeque::new(),
            missed: 0,
            next_nonce: 1,
            origin,
            rtt: None,
        }
    }

    /// When the next tick is due.
    pub fn due(&self) -> Instant {
        self.due
    }

    /// Nothing before the tick is due. Then either the next ping, or `Lost`
    /// when this is the [`MISSED_PONGS`]th tick in a row to find the last
    /// ping unanswered.
    pub fn tick(&mut self, now: Instant) -> Option<Beat> {
        if now < self.due {
            return None;
        }
        if !self.waiting.is_empty() {
            self.missed += 1;
            if self.missed >= MISSED_PONGS {
                return Some(Beat::Lost);
            }
        }
        // A timer that woke late (a suspended laptop) owes one ping, not
        // one per interval it slept through.
        while self.due <= now {
            self.due += self.interval;
        }
        let nonce = self.origin | self.next_nonce;
        self.next_nonce = self.next_nonce.wrapping_add(1) & !self.origin;
        self.waiting.push_back((nonce, now));
        if self.waiting.len() > MISSED_PONGS as usize {
            self.waiting.pop_front();
        }
        Some(Beat::Ping(nonce))
    }

    /// A pong arrived. A nonce still waiting gives its round trip and
    /// resets the missed count; any other is ignored.
    pub fn pong(&mut self, nonce: u32, now: Instant) -> Option<Duration> {
        let at = self.waiting.iter().position(|(n, _)| *n == nonce)?;
        let sent = self.waiting[at].1;
        // Older pings than this one won't be answered now.
        self.waiting.drain(..=at);
        self.missed = 0;
        let rtt = now.saturating_duration_since(sent);
        self.rtt = Some(Rtt {
            latest: rtt,
            min: self.rtt.map_or(rtt, |r| r.min.min(rtt)),
        });
        Some(rtt)
    }

    pub fn rtt(&self) -> Option<Rtt> {
        self.rtt
    }
}
