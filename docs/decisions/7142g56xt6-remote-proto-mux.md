# fe-remote-proto: the multiplexer (T395, remote 2b)

The choices behind `crates/fe-remote-proto/src/mux` and its neighbours that
[`j8wvgszah0-remote-development.md`](j8wvgszah0-remote-development.md) ("D")
§ 2.2, § 2.4 and § 2.5 left open. The frames and bodies are in
[`r9cn6a3117-remote-proto-frames.md`](r9cn6a3117-remote-proto-frames.md).

## Runtime shape

- **Three threads per link, none per request.** A reader routes frames, a
  writer drains the send queue, and a timer runs the heartbeat. All three
  and every handle share one `Mutex<State>`, so the routing, credit and
  queue rules are read in one place with no lock order to keep.
- **Handles block or poll.** `Stream::recv`/`send`, `Mux::accept` and
  `Mux::wait_lost` block on condvars. Each also has a `poll_*` or `async`
  form built on `std::task::Waker`, with no runtime and no new crate. The
  server (no runtime) uses the blocking forms on its own threads. The GPUI
  client awaits the async forms on its executor without tying up a pool
  thread per request. Wakers are woken after the lock is released, so no
  task is polled under it.
- **The clock is injected.** `Clock::System`, or `Clock::Manual` that only
  `advance` moves. The timer waits on an alarm that the manual clock rings
  when it moves, so heartbeat tests move time instead of sleeping.

## The link

- `Link { reader, writer, child }` is D § 1's seam. `Link::spawn` gives a
  child's stdio (stderr is left as the caller sets it), and `Link::pipe`
  gives two in-process ends over OS pipes. The `RemoteTransport` trait and
  ssh come with phase 4.
- The child is reaped once the link ends. It is killed first whenever the
  link is lost, unless this side hung up and the child is exiting anyway.
  A dead network path can hold ssh's stdout open until its own keepalive
  gives up, and a child can close its stdin and keep its stdout, so the
  reader would otherwise wait on it for good (review 2w5cvv F5).
- **A failed start is undone.** If a thread can't be started, the link is
  lost, the threads already running finish, and the child is killed and
  reaped (F7). `testing::fail_mux_threads_after` injects the failure.

## Requests and streams

- **One handle type, `Stream`,** for a request this side made, a request
  the peer made (`Incoming.stream`), and a dynamic channel's stream. They
  share the inbox, the credit wait and cancel. `end` on one's own request
  is `MuxError::OwnRequest`, because D § 2.2 has the requester never end it.
- **Requests go on fixed channels, streams on dynamic ones,** as T394's
  frame checks already require. A stream is id 0 on its channel. Each end
  claims it with `Mux::stream(chan)`. What the peer sends before the claim
  waits in the route, held to the channel's window, so a server can claim
  and name a channel in the same breath as it starts sending. A `Cancel`
  before the claim waits there too.
- **A channel carries one stream for the life of the link** (F3). Claiming
  it again is `MuxError::Spent`, and a frame on it after its stream ended
  is a protocol error. Reusing a channel would let a frame of the old
  stream, or the new one's first frame meeting the old one's handle still
  held on the other end, land in the wrong stream. Whoever allocates
  channels (`Open`, phase 3's service) hands out fresh ones; a link has
  65,472.
- **An empty `Data` body on a stream is nothing.** Sending one is a no-op,
  and one received is dropped: it costs no credit, so queueing it would let
  an unread stream grow without bound (F8).
- **Ids.** The client numbers its requests from 1 and the server sets the
  high bit (D § 2.2). A request with the wrong side's bit, or an id that is
  already open, is a protocol error and loses the link. The mux checks its
  own request bodies against the channel before sending them, since the
  peer would drop the link over one that doesn't fit.
- **Cancel.** `cancel` sends `Cancel` and keeps the handle receiving until
  the `End(Cancelled)`. A `Cancel` is urgent and a `Request` isn't, so a
  cancel of a request still in the queue would reach the peer first and be
  ignored. Instead the queued `Request` is taken back and the request ends
  at once with `Cancelled`; the peer never sees either (F1). On the answering side, a cancel stops `send` with
  `MuxError::Cancelled` and drops that request's queued, unsent data,
  refunding its credit. The handler then ends with `Cancelled`. The mux
  doesn't end the request for it, because only the handler knows whether
  its work (a write's commit) already happened.
- **Dropping a handle** cancels one's own unfinished request, ends an
  unanswered peer request (`Cancelled` if it was cancelled, else an
  error), and ends a stream's direction with `Ok` and cancels the other.
  What was waiting in it is credited back.
- **A lost link** ends one's own requests and streams with
  `End(LinkLost)`. Peer requests stop, `send` fails with
  `MuxError::LinkLost(why)`, and `accept` returns `None`. The loss is
  terminal whichever thread found it: the reader routes nothing more, and
  requests not yet accepted are dropped unanswered, outside the lock
  (F4).
- **Anything that stops sending wakes the senders** waiting for credit: the
  peer's `End`, this side's `end`, a cancel, a hang-up and a loss (F2).
- **`end` encodes its status first,** so one too long to send is an error
  and leaves the stream open (F6). `Lost` says why:
  `Eof`, `MissedPongs`, `Io`, `Protocol`, or `Closed` when this side hung
  up.

## Credits

- **Credit is taken when a `Data` frame is queued,** not when it's written,
  so the queue plus the wire never hold more than the window. That's what
  bounds the data after a cancel. Only `Data` bodies count (D § 2.4).
- **The receiver returns credit** in grants of at least a quarter window as
  the consumer takes data, and everything owed when a request's `End` is
  taken or its handle dropped. Data for nobody (a dropped handle, a late
  upload) is credited back as it lands.
- **An overrun is a protocol error.** The receiver tracks what the peer
  holds, so a `Data` frame past it loses the link.
- **`MAX_DATA` is `CHUNK` + 1 KiB.** That leaves room for a chunk's own
  framing. It also keeps any frame under three quarters of the smallest
  window, which is the credit a sender always gets back once its consumer
  catches up. Otherwise a body larger than a 256 KiB window could never be
  sent.
- **Both windows are checked.** The server refuses a `Hello` window under
  `MIN_WINDOW` (four chunks) with the reason. The client refuses a
  `Welcome` window under it and won't offer one itself.

## The send queue

- Urgent frames go first, then the channels round robin one frame each
  (the spike's `Outbox`). Urgent means the handshake (sent before the
  threads start), pongs and cancels, as D lists, and also **pings and
  credit grants**. A ping queued behind this side's bulk would measure our
  queue, not the link. A grant queued behind it would stall the peer's
  stream on another channel for as long.
- **The writer's buffer is 16 KiB,** smaller than a chunk. Small frames
  coalesce, and a chunk goes straight to the link. The writer therefore
  holds at most the frame it's writing ahead of an urgent one. The spike's
  256 KiB `BufWriter` could hold four chunks ahead of a pong.

## Heartbeat

- **Both peers ping.** D § 2.5 has the client ping. The server pings too,
  on the same 10 s schedule and with the high bit in its nonces. It needs
  its own round trip to size the window of a bulk upload it receives, and
  its own lost-link detection for the session grace of D § 3. Each peer's
  pong answers only the other's ping.
- **The first ping goes when the link opens,** so the pill shows a round
  trip at once, and the bulk window has a minimum RTT taken on an idle
  link. After that one ping goes every interval.
- **Lost:** each tick that finds the last ping unanswered counts a miss.
  At two misses in a row the link is lost: 20 s after the first unanswered
  ping. Any waited-for pong, late or not, resets the count. A timer that
  wakes late (a suspended laptop) sends one ping, not one per interval it
  missed.
- `Mux::rtt()` gives the latest round trip, for the tooltip, and the
  shortest since the link opened, for the bulk window. The latest is
  inflated by bulk queued ahead of the pong on the wire, which a capped
  link shows plainly.

## The bulk window

- **The receiver sizes it,** since it grants the credit: `Mux::set_bulk`
  marks a channel bulk on this side. At each heartbeat the window becomes
  `bulk_window(rate, min RTT)` = `2 × rate × min RTT`, clamped to
  256 KiB–4 MiB. Growth is granted at once. A shrink is reached by keeping
  back credit as it comes due rather than returning it.
- **The rate** is the most bytes per second received in any of the last
  three heartbeat intervals that carried data. An idle interval is no
  sample, so an idle channel keeps its window. It uses the most rather than
  the latest, so an interval the sender spent on disk doesn't shrink the
  window it needs. A window-limited stream delivers about a window per
  round trip, so the estimate doubles each heartbeat until the link's rate
  limits it, and settles at twice the bandwidth-delay product.
- **Validated on the capped `Delayed` link** (D § 14), in a debug build at
  load ~7, three runs. On an 8 MiB/s cap with 20 ms one way, a bulk stream
  that opened at 1.25 MiB settled at 0.70–0.71 MiB. Twice the cap times the
  measured minimum RTT (42.9–43.7 ms) is 0.69–0.70 MiB. Delivery held at
  8.00–8.03 MiB/s, so twice the BDP keeps the link full. With no cap and
  50 ms one way, the window grows from 1.25 MiB to the 4 MiB ceiling within
  a few heartbeats. `tests/mux_bulk_window.rs` checks both with wide
  bounds.

## Test support

- Feature `test-support` gives `fe_remote_proto::testing`: `delayed`
  (the spike's `Delayed`: latency each way, an optional cap, spinning to
  its deadlines rather than sleeping), `RawPeer` (a frame-level peer for
  fakes and for seeing exactly what a mux sends), and `serve` (a small test
  service). It also gives the `fe-remote-proto-peer` binary, which serves
  that service on its stdio for the child-stdio tests. The crate
  dev-depends on itself with the feature, as fe-dap does for its fake
  adapter.
- The scheduler tests read the server's frames from a raw client over a
  pipe smaller than a chunk, so the server's writer is stuck inside its
  first chunk. They get the urgent frame queued before reading by sending
  a request after the ping and waiting for the handler to see it, since
  the server handles frames in order. The reply rides on channel 2, above
  the bulk stream's channel 1, so a scheduler that favoured low channels
  fails the test too.

## Known limits

- The minimum RTT never forgets. A path whose round trip grows (a roaming
  laptop) keeps the window it had. A windowed minimum would need idle
  samples, which a long bulk stream doesn't give.
- On a link slower than about 64 KiB/s, a full 1.25 MiB initial window
  queued ahead of a pong takes more than 20 s to drain, and the heartbeat
  would call the link lost. Bulk channels shrink to fit. Others don't.
- The handshake has no timeout of its own. ssh's `ConnectTimeout` bounds
  the connect, but not a server that never answers.
- `Mux::close` waits for the peer to hang up in turn. Dropping the `Mux`
  instead lets the threads finish on their own. A writer blocked on a peer
  that has stopped reading stays blocked until the link breaks.
