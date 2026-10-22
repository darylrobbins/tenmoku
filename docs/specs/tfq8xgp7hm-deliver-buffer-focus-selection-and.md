---
task_uid: "01m5j50sx2k0vc93tfq8xgp7hm"
task: "T277"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "events::changed_events_for_one_buffer_coalesce: three consecutive changed events for one buffer are delivered as one with the latest version and the union of ranges"
    required: true
    manual: false
  - id: "C2"
    text: "events::focus_and_selection_keep_only_the_latest: ten focus events queued behind a busy plugin deliver one"
    required: true
    manual: false
  - id: "C3"
    text: "events::opened_saved_closed_are_never_merged_and_stay_ordered: per-buffer order is preserved across coalescing"
    required: true
    manual: false
  - id: "C4"
    text: "events::a_full_queue_drops_the_oldest_and_reports_lagged: pushing 1000 events at 256 capacity never waits on the plugin, keeps ONE shared capacity of 256 across all pending storage, evicts the oldest coalescible event (a full queue never discards the newest focus or selection), and the next delivery is lagged{dropped}"
    required: true
    manual: false
  - id: "C5"
    text: "events::a_stuck_plugin_does_not_delay_the_sender: while the plugin is inside a 100 ms call, 1000 pushes complete in under 50 ms (a push never waits on the plugin; it may wait at most for another sender or the drain to finish a critical section of at most O(capacity) work, see Known limits)"
    required: true
    manual: false
  - id: "C6"
    text: "events::an_unloaded_plugin_receives_nothing: after an unload push is a no-op and the queue is empty"
    required: true
    manual: false
  - id: "C7"
    text: "events::events_carry_no_text: a changed event holds ranges and a version only"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Deliver buffer, focus, selection and config events to plugins through a bounded 256-event queue that coalesces and never blocks the sender (Mac T1053)

## Problem

Plugins react to the editor through events (§7): buffer opened, changed (with edit ranges and version), saved, closed;
focus and selection changed; config reloaded. They must be delivered asynchronously and coalesced, and a slow plugin
gets fewer, later events, never blocking the app.

## Change

`kiln/crates/kiln-plugin/src/events.rs` and `wit/events.wit`:

- `EventQueue` per plugin, bounded to `Limits::event_queue` (256): `push(&self, Event)` never blocks and never allocates
  without bound. Coalescing: consecutive `changed` events for one buffer merge into one (latest version, the union of
  ranges mapped to the latest text, or a `resync` flag when the union would exceed a cap); `focus` and `selection`
  keep only the latest; `opened`, `saved`, `closed` are never merged but keep per-buffer order; on overflow the oldest
  coalescible events go first, and the plugin next receives a `lagged { dropped }` event.
- Delivery: the plugin's thread drains the queue one event per call under the call deadline (the runner's `call`),
  in order; an unloaded plugin receives nothing and its queue is dropped.
- Events carry ranges and versions only, never text; reading text needs `buffer.read` (P11).

## Notes

The sender is the UI thread; the test that matters is that `push` returns fast while the plugin is stuck in a slow
call. Wiring editor events into `push` is the app wiring card (P19).

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Design and Known limits (respec after review round 2)

One `Mutex<Queue>` holds the whole queue (deque, coalescing state, lagged count): one store, one shared capacity of `Limits::event_queue`, coalescing and oldest-first eviction cover every pending event. The lock is held only for O(capacity) work, never across a plugin call, a WIT call, an allocation without bound, or I/O.

Known limit: "never blocks the sender" means a push never waits on the plugin and never waits on anything unbounded; it can wait at most for another sender's or the drain's critical section (O(256) work). It is not wait-free: a strictly wait-free bounded coalescing queue needs a hand-written lock-free structure with unsafe code, which is its own card if wanted.

## Implementation notes

- The host pushes a `HostEvent`, not the WIT `Event`: its change carries `Edit { replaced, inserted }`
  (byte ranges in the text before, and a length; still no text), because the WIT change's ranges are in the
  text *after* it, and mapping an earlier change's ranges into the latest text needs each later edit's old extent.
  The queue stores and delivers WIT events.
- The `resync` flag is an empty `ranges` list rather than a new field: adding a field to `buffer-changed` changes
  the type every fixture and guest in other cards spells out. `events.wit` documents it. The cap is `MAX_RANGES`
  (64); a change of more edits than that resyncs before anything is allocated for it.
- One `Mutex<Queue>` holds everything that waits: the deque, the subscription, the lagged count and the closed flag,
  so the one capacity bounds it all and coalescing and eviction see every pending event. A push or pop holds the lock
  for at most O(capacity) work (a scan of the deque, a merge of at most `MAX_RANGES` ranges, a selection of at most
  `MAX_RANGES` ranges mapped), and delivery holds it only to pop, never across the plugin call (Known limits). Round 2's
  lock-free inbox is gone.
- A merged change keeps its place in the queue. A selection of that buffer waiting after it moves with it: its
  ranges are mapped through the new edits and it takes the new version, so it never arrives older than a change
  before it; if the new edits can't map it (out of order, or past the cap), or the selection has more than
  `MAX_RANGES` ranges, the change doesn't merge. Focus,
  selection (per buffer) and config reloads move to the end as the latest, so a focus never arrives before the open
  of the buffer it names. Config reloads coalesce too.
- `Recipient` owns the `Plugin` beside its queue: `deliver` sends one event per `Plugin::call`, and any call that
  unloads the plugin (an event or a command), or dropping the recipient, closes the queue, after which `push` does
  nothing. runner.rs is outside this card, so the queue is tied to the plugin's lifecycle by owning it rather than
  from inside `Plugin`.
- The guest's `subscribe` import stays a stub: `events::State` is built in `bindings.rs` as a unit struct with no
  handle on the queue, and runner.rs and bindings.rs are outside this card. The queue starts subscribed to nothing,
  as the WIT says; `EventQueue::subscribe` is what that import will call once the wiring card (P19) links them.
- Subscription gaps (review F7): a change pushed while the plugin is not subscribed to changes is not queued, but it
  marks its buffer's waiting change as resync (empty ranges, the gap's version), so no later change maps ranges across
  the missing history. A selection waiting after that change goes and counts toward `lagged`. A selection pushed after
  the gap is in the latest text and still moves with later merges.
- A zero `event_queue` limit stores nothing: every push does nothing (review F8).
