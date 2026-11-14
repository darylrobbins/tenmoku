---
task_uid: "01m7ch6tgjt6x5rn4phwrwtnwt"
task: "T645"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "request_while_the_agent_end_is_closed_is_always_a_disconnect: with the caller and the connection driver on separate threads, a request() sent while the agent side of the transport is closed always returns an error for which is_disconnect() is true, never Failed, across at least 200 iterations, without waiting for the task queue to close or for termination to be published"
    required: true
    manual: false
  - id: "C2"
    text: "cancel_racing_the_agent_exit_is_always_a_disconnect: AcpClient::cancel() raced against the agent exiting always returns a disconnect error (is_disconnect() true), never Failed, and answers the session's pending permission requests Cancelled"
    required: true
    manual: false
  - id: "C3"
    text: "an error unrelated to the connection (the agent answers a request with a JSON-RPC error while the connection is alive) is still returned unchanged and is not classified as a disconnect"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-acp passes and cargo clippy -p fe-acp --all-targets is clean"
    required: true
    manual: false
---

# fe-acp client: a send-side failure on the outgoing channel (request or session/cancel) is normalized to a disconnect like an incoming one, whatever the interleaving with teardown (T1787 F11 follow-up) (Mac T1870)

Follow-up of T1787 review finding F11 (helium T619, medium): outgoing-channel failures escape disconnect normalization in crates/fe-acp/src/client.rs (`request` ~l.550, `cancel` ~l.519, and the send site ~l.469), so T1787 C5 (each state) is unmet for Disconnected. T1787 is respecced with this as a known limit.

Defect: `normalized(connection, method, error)` treats an error as a disconnect only if `is_disconnect(error)` (incoming-transport-closed, connection-terminated, or our own marker) or `ending(connection)` (termination published, or the task queue refuses a no-op spawn). agent-client-protocol 5.2.0's outgoing actor calls `pending_reply.fail` with an untagged local channel error BEFORE it drops the task receiver and before termination is published. A caller on another thread than the driver can therefore receive that error while `ending()` is still false, so `request()` returns Failed with no stderr instead of Disconnected. `cancel()` has the same window after the outgoing receiver drops, when `send_notification` fails. The rev-4 test waits until the task queue has closed, so it only covers the interleaving after `ending()` flips.

Fix: recognise a local outgoing-channel failure independently of task-queue closure and termination publication. Classify by error shape (read agent-client-protocol 5.2.0 jsonrpc.rs, where pending_reply.fail and send_request/send_notification produce it) and add it to the recognised set in `is_disconnect`, or wait for termination with a short bounded grace when `send_*` fails with a local channel error. An error from a live agent (a JSON-RPC error response) must stay untouched. Apply the same normalization at every send site, including the one ~l.469, via the single `normalized` helper. Keep `disconnected(method)` as the produced error so callers see one shape.

Tests: new crates/fe-acp/tests/outgoing_disconnect.rs, using only the base public API (AcpClient, is_disconnect; copy the fake agent/transport helpers from tests/client.rs rather than importing them): run the connection driver on its own thread, close the agent end, and call request() and cancel() from a different thread in a loop so the pre-termination window is hit; assert is_disconnect() on every error. Plus one control test that a live agent's JSON-RPC error is not turned into a disconnect.
