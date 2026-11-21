//! [`Stream`]: one conversation on the mux, a request or a dynamic
//! channel's byte stream, with both a blocking and a polling face.

use super::state::{Holder, Key, Out, Route, RouteKind, Shared, encode_end};
use super::{Event, MuxError};
use crate::window::MAX_DATA;
use crate::{FrameError, HEADER, Header, IoError, IoKind, Kind, Status, frame};
use serde::{Deserialize, Serialize};
use std::future::poll_fn;
use std::sync::{Arc, Condvar};
use std::task::{Context, Poll};

/// One request, from either end, or one dynamic channel's stream.
///
/// - A request this side made ([`crate::Mux::request`]): `recv` gives the
///   reply's `Data` and then its `End`. `send` uploads the bytes a request
///   such as `WriteAtomic` announces. `cancel` asks the peer to stop.
///   Dropped before its `End`, it cancels.
/// - A request the peer made ([`crate::Incoming`]): `send` the reply's
///   `Data`, then `end` it. `recv` gives the peer's uploads. Once the peer
///   cancels, `send` fails with [`MuxError::Cancelled`] and the request
///   should `end` with [`Status::Cancelled`]. Dropped without an `end`, it
///   ends with `Cancelled` if cancelled, else with an error.
/// - A stream ([`crate::Mux::stream`]): both ends `send` and `recv` bytes,
///   and each `end`s its own direction. `cancel` asks the peer to stop
///   sending. Dropped, it ends its direction and cancels the other.
///
/// Every `send` waits for the channel's credit (§ 2.4); `recv` returns
/// that credit as it hands data over. After a lost link, `recv` ends with
/// [`Status::LinkLost`] and `send` fails with [`MuxError::LinkLost`].
pub struct Stream {
    shared: Arc<Shared>,
    key: Key,
    kind: RouteKind,
    cv: Arc<Condvar>,
}

impl Stream {
    pub(super) fn new(shared: Arc<Shared>, key: Key, route: &Route) -> Stream {
        Stream {
            shared,
            key,
            kind: route.kind,
            cv: route.cv.clone(),
        }
    }

    pub fn chan(&self) -> u16 {
        self.key.0
    }

    /// The request id; 0 on a stream.
    pub fn id(&self) -> u32 {
        self.key.1
    }

    /// Decodes a `Data` body from this stream's channel.
    pub fn decode<'a, T: Deserialize<'a>>(&self, bytes: &'a [u8]) -> Result<T, FrameError> {
        let header = Header {
            kind: Kind::Data,
            chan: self.key.0,
            id: self.key.1,
            body_len: bytes.len(),
        };
        frame::body(&header, bytes)
    }

    /// The next event, waiting for one. `None` once the `End` has been
    /// taken, or on a reply, once the peer has nothing more to send.
    pub fn recv(&mut self) -> Option<Event> {
        let mut st = self.shared.lock();
        loop {
            if let Poll::Ready(ev) = self.take(&mut st) {
                return ev;
            }
            st.wait(&self.cv);
        }
    }

    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        let mut st = self.shared.lock();
        let ready = self.take(&mut st);
        if ready.is_pending()
            && let Some(r) = st.routes.get_mut(&self.key)
        {
            r.waker = Some(cx.waker().clone());
        }
        ready
    }

    pub async fn recv_async(&mut self) -> Option<Event> {
        poll_fn(|cx| self.poll_recv(cx)).await
    }

    fn take(&self, st: &mut super::state::State) -> Poll<Option<Event>> {
        let Some(r) = st.routes.get_mut(&self.key) else {
            return Poll::Ready(None);
        };
        match r.inbox.pop_front() {
            Some(Event::Data(bytes)) => {
                self.shared.consumed(st, self.key.0, bytes.len(), false);
                Poll::Ready(Some(Event::Data(bytes)))
            }
            Some(Event::End(status)) => {
                // A request's end returns its channel's credit at once.
                self.shared.consumed(st, self.key.0, 0, true);
                Poll::Ready(Some(Event::End(status)))
            }
            None if r.closed_in => Poll::Ready(None),
            None => Poll::Pending,
        }
    }

    fn data_frame<T: Serialize + ?Sized>(&self, body: &T) -> Result<Vec<u8>, MuxError> {
        let frame = frame::encode(Kind::Data, self.key.0, self.key.1, body)?;
        Self::check_len(frame)
    }

    fn raw_frame(&self, bytes: &[u8]) -> Result<Vec<u8>, MuxError> {
        let frame = frame::encode_raw(Kind::Data, self.key.0, self.key.1, bytes)?;
        Self::check_len(frame)
    }

    fn check_len(frame: Vec<u8>) -> Result<Vec<u8>, MuxError> {
        let n = frame.len() - HEADER;
        if n > MAX_DATA {
            return Err(MuxError::TooLong(n));
        }
        Ok(frame)
    }

    /// Queues `frame` if the channel has the credit for it. False if it
    /// doesn't yet.
    fn push(
        &self,
        st: &mut super::state::State,
        frame: &mut Option<Vec<u8>>,
    ) -> Result<bool, MuxError> {
        self.shared.gone(st)?;
        st.routes
            .get(&self.key)
            .ok_or(MuxError::Ended)?
            .sendable()?;
        let Some(bytes) = frame.take() else {
            return Ok(true);
        };
        let n = (bytes.len() - HEADER) as u64;
        if n == 0 {
            // Nothing to carry, and the peer drops it: an empty write is
            // done.
            return Ok(true);
        }
        let sc = self.shared.send_chan(st, self.key.0);
        if sc.credit < n {
            *frame = Some(bytes);
            return Ok(false);
        }
        sc.credit -= n;
        let out = Out {
            kind: Kind::Data,
            id: self.key.1,
            credit: n,
            bytes,
        };
        self.shared.push(st, self.key.0, out);
        Ok(true)
    }

    fn send_frame(&self, frame: Vec<u8>) -> Result<(), MuxError> {
        let mut frame = Some(frame);
        let mut st = self.shared.lock();
        loop {
            if self.push(&mut st, &mut frame)? {
                return Ok(());
            }
            let cv = self.shared.send_chan(&mut st, self.key.0).cv.clone();
            st.wait(&cv);
        }
    }

    fn try_send_frame(&self, frame: Vec<u8>) -> Result<(), MuxError> {
        let mut st = self.shared.lock();
        if self.push(&mut st, &mut Some(frame))? {
            Ok(())
        } else {
            Err(MuxError::Full(self.key.0))
        }
    }

    async fn send_frame_async(&self, frame: Vec<u8>) -> Result<(), MuxError> {
        let mut frame = Some(frame);
        poll_fn(|cx| {
            let mut st = self.shared.lock();
            match self.push(&mut st, &mut frame) {
                Ok(true) => Poll::Ready(Ok(())),
                Ok(false) => {
                    let sc = self.shared.send_chan(&mut st, self.key.0);
                    super::state::register(&mut sc.wakers, cx);
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        })
        .await
    }

    /// Sends `body` as a `Data` frame, waiting for credit.
    pub fn send<T: Serialize + ?Sized>(&self, body: &T) -> Result<(), MuxError> {
        self.send_frame(self.data_frame(body)?)
    }

    /// Sends `bytes` as an opaque `Data` body, waiting for credit.
    pub fn send_bytes(&self, bytes: &[u8]) -> Result<(), MuxError> {
        self.send_frame(self.raw_frame(bytes)?)
    }

    /// Sends `body` if the channel has the credit now, else
    /// [`MuxError::Full`].
    pub fn try_send<T: Serialize + ?Sized>(&self, body: &T) -> Result<(), MuxError> {
        self.try_send_frame(self.data_frame(body)?)
    }

    pub fn try_send_bytes(&self, bytes: &[u8]) -> Result<(), MuxError> {
        self.try_send_frame(self.raw_frame(bytes)?)
    }

    pub async fn send_async<T: Serialize + ?Sized>(&self, body: &T) -> Result<(), MuxError> {
        self.send_frame_async(self.data_frame(body)?).await
    }

    pub async fn send_bytes_async(&self, bytes: &[u8]) -> Result<(), MuxError> {
        self.send_frame_async(self.raw_frame(bytes)?).await
    }

    /// Ends this side: a reply's `End`, or a stream's direction. The
    /// requester never ends its own request ([`MuxError::OwnRequest`]).
    pub fn end(&self, status: Status) -> Result<(), MuxError> {
        let bytes = encode_end(self.key, &status)?;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        self.shared.gone(st)?;
        let r = st.routes.get_mut(&self.key).ok_or(MuxError::Ended)?;
        if r.kind == RouteKind::Call {
            return Err(MuxError::OwnRequest);
        }
        if r.closed_out {
            return Err(MuxError::Ended);
        }
        r.closed_out = true;
        if r.kind == RouteKind::Reply {
            // Uploads still in flight are credited back as they land.
            r.closed_in = true;
            r.wake(&mut st.wakes);
        }
        self.shared.end_frame(st, self.key, bytes);
        Ok(())
    }

    /// Asks the peer to stop sending. A request's reply then ends with
    /// [`Status::Cancelled`]; keep receiving to see it. The data still in
    /// flight is at most the channel's window, and a request not sent yet
    /// ends here and then. On a reply this does nothing: the peer's request
    /// is the peer's to cancel.
    pub fn cancel(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if self.shared.gone(st).is_err() {
            return;
        }
        let Some(r) = st.routes.get_mut(&self.key) else {
            return;
        };
        if r.kind == RouteKind::Reply || r.closed_in || r.cancel_sent {
            return;
        }
        if r.kind == RouteKind::Call {
            self.shared.cancel_call(st, self.key);
        } else {
            r.cancel_sent = true;
            self.shared
                .urgent_empty(st, Kind::Cancel, self.key.0, self.key.1);
        }
    }

    /// The peer cancelled: stop sending, and end with `Cancelled`.
    pub fn cancelled(&self) -> bool {
        let st = self.shared.lock();
        self.kind != RouteKind::Call && st.routes.get(&self.key).is_some_and(|r| r.stopped)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let lost = st.lost.is_some();
        let Some(r) = st.routes.get_mut(&self.key) else {
            return;
        };
        r.holder = Holder::Dropped;
        let leftover: usize = r
            .inbox
            .drain(..)
            .map(|ev| match ev {
                Event::Data(bytes) => bytes.len(),
                Event::End(_) => 0,
            })
            .sum();
        let mut end = None;
        let mut cancel = false;
        let mut cancel_call = false;
        if lost {
            r.closed_out = true;
        } else {
            match r.kind {
                RouteKind::Call => cancel_call = !r.closed_in && !r.cancel_sent,
                RouteKind::Reply => {
                    if !r.closed_out {
                        end = Some(if r.stopped {
                            Status::Cancelled
                        } else {
                            Status::Error(IoError {
                                kind: IoKind::Other,
                                message: "the request was dropped without an answer".into(),
                            })
                        });
                    }
                    r.closed_in = true;
                }
                RouteKind::Stream => {
                    if !r.closed_out {
                        end = Some(Status::Ok);
                    }
                    cancel = !r.closed_in && !r.cancel_sent;
                }
            }
            if end.is_some() {
                r.closed_out = true;
            }
            if cancel {
                r.cancel_sent = true;
            }
        }
        if let Some(status) = end {
            // These statuses are short: they always encode.
            let bytes = encode_end(self.key, &status).expect("a short status encodes");
            self.shared.end_frame(st, self.key, bytes);
        }
        if cancel {
            self.shared
                .urgent_empty(st, Kind::Cancel, self.key.0, self.key.1);
        }
        if cancel_call {
            self.shared.cancel_call(st, self.key);
        }
        self.shared.consumed(st, self.key.0, leftover, true);
        self.shared.retire_if_done(st, self.key);
    }
}
