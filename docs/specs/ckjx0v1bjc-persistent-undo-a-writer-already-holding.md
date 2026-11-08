---
task_uid: "01m6vr9vzvgw8pprckjx0v1bjc"
task: "T518"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "pane_item::tests::a_writer_holding_a_capture_dropped_by_off_then_on_writes_nothing and pane_item::tests::a_writer_holding_a_removal_dropped_by_off_then_on_removes_nothing: a writer that copied a capture, then waited on the policy lock while the window was turned off (capture dropped) and on again, neither writes nor removes the record"
    required: true
    manual: false
  - id: "C2"
    text: "UndoWrites::flush checks, while holding the policy lock and before any disk write or removal, that the capture's number is still path's pending entry; pane_item::tests::a_newer_capture_taken_while_a_writer_waits_is_still_written passes"
    required: true
    manual: false
  - id: "C3"
    text: "The existing pane_item::undo_writes_tests::* tests and fe-workbench undo_persist_tests::* tests (T507's included) still pass, unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-gpui -p fe-workbench pass"
    required: true
    manual: false
---

# Persistent undo: a writer already holding a capture writes nothing once the window turns editor.persistent_undo off and back on (follow-up to T507)

Follow-up to T507 (itself a follow-up to T483, persistent undo, lent from Mac T1411), from its approving review's finding F4 (medium, follow-up, `crates/fe-gpui/src/pane_item.rs:1115`). The review notes the edge case predates T507. When T483 is cut back to the Mac, this card goes with it.

# Persistent undo: a writer already holding a capture writes nothing once the window turns editor.persistent_undo off and back on

## Problem
`UndoWrites::flush` (`crates/fe-gpui/src/pane_item.rs`) runs on the background executor, one writer per capture (`queue_undo_write`). It takes the `writing` lock, then copies the file's newest capture out of `pending` (`waiting_numbered`), and only then takes the `policy` lock. While it holds `policy` it checks `policy.allows(window)` and writes or removes the record.

T507 made `UndoWrites::publish` (called by `set_window_persistent_undo` and `wait_for_window_persistent_undo` through `publish_undo_policy`) drop the captures in `pending` whose window may no longer write. `publish` takes `policy`, then `pending`. It does not take `writing`.

So a writer can copy a capture, then wait on `policy` while the window turns `persistent_undo` off and then on again. The first publish drops the capture from `pending`. The second publishes a policy that allows the window. The writer then takes `policy`, sees the window allowed, and writes (or removes) the record from its stale copy. A capture that was dropped comes back on disk. T507's spec says "Turning a window back on does not bring back a dropped capture." This path breaks that.

The review asked: "While holding policy, verify that the cloned capture's number is still pending before performing disk mutation, or attach an invalidation generation to captures. Add an off-then-on regression for writes and removals."

## Change
Take the first option: the pending-number check. Don't add a generation. The regression tests below stage the drop by editing `pending` directly while they hold `policy`, so a generation bumped only inside `publish` would not see it.

- In `UndoWrites::flush`, once the `policy` guard is held, lock `pending` and check that `path`'s entry is still the capture copied earlier (same `number`). If it is gone, or a newer capture has replaced it, return without touching the disk. Drop the `pending` guard before the disk write. Holding `policy` is enough: `publish` can't drop anything until the writer lets go of it. A `capture` landing during the write only replaces the entry with a newer number, and that capture has its own writer.
- Keep the rest as it is: the policy check under `policy`, the failed write kept waiting, and the removal of the entry after a successful write only if its number still matches. A small helper for "is `number` still `path`'s pending capture" can serve all three places.
- A newer capture is still written by its own writer, or by `flush_all` at quit. A writer holding an older copy now skips writing instead of writing the older record first.
- Removals (`record: None`) follow the same rule as writes.

## Tests
Add these to `crates/fe-gpui/src/pane_item_tests.rs`. It is `pane_item::tests`, a child of `pane_item`, so it can reach the private `UndoWrites`, `UndoPolicy`, and their `pending`, `writing` and `policy` fields. Don't add them to the inline `undo_writes_tests` module: the regression check copies only test files onto the base, and an inline module in `pane_item.rs` can't be copied without the fix. Use only APIs that exist on main (`UndoWrites::default`, `capture`, `waiting`, `flush`; `UndoPolicy { windows, fallback }`; `UndoStore::new`, `write`, `read`; `gpui::WindowId::from(1u64)`; `std::thread`, `std::sync::Arc`). Fail through `assert_eq!` on what is on disk, not through `unwrap`/`expect`.

- `pane_item::tests::a_writer_holding_a_capture_dropped_by_off_then_on_writes_nothing` (the regression test). An old record is on disk for `path`. Capture a new record for window 1. The test locks `writes.policy`, standing for a write in flight, and spawns a `std::thread` that runs `writes.flush(&store, &path)` on an `Arc<UndoWrites>`. It waits until the writer holds `writing` (spin on `writes.writing.try_lock().is_err()` with `thread::yield_now`), then sleeps about 200 ms so the writer copies the capture and blocks on `policy`. The existing `publishing_waits_for_the_write_in_flight` uses a 200 ms wait the same way. Still holding the `policy` guard, the test does what `publish(off)` and then `publish(on)` do: it sets the guard to a policy with the window off, drops the window's captures from `pending` (`retain` with that policy's `allows`), and then sets the guard to a policy with the window on. It drops the guard and joins the writer. Assert that `store.read(&path)` is still the old record and that `writes.waiting(&path)` is `None`. On main the writer writes the new record, so `assert_eq!` fails.
- `pane_item::tests::a_writer_holding_a_removal_dropped_by_off_then_on_removes_nothing` (regression): the same with a removal captured (`None`). The old record is still on disk afterwards. On main it is removed.
- `pane_item::tests::a_newer_capture_taken_while_a_writer_waits_is_still_written` (guard, passes on main too): as the first test, but instead of the off-then-on the test captures a newer record while the writer waits. After the writer finishes, a second `flush` (the newer capture's own writer) leaves the newer record on disk, and nothing is waiting.
- The existing `pane_item::undo_writes_tests::*` tests and T507's `undo_persist_tests::*` tests in fe-workbench still pass, unchanged.

The ~200 ms wait can only make the regression tests pass by mistake on main (the writer hasn't copied the capture yet, so it finds nothing to write). It can't make them fail with the fix. If the worker finds a deterministic way to know the writer has copied the capture, using only APIs on main, prefer it.

## Not in scope
A generation counter on captures. Changing `publish` to take `writing`: it must not wait for a disk write that is still queued. Other T483 follow-ups: T508 (Vim insert-mode import), T509 (read in flight across a re-root), T510 (Vim state across the import), T512 (live settings sources). Writes from another process.
