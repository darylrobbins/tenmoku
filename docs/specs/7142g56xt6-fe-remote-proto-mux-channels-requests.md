---
task_uid: "01m7xbb6182tfkjy7142g56xt6"
task: "T395"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "`a_different_build_is_refused_at_the_handshake` and `a_server_answering_a_different_build_is_refused_by_the_client` pass (tests/mux_handshake.rs, ported from tools/remote-spike/tests/spike.rs): protocol and build must both match, checked by both peers"
    required: true
    manual: false
  - id: "C2"
    text: "`a_stalled_channel_does_not_block_another_and_cancel_stops_within_its_window` passes (tests/mux_credits.rs): 1.25 MiB initial window per channel each way, quarter-window grants returned as the consumer takes data, a window under 256 KiB refused, and data after a cancel never exceeds the window"
    required: true
    manual: false
  - id: "C3"
    text: "`a_pong_is_not_queued_behind_bulk` passes: urgent frames (handshake, pong, cancel) go first, then channels round-robin one frame at a time, so a bulk stream delays another channel by at most one 64 KiB chunk"
    required: true
    manual: false
  - id: "C4"
    text: "tests/mux_heartbeat.rs: with an injected clock, a ping every 10 s and two missed pongs or EOF report the link lost; the heartbeat RTT is exposed for the pill and the bulk window"
    required: true
    manual: false
  - id: "C5"
    text: "tests/mux_bulk_window.rs: a bulk channel's window is 2 x delivery rate x min RTT clamped to 256 KiB-4 MiB and resized each heartbeat, checked on the `Delayed` latency transport with a bandwidth cap (D § 14 asks this card to validate the estimator)"
    required: true
    manual: false
  - id: "C6"
    text: "The `Delayed` injected-latency transport (one-way latency each way, optional bandwidth cap, spinning to deadlines rather than sleeping) is available to other crates' tests behind a `test-support` feature"
    required: true
    manual: false
---

# fe-remote-proto mux: channels, requests and streams, cancel, per-channel credits, the urgent-first scheduler, bulk window sizing, heartbeat and a strict handshake (remote 2b) (Mac T2041)

# fe-remote-proto mux (remote 2b)

D § 2.2 (requests = Request then Data* then End; subscriptions; cancel; server-originated ids with the high bit), § 2.4 (credits, scheduler, initial and bulk windows, the measured tables), § 2.5 (handshake, heartbeat), § 12 (the spike's numbers and notes: no short sleeps).
Port from `tools/remote-spike/src/{server,client,transport}.rs`; the spike was one thread per request, this crate must not be. It is GPUI-free and used by both the client (GPUI app) and tenmoku-server (no GPUI), so it should not depend on a particular async runtime: pick the smallest shape that serves both (blocking reader/writer threads plus channels, or futures without a runtime) and record the choice in your lane decision file.
The transport seam is a reader/writer pair (D § 1 `Link`); a child-stdio and an in-memory pipe impl are enough here. Resumable streams and session resume are T1991.

## Rules
Read the decision sections named above in `docs/decisions/j8wvgszah0-remote-development.md` (and the design file `docs/designs/remote-development.md` where cited) before starting; they are the spec, this card only cuts a slice of them. Do not widen --scope; if a file outside it is needed, stop and respec (notes and choices go in your lane's `docs/decisions/<lane>-*.md`, which is exempt). New tests go in new files, one fn per behaviour, small fixtures, no sleeps for timing (D § 12 spike notes). Gate tests on the affected crates only. Not here: anything the decision assigns to another phase (D § 13).

## Implementation notes

- Runtime shape: three threads per link (reader, writer, heartbeat timer)
  and none per request, under one state lock. Handles block, or poll with
  a std `Waker` for an async executor, with no runtime and no new crate.
  The choices, and the bulk window's measured validation, are in
  `docs/decisions/7142g56xt6-remote-proto-mux.md`.
- Both peers ping. The server needs its own round trip to size an upload's
  bulk window, and its own lost-link detection.
- `Delayed`, a raw-frame peer, a test service and the
  `fe-remote-proto-peer` binary are behind `test-support`.
