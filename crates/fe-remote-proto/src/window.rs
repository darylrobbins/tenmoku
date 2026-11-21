//! Credit windows (D § 2.4). Credits count `Data` body bytes, per channel
//! and per direction. The receiver grants each channel a window and returns
//! credit as its consumer takes data, a quarter window at a time, or at once
//! when a request ends. A bulk channel's window is sized from the link
//! instead: [`bulk_window`] at each heartbeat.

use crate::CHUNK;
use std::collections::VecDeque;
use std::time::Duration;

/// The credit each side grants each channel to begin with: 1.25 MiB, so a
/// 1 MiB response with its chunks' framing takes one round trip.
pub const INITIAL_WINDOW: u32 = 1280 * 1024;
/// The smallest window either peer accepts, four chunks, so that a quarter
/// window's grant always covers a chunk.
pub const MIN_WINDOW: u32 = 4 * CHUNK as u32;
/// The most a bulk channel's window grows to.
pub const MAX_BULK_WINDOW: u32 = 4 << 20;
/// The largest `Data` body the mux sends. Senders chunk at [`CHUNK`]; this
/// leaves room for a chunk's own framing (an enum tag, a length, an offset)
/// and keeps any `Data` frame within three quarters of [`MIN_WINDOW`], the
/// credit a sender always gets back once the consumer catches up.
pub const MAX_DATA: usize = CHUNK + 1024;
/// The heartbeat intervals a bulk channel's delivery rate is the most of.
pub const RATE_SAMPLES: usize = 3;

/// A bulk channel's window: twice the bytes the link delivers in its
/// shortest round trip, clamped to [`MIN_WINDOW`]..=[`MAX_BULK_WINDOW`].
pub fn bulk_window(bytes_per_sec: f64, min_rtt: Duration) -> u32 {
    let w = 2.0 * bytes_per_sec * min_rtt.as_secs_f64();
    if w.is_nan() {
        return MIN_WINDOW;
    }
    w.clamp(f64::from(MIN_WINDOW), f64::from(MAX_BULK_WINDOW)) as u32
}

/// A bulk channel's delivery rate: the most bytes per second it received in
/// any of the last [`RATE_SAMPLES`] heartbeat intervals that carried data.
/// The most rather than the latest, so an interval the sender spent
/// waiting on credit or on disk doesn't shrink the window it needs.
#[derive(Clone, Debug, Default)]
pub struct DeliveryRate {
    samples: VecDeque<f64>,
}

impl DeliveryRate {
    /// Records `bytes` received over `over`. An interval with no data is no
    /// sample: an idle channel keeps its window. `None` until there is one.
    pub fn sample(&mut self, bytes: u64, over: Duration) -> Option<f64> {
        if bytes > 0 && !over.is_zero() {
            if self.samples.len() == RATE_SAMPLES {
                self.samples.pop_front();
            }
            self.samples.push_back(bytes as f64 / over.as_secs_f64());
        }
        self.rate()
    }

    pub fn rate(&self) -> Option<f64> {
        self.samples.iter().copied().reduce(f64::max)
    }
}

/// The receiving side of one channel's credit.
///
/// `circulating` is the credit given out: what the peer holds, plus what has
/// arrived and not been consumed, plus what has been consumed and not
/// returned yet (`owed`). It equals the window except while a resize
/// settles: a larger window is granted at once, and a smaller one is reached
/// by keeping back owed credit instead of returning it.
#[derive(Debug)]
pub(crate) struct RecvWindow {
    window: u32,
    circulating: u64,
    /// The credit the peer holds. A `Data` frame larger is an overrun.
    peer_holds: u64,
    owed: u64,
    /// Bytes received since the last heartbeat, for the delivery rate.
    pub(crate) arrived: u64,
    pub(crate) bulk: Option<DeliveryRate>,
}

impl RecvWindow {
    pub(crate) fn new(window: u32) -> RecvWindow {
        RecvWindow {
            window,
            circulating: u64::from(window),
            peer_holds: u64::from(window),
            owed: 0,
            arrived: 0,
            bulk: None,
        }
    }

    pub(crate) fn window(&self) -> u32 {
        self.window
    }

    /// A `Data` body of `n` bytes arrived. False if the peer had no credit
    /// for it.
    pub(crate) fn arrive(&mut self, n: usize) -> bool {
        let n = n as u64;
        if n > self.peer_holds {
            return false;
        }
        self.peer_holds -= n;
        self.arrived += n;
        true
    }

    /// The consumer took `n` bytes. Returns the grant to send, if one is
    /// due: a quarter window's worth, or everything owed with `flush` (a
    /// request ended).
    pub(crate) fn consume(&mut self, n: usize, flush: bool) -> Option<u32> {
        self.owed += n as u64;
        self.settle(flush)
    }

    /// Sets a new window. Growth is granted at once; a shrink is settled
    /// as the consumer returns credit.
    pub(crate) fn resize(&mut self, window: u32) -> Option<u32> {
        self.window = window;
        let grow = u64::from(window).saturating_sub(self.circulating);
        self.owed += grow;
        self.circulating += grow;
        self.settle(grow > 0)
    }

    fn settle(&mut self, flush: bool) -> Option<u32> {
        let excess = self.circulating.saturating_sub(u64::from(self.window));
        let keep_back = excess.min(self.owed);
        self.owed -= keep_back;
        self.circulating -= keep_back;
        if self.owed == 0 || !(flush || self.owed >= u64::from(self.window / 4)) {
            return None;
        }
        let grant = self.owed.min(u64::from(u32::MAX));
        self.owed -= grant;
        self.peer_holds += grant;
        Some(grant as u32)
    }
}
