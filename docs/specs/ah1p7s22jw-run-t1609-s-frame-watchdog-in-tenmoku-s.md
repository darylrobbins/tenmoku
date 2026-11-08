---
task_uid: "01m6wcrf3p3hkceeah1p7s22jw"
task: "T1610"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Every Tenmoku window created through new_workbench starts the kiln-shell frame watchdog on macOS. Test: a crates/fastedit test (new file crates/fastedit/tests/frame_watch_wiring.rs, or an existing in-binary test module if new_workbench is only reachable there) that opens a workbench window headlessly and asserts the watchdog is attached to it"
    required: true
    manual: false
---

# Run T1609's frame watchdog in Tenmoku's own windows (new_workbench in crates/fastedit/src/main.rs)

# Run the frame watchdog in Tenmoku's own windows (T1609 follow-up)

T1609 adds a frame-stall watchdog to kiln-shell (`kiln/crates/kiln-shell/src/frame_watch.rs`) and starts it
from kiln's `Root`. Tenmoku's windows are not kiln Roots: `crates/fastedit/src/main.rs` `new_workbench` roots
each window at a `Workbench`, so the watchdog never runs in the app that froze (T1507).

## Change

- In `new_workbench` (`crates/fastedit/src/main.rs`), beside `window.follow_system_appearance(cx)`, start the
  watchdog T1609 exports from `kiln-shell` (its pub entry point in `kiln/crates/kiln-shell/src/lib.rs`) for the
  window, so every Tenmoku window gets it, including ones opened later (new window, `--wait`, IPC handoff).
- Nothing else changes: the policy, logging and backoff are T1609's.

Platform: **Mac-only** (the watchdog compiles out elsewhere; the test still builds on Linux).
