---
task_uid: "01m4ekr58vcknp9q0dqj4j4651"
task: "T204"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "A buffer subscribed early, edited and saved, then opened through LspManager::open (routed, not open_on) sends didOpen with the current text followed by didSave for that version; a save of an earlier version, superseded by a later edit, is not sent. Tested in crates/fe-lsp/src/tests/registry/routed_save.rs."
    required: true
    manual: false
  - id: "C2"
    text: "A document handed on to another server (a failed or restarting server's openings, via Places::forward) with a queued save of its caught-up version also gets that didSave after its didOpen on the new server, and its didOpen text is the latest edit. Tested in the same new file."
    required: true
    manual: false
  - id: "C3"
    text: "The fix is in crates/fe-lsp/src/document.rs only; tests/registry.rs gains only the mod line; events_before_the_opened_version_are_skipped and the rest of fe-lsp's tests pass unchanged."
    required: true
    manual: false
---

# Follow-ups from review of T177: routed opens and server handoffs keep a queued didSave of the opened version (F5)

Finding F5 of the review of T177 (below merge.block_severity), split from T201.

- F5 medium crates/fe-lsp/src/server.rs:186: Unconditionally calling catch_up now discards queued Saved events on initial routed opens and server handoffs. Subscribe to a buffer, edit and save it, then pass its current snapshot and that subscription to LspManager::open: didSave is lost because catch_up drains every event but retains only edits. The existing events_before_the_opened_version_are_skipped test explicitly requires a save of the opened version to reach the server, but uses open_on and therefore misses this regression. Servers relying on save notifications can miss work.
  fix: Keep didOpen current without discarding save notifications applicable to the opened version. Add a routed-open regression using an early subscription and a queued save of the current version, alongside the late-handoff text regression.
  instances: crates/fe-lsp/src/server.rs:186

Still open on main (86d226d): `Opening::catch_up` (crates/fe-lsp/src/document.rs:241) drains `take_events()` and keeps only `Edited`; it is called from `Places::forward` (server.rs:186), restart.rs and manager.rs (the lot).

## Fix

In crates/fe-lsp/src/document.rs only: `Opening` remembers a save it drained that applies to the version it is caught up to (a `Saved` at or after that version, cleared by a later edit), and `Document::open` queues that `didSave` right after the `didOpen`, the order `open_on` gives today (didOpen, didSave, then later didChange). Saves of earlier versions are still dropped. The callers of `catch_up` (server.rs, restart.rs, manager.rs) don't change.

## Tests

New file crates/fe-lsp/src/tests/registry/routed_save.rs, declared by one `mod routed_save;` line added to crates/fe-lsp/src/tests/registry.rs (no other change there; it uses registry's `routed`, `open`, `barrier` via `super::`). Don't touch crates/fe-lsp/src/tests/mod.rs (T144 to T147 hold it).
