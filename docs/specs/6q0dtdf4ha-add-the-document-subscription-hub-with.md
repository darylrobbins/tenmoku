---
task_uid: "01m77zy6xxr3n34w6q0dtdf4ha"
task: "T359"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`tests::sync::fan_out::two_taps_drain_the_same_events_in_order`, `tests::sync::fan_out::late_tap_starts_at_the_latest_text` and `tests::sync::fan_out::detach_stops_both_taps` pass"
    required: true
    manual: false
  - id: "C2"
    text: "Existing crates/fe-lsp sync tests pass unchanged (one server per document, no behaviour change)"
    required: true
    manual: false
---

# Add the document subscription hub with per-server taps and latest text, with one server per document still (LSP add-ons A3, T1645) (Mac T1733)

# Add the document subscription hub with per-server taps and latest text, with one server per document still (LSP add-ons A3, T1645)

Spec: §4 (Document sync), §8 row A3.

Row: the subscription hub: `Subscribed` with per-server taps and latest text. Still one server per document, so behaviour is unchanged.

Test file: crates/fe-lsp/src/tests/sync/fan_out.rs (two taps drain the same events in order; a late tap starts at the latest text; detach stops both). Declare it from tests/sync.rs.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- `Subscribed` (document.rs) is now the hub: the buffer's subscription, a
  list of taps (each with its own event queue and caught-up save), and the
  latest `(Text, Version)` drained. `Subscribed::new` takes the text and
  version the subscription was made at; `tap()` returns an `Attached` and
  the text and version it starts at.
- Draining any tap (`Attached::take_events`) drains the subscription into
  every tap, cloning for all but the last, and moves the latest text on.
  A new tap is added without draining, so events still in the subscription
  (all newer than the latest text) reach it too, saves included.
- Dropping an `Attached` removes only its tap. Dropping the last tap drops
  the subscription, as dropping the one `Attached` did before, so a stop or
  failed start still stops the buffer queueing events
  (`subscriptions_go_with_the_server`). `detach` fans out what is queued,
  then drops the subscription.
- The manager makes one tap per open (`open` and `open_on`), so there is
  still one server per document; server.rs needed no change.
