//! The mux's shared state, under one lock: the send queue, both sides'
//! credit, the routes from `(channel, id)` to a [`Stream`], and the link's
//! fate. The reader, writer and timer threads and every handle go through
//! [`Shared::lock`].

use super::{Event, Incoming, Lost, MuxError, Stream};
use crate::clock::{Alarm, Clock};
use crate::frame::{self, check_body};
use crate::heartbeat::{Beat, Heartbeat};
use crate::window::{RecvWindow, bulk_window};
use crate::{Credit, Header, Kind, SERVER_ORIGINATED, Status, chan};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ops::{Deref, DerefMut};
use std::process::Child;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Waker};
use std::time::Instant;

pub(super) type Key = (u16, u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Client,
    Server,
}

impl Role {
    /// The id bit on requests and pings this side starts.
    pub(super) fn origin(self) -> u32 {
        match self {
            Role::Client => 0,
            Role::Server => SERVER_ORIGINATED,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RouteKind {
    /// A request this side made.
    Call,
    /// A request the peer made, which this side answers.
    Reply,
    /// A dynamic channel's byte stream (id 0), both ways.
    Stream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Holder {
    /// A stream the peer started sending on before this side claimed it.
    Unclaimed,
    Live,
    /// The handle is gone: what arrives is credited back at once, and the
    /// route goes when both directions are done.
    Dropped,
}

pub(super) struct Route {
    pub(super) kind: RouteKind,
    pub(super) holder: Holder,
    pub(super) inbox: VecDeque<Event>,
    pub(super) cv: Arc<Condvar>,
    pub(super) waker: Option<Waker>,
    /// Nothing more arrives: the peer's End is in the inbox, the peer
    /// cancelled its request, this side ended its reply, or the link is
    /// lost.
    pub(super) closed_in: bool,
    /// This side sent its End. A call starts closed: the requester never
    /// ends its own request.
    pub(super) closed_out: bool,
    /// This side must stop sending: the peer cancelled, or this side
    /// cancelled its own call.
    pub(super) stopped: bool,
    pub(super) cancel_sent: bool,
}

impl Route {
    fn new(kind: RouteKind, holder: Holder) -> Route {
        Route {
            kind,
            holder,
            inbox: VecDeque::new(),
            cv: Arc::new(Condvar::new()),
            waker: None,
            closed_in: false,
            closed_out: kind == RouteKind::Call,
            stopped: false,
            cancel_sent: false,
        }
    }

    pub(super) fn wake(&mut self, wakes: &mut Vec<Waker>) {
        self.cv.notify_all();
        wakes.extend(self.waker.take());
    }

    /// Whether this side may still send `Data` on it.
    pub(super) fn sendable(&self) -> Result<(), MuxError> {
        if self.stopped {
            return Err(MuxError::Cancelled);
        }
        let ended = match self.kind {
            RouteKind::Call => self.closed_in,
            RouteKind::Reply | RouteKind::Stream => self.closed_out,
        };
        if ended {
            return Err(MuxError::Ended);
        }
        Ok(())
    }

    fn done(&self) -> bool {
        self.holder == Holder::Dropped && self.closed_in && self.closed_out
    }
}

/// A frame waiting for the writer.
pub(super) struct Out {
    pub(super) kind: Kind,
    pub(super) id: u32,
    /// The credit a `Data` frame took, refunded if it's purged unsent.
    pub(super) credit: u64,
    pub(super) bytes: Vec<u8>,
}

/// The send queue (D § 2.4): urgent frames first, then the channels round
/// robin, one frame each, so a bulk stream holds another channel back by
/// at most the one frame being written.
#[derive(Default)]
pub(super) struct Outq {
    urgent: VecDeque<Vec<u8>>,
    chans: BTreeMap<u16, VecDeque<Out>>,
    last: u16,
}

impl Outq {
    pub(super) fn pop(&mut self) -> Option<Vec<u8>> {
        if let Some(f) = self.urgent.pop_front() {
            return Some(f);
        }
        let after = self.last;
        let chan = *self
            .chans
            .range(after.wrapping_add(1)..)
            .chain(self.chans.range(..=after))
            .next()?
            .0;
        self.last = chan;
        let q = self.chans.get_mut(&chan)?;
        let f = q.pop_front();
        if q.is_empty() {
            self.chans.remove(&chan);
        }
        f.map(|o| o.bytes)
    }

    /// Drops the queued `Data` frames of `(chan, id)` and returns the
    /// credit they took. Its End, if queued, stays.
    fn purge(&mut self, (chan, id): Key) -> u64 {
        let mut refund = 0;
        self.retain(chan, |o| {
            let drop = o.id == id && o.kind == Kind::Data;
            if drop {
                refund += o.credit;
            }
            !drop
        });
        refund
    }

    /// Takes back the `Request` of `(chan, id)` if the writer hasn't taken
    /// it yet. True if it was still queued: the peer will never see it.
    fn withdraw_request(&mut self, (chan, id): Key) -> bool {
        let mut found = false;
        self.retain(chan, |o| {
            let drop = o.id == id && o.kind == Kind::Request;
            found |= drop;
            !drop
        });
        found
    }

    fn retain(&mut self, chan: u16, keep: impl FnMut(&Out) -> bool) {
        let Some(q) = self.chans.get_mut(&chan) else {
            return;
        };
        q.retain(keep);
        if q.is_empty() {
            self.chans.remove(&chan);
        }
    }

    fn clear(&mut self) {
        self.urgent.clear();
        self.chans.clear();
    }
}

/// This side's credit to send on one channel, and who waits for more.
pub(super) struct SendChan {
    pub(super) credit: u64,
    pub(super) cv: Arc<Condvar>,
    pub(super) wakers: Vec<Waker>,
}

pub(super) struct State {
    pub(super) out: Outq,
    pub(super) send: HashMap<u16, SendChan>,
    pub(super) recv: HashMap<u16, RecvWindow>,
    pub(super) routes: HashMap<Key, Route>,
    pub(super) accept: VecDeque<Incoming>,
    pub(super) accept_waker: Option<Waker>,
    /// The dynamic channels that have carried a stream. Each stream takes
    /// a fresh channel for the life of the link, so a late frame can't be
    /// taken for the next stream's.
    pub(super) spent: HashSet<u16>,
    pub(super) next_id: u32,
    pub(super) heartbeat: Option<Heartbeat>,
    pub(super) last_beat: Instant,
    pub(super) lost: Option<Lost>,
    pub(super) lost_wakers: Vec<Waker>,
    /// `close` was called: the writer drains the queue and hangs up.
    pub(super) closing: bool,
    pub(super) child: Option<Child>,
    /// Woken once the lock is released, so no task runs under it.
    pub(super) wakes: Vec<Waker>,
    /// Requests nobody will accept now the link is lost, dropped once the
    /// lock is released: their streams take it as they go.
    pub(super) dead: Vec<Incoming>,
}

pub(super) struct Shared {
    state: Mutex<State>,
    pub(super) writer_cv: Condvar,
    pub(super) accept_cv: Condvar,
    pub(super) lost_cv: Condvar,
    pub(super) alarm: Arc<Alarm>,
    pub(super) clock: Clock,
    pub(super) role: Role,
    /// The credit this side grants each channel.
    pub(super) window: u32,
    /// The credit the peer grants each channel.
    pub(super) peer_window: u32,
}

/// The state's lock. Wakers queued while it's held are woken after it's
/// released.
pub(super) struct Locked<'a> {
    shared: &'a Shared,
    guard: Option<MutexGuard<'a, State>>,
}

impl Deref for Locked<'_> {
    type Target = State;
    fn deref(&self) -> &State {
        self.guard.as_ref().unwrap()
    }
}

impl DerefMut for Locked<'_> {
    fn deref_mut(&mut self) -> &mut State {
        self.guard.as_mut().unwrap()
    }
}

impl Locked<'_> {
    /// Waits on `cv`. With wakers queued it wakes them instead and returns
    /// at once, so the caller looks again before it sleeps.
    pub(super) fn wait(&mut self, cv: &Condvar) {
        let wakes = std::mem::take(&mut self.wakes);
        let guard = self.guard.take().unwrap();
        if wakes.is_empty() {
            self.guard = Some(cv.wait(guard).unwrap_or_else(PoisonError::into_inner));
        } else {
            drop(guard);
            wakes.into_iter().for_each(Waker::wake);
            self.guard = Some(self.shared.raw_lock());
        }
    }
}

impl Drop for Locked<'_> {
    fn drop(&mut self) {
        let Some(mut guard) = self.guard.take() else {
            return;
        };
        let wakes = std::mem::take(&mut guard.wakes);
        let dead = std::mem::take(&mut guard.dead);
        drop(guard);
        wakes.into_iter().for_each(Waker::wake);
        drop(dead);
    }
}

/// An `End` frame, encoded before a route changes so a status that
/// can't be sent leaves it as it was.
pub(super) fn encode_end((chan, id): Key, status: &Status) -> Result<Vec<u8>, MuxError> {
    Ok(frame::encode(Kind::End, chan, id, status)?)
}

/// Adds `cx`'s waker to `wakers`, once however often a task polls.
pub(super) fn register(wakers: &mut Vec<Waker>, cx: &Context<'_>) {
    if !wakers.iter().any(|w| w.will_wake(cx.waker())) {
        wakers.push(cx.waker().clone());
    }
}

fn protocol(why: impl Into<String>) -> Lost {
    Lost::Protocol(why.into())
}

impl Shared {
    pub(super) fn new(
        role: Role,
        clock: Clock,
        window: u32,
        peer_window: u32,
        heartbeat: Option<Heartbeat>,
        child: Option<Child>,
    ) -> Shared {
        let now = clock.now();
        Shared {
            state: Mutex::new(State {
                out: Outq::default(),
                send: HashMap::new(),
                recv: HashMap::new(),
                routes: HashMap::new(),
                accept: VecDeque::new(),
                accept_waker: None,
                spent: HashSet::new(),
                next_id: 1,
                heartbeat,
                last_beat: now,
                lost: None,
                lost_wakers: Vec::new(),
                closing: false,
                child,
                wakes: Vec::new(),
                dead: Vec::new(),
            }),
            writer_cv: Condvar::new(),
            accept_cv: Condvar::new(),
            lost_cv: Condvar::new(),
            alarm: Alarm::new(&clock),
            clock,
            role,
            window,
            peer_window,
        }
    }

    fn raw_lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn lock(&self) -> Locked<'_> {
        Locked {
            shared: self,
            guard: Some(self.raw_lock()),
        }
    }

    /// Why nothing more can be sent, if so.
    pub(super) fn gone(&self, st: &State) -> Result<(), MuxError> {
        match &st.lost {
            Some(lost) => Err(MuxError::LinkLost(lost.clone())),
            None if st.closing => Err(MuxError::LinkLost(Lost::Closed)),
            None => Ok(()),
        }
    }

    pub(super) fn send_chan<'s>(&self, st: &'s mut State, chan: u16) -> &'s mut SendChan {
        st.send.entry(chan).or_insert_with(|| SendChan {
            credit: u64::from(self.peer_window),
            cv: Arc::new(Condvar::new()),
            wakers: Vec::new(),
        })
    }

    pub(super) fn recv_window<'s>(&self, st: &'s mut State, chan: u16) -> &'s mut RecvWindow {
        st.recv
            .entry(chan)
            .or_insert_with(|| RecvWindow::new(self.window))
    }

    pub(super) fn urgent(&self, st: &mut State, frame: Vec<u8>) {
        st.out.urgent.push_back(frame);
        self.writer_cv.notify_one();
    }

    pub(super) fn push(&self, st: &mut State, chan: u16, out: Out) {
        st.out.chans.entry(chan).or_default().push_back(out);
        self.writer_cv.notify_one();
    }

    /// Sends a control frame with an empty body (Cancel, Ping, Pong).
    pub(super) fn urgent_empty(&self, st: &mut State, kind: Kind, chan: u16, id: u32) {
        let frame = frame::encode(kind, chan, id, &()).expect("an empty body encodes");
        self.urgent(st, frame);
    }

    /// Queues an `End` encoded by [`encode_end`] behind the route's data.
    pub(super) fn end_frame(&self, st: &mut State, (chan, id): Key, bytes: Vec<u8>) {
        let out = Out {
            kind: Kind::End,
            id,
            credit: 0,
            bytes,
        };
        self.push(st, chan, out);
        // Sending on it now fails: whoever waits for credit should see so.
        self.wake_senders(st, chan);
    }

    /// Wakes every sender on every channel: sending has stopped.
    pub(super) fn wake_all_senders(&self, st: &mut State) {
        for sc in st.send.values_mut() {
            sc.cv.notify_all();
            st.wakes.append(&mut sc.wakers);
        }
    }

    /// Wakes everyone waiting for credit on `chan`.
    pub(super) fn wake_senders(&self, st: &mut State, chan: u16) {
        let sc = self.send_chan(st, chan);
        sc.cv.notify_all();
        let wakers = std::mem::take(&mut sc.wakers);
        st.wakes.extend(wakers);
    }

    fn add_credit(&self, st: &mut State, chan: u16, bytes: u64) {
        let sc = self.send_chan(st, chan);
        sc.credit = sc.credit.saturating_add(bytes);
        self.wake_senders(st, chan);
    }

    /// The consumer of `chan` took `n` bytes: returns credit when a grant
    /// is due, or everything owed with `flush`.
    pub(super) fn consumed(&self, st: &mut State, chan: u16, n: usize, flush: bool) {
        if let Some(grant) = self.recv_window(st, chan).consume(n, flush) {
            self.credit_frame(st, chan, grant);
        }
    }

    fn credit_frame(&self, st: &mut State, chan: u16, bytes: u32) {
        // Urgent: a grant held behind this side's own bulk would stall the
        // peer's stream for as long.
        let frame =
            frame::encode(Kind::Credit, chan, 0, &Credit { bytes }).expect("a credit encodes");
        self.urgent(st, frame);
    }

    pub(super) fn open_route(&self, st: &mut State, key: Key, kind: RouteKind, holder: Holder) {
        if kind == RouteKind::Stream {
            st.spent.insert(key.0);
        }
        st.routes.insert(key, Route::new(kind, holder));
    }

    pub(super) fn retire_if_done(&self, st: &mut State, key: Key) {
        if st.routes.get(&key).is_some_and(Route::done) {
            st.routes.remove(&key);
        }
    }

    /// Cancels this side's own request `key`. If its `Request` is still
    /// queued it is taken back and the call ends here with `Cancelled`, as
    /// a `Cancel` sent ahead of it would reach a peer that doesn't know the
    /// request yet. Otherwise the `Cancel` goes after it. Either way its
    /// uploads stop.
    pub(super) fn cancel_call(&self, st: &mut State, key: Key) {
        let withdrawn = st.out.withdraw_request(key);
        let Some(r) = st.routes.get_mut(&key) else {
            return;
        };
        r.cancel_sent = true;
        r.stopped = true;
        if withdrawn {
            r.closed_in = true;
            if r.holder != Holder::Dropped {
                r.inbox.push_back(Event::End(Status::Cancelled));
            }
            r.wake(&mut st.wakes);
        } else {
            self.urgent_empty(st, Kind::Cancel, key.0, key.1);
        }
        self.purge(st, key);
        self.retire_if_done(st, key);
    }

    /// Stops this side's queued and future `Data` on `key`, refunding the
    /// credit its unsent frames took.
    pub(super) fn purge(&self, st: &mut State, key: Key) {
        let refund = st.out.purge(key);
        if refund > 0 {
            self.add_credit(st, key.0, refund);
        } else {
            self.wake_senders(st, key.0);
        }
    }

    /// Routes one frame from the peer. An error loses the link.
    pub(super) fn on_frame(self: &Arc<Shared>, h: Header, body: Vec<u8>) -> Result<(), Lost> {
        check_body(&h, &body).map_err(|e| protocol(e.to_string()))?;
        let mut guard = self.lock();
        let st = &mut *guard;
        if let Some(lost) = &st.lost {
            // The timer or the writer lost the link: route nothing more.
            return Err(lost.clone());
        }
        let key = (h.chan, h.id);
        let dynamic = h.chan >= chan::FIRST_DYNAMIC;
        if dynamic && matches!(h.kind, Kind::Data | Kind::End | Kind::Cancel) {
            if h.id != 0 {
                return Err(protocol(format!(
                    "{:?} on stream channel {} with id {:#x}, not 0",
                    h.kind, h.chan, h.id
                )));
            }
            if h.kind == Kind::Data && h.body_len == 0 {
                // Carries nothing and costs no credit: queueing it would
                // let an unread stream grow without bound.
                return Ok(());
            }
            if !st.routes.contains_key(&key) {
                if st.spent.contains(&h.chan) {
                    if h.kind == Kind::Cancel {
                        return Ok(());
                    }
                    return Err(protocol(format!(
                        "{:?} on stream channel {}, whose stream has ended",
                        h.kind, h.chan
                    )));
                }
                // The peer started this stream before this side claimed it.
                self.open_route(st, key, RouteKind::Stream, Holder::Unclaimed);
            }
        }
        match h.kind {
            Kind::Hello | Kind::Welcome | Kind::Refused => {
                return Err(protocol(format!("{:?} after the handshake", h.kind)));
            }
            Kind::Ping => self.urgent_empty(st, Kind::Pong, chan::CONTROL, h.id),
            Kind::Pong => {
                let now = self.clock.now();
                if let Some(hb) = st.heartbeat.as_mut() {
                    hb.pong(h.id, now);
                }
            }
            Kind::Credit => {
                let c: Credit = frame::body(&h, &body).map_err(|e| protocol(e.to_string()))?;
                self.add_credit(st, h.chan, u64::from(c.bytes));
            }
            Kind::Request => {
                let theirs = match self.role {
                    Role::Client => SERVER_ORIGINATED,
                    Role::Server => 0,
                };
                if h.id & SERVER_ORIGINATED != theirs {
                    return Err(protocol(format!(
                        "request id {:#x} on channel {} has the other side's origin bit",
                        h.id, h.chan
                    )));
                }
                if st.routes.contains_key(&key) {
                    return Err(protocol(format!(
                        "request id {:#x} on channel {} is already open",
                        h.id, h.chan
                    )));
                }
                self.open_route(st, key, RouteKind::Reply, Holder::Live);
                let stream = Stream::new(self.clone(), key, &st.routes[&key]);
                st.accept.push_back(Incoming {
                    header: h,
                    op: body,
                    stream,
                });
                self.accept_cv.notify_one();
                st.wakes.extend(st.accept_waker.take());
            }
            Kind::Data => {
                let n = h.body_len;
                if !self.recv_window(st, h.chan).arrive(n) {
                    return Err(protocol(format!(
                        "{n} data bytes on channel {} past the credit granted",
                        h.chan
                    )));
                }
                match st.routes.get_mut(&key) {
                    Some(r) if r.kind == RouteKind::Stream && r.closed_in => {
                        return Err(protocol(format!(
                            "data on stream channel {} after its End",
                            h.chan
                        )));
                    }
                    Some(r) if r.holder != Holder::Dropped && !r.closed_in => {
                        r.inbox.push_back(Event::Data(body));
                        r.wake(&mut st.wakes);
                    }
                    // Late (an upload to a reply that has ended), or nobody
                    // is listening: the credit goes back.
                    _ => self.consumed(st, h.chan, n, false),
                }
            }
            Kind::End => {
                let status: Status = frame::body(&h, &body).map_err(|e| protocol(e.to_string()))?;
                match st.routes.get_mut(&key) {
                    Some(r) if r.kind != RouteKind::Reply && !r.closed_in => {
                        r.closed_in = true;
                        if r.holder == Holder::Dropped {
                            self.consumed(st, h.chan, 0, true);
                            self.retire_if_done(st, key);
                        } else {
                            r.inbox.push_back(Event::End(status));
                            r.wake(&mut st.wakes);
                        }
                        // A request's uploads stop at its End.
                        self.wake_senders(st, h.chan);
                    }
                    // The peer never ends its own request, and nothing
                    // follows an End.
                    _ => {}
                }
            }
            Kind::Cancel => match st.routes.get_mut(&key) {
                Some(r) if r.kind != RouteKind::Call => {
                    r.stopped = true;
                    if r.kind == RouteKind::Reply {
                        // The peer sends no more of its request.
                        r.closed_in = true;
                    }
                    r.wake(&mut st.wakes);
                    self.purge(st, key);
                }
                // Already ended, or a cancel of this side's own request.
                _ => {}
            },
        }
        Ok(())
    }

    /// The heartbeat's tick: a ping, the bulk windows resized, or a lost
    /// link.
    pub(super) fn beat(&self, st: &mut State) {
        let now = self.clock.now();
        let Some(hb) = st.heartbeat.as_mut() else {
            return;
        };
        match hb.tick(now) {
            None => {}
            Some(Beat::Lost) => self.lose(st, Lost::MissedPongs),
            Some(Beat::Ping(nonce)) => {
                let min_rtt = hb.rtt().map(|r| r.min);
                self.urgent_empty(st, Kind::Ping, chan::CONTROL, nonce);
                let over = now.saturating_duration_since(st.last_beat);
                st.last_beat = now;
                let mut grants = Vec::new();
                for (&chan, rw) in &mut st.recv {
                    let arrived = std::mem::take(&mut rw.arrived);
                    let Some(rate) = rw.bulk.as_mut().and_then(|b| b.sample(arrived, over)) else {
                        continue;
                    };
                    let Some(min_rtt) = min_rtt else { continue };
                    if let Some(g) = rw.resize(bulk_window(rate, min_rtt)) {
                        grants.push((chan, g));
                    }
                }
                for (chan, g) in grants {
                    self.credit_frame(st, chan, g);
                }
            }
        }
    }

    /// The link is gone. Every open call and stream ends with `LinkLost`,
    /// every reply stops, requests not yet accepted are dropped, the child
    /// is killed unless this side hung up, and everyone waiting wakes. The
    /// first reason stands.
    pub(super) fn lose(&self, st: &mut State, reason: Lost) {
        if st.lost.is_some() {
            return;
        }
        if reason != Lost::Closed
            && let Some(child) = st.child.as_mut()
        {
            // Its stdout may never close on its own (a dead network path
            // holds ssh open until its own keepalive gives up, and a child
            // may close stdin and keep stdout), and the reader waits on it.
            // A child this side hung up on is exiting anyway.
            let _ = child.kill();
        }
        st.lost = Some(reason);
        st.out.clear();
        for r in st.routes.values_mut() {
            if r.kind != RouteKind::Reply && !r.closed_in && r.holder != Holder::Dropped {
                r.inbox.push_back(Event::End(Status::LinkLost));
            }
            r.closed_in = true;
            r.stopped = true;
            r.wake(&mut st.wakes);
        }
        self.wake_all_senders(st);
        let unaccepted = std::mem::take(&mut st.accept);
        st.dead.extend(unaccepted);
        self.accept_cv.notify_all();
        st.wakes.extend(st.accept_waker.take());
        self.lost_cv.notify_all();
        st.wakes.append(&mut st.lost_wakers);
        self.writer_cv.notify_all();
        self.alarm.stop();
    }
}
