---
task_uid: "01m4s0c1kdc6jgbh13eh5ch952"
task: "T58"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "gesture_ends_even_when_a_sibling_stops_propagation waits (bounded, like expect_written/until) until the press and release reports are printed before asserting, instead of reading once after a fixed settle"
    required: true
    manual: false
  - id: "C2"
    text: "Test-only or test-support only: no production code changes; the affected crate's tests pass under nextest"
    required: true
    manual: false
---

# Wait for the mouse report instead of a fixed settle in fe-terminal-view's gesture_ends_even_when_a_sibling_stops_propagation (flake)

Flake (load ~36, 2026-10-12): `fe-terminal-view::mouse gesture_ends_even_when_a_sibling_stops_propagation` failed in T54's gates (for example run 01m4rz7qs5jrecfs7w9sfgb06d). It passes on its own.

Cause (from reading crates/fe-terminal-view/tests/mouse.rs): after the button release, the test calls `settle(cx)`, a fixed 200 ms of drawing, and then reads `written(...)` once. Under load, the hex_reader program hasn't printed the release report yet, so the `text.ends_with('m')` or `starts_with` assertion sees partial output.

Fix (test-only): wait for the output instead of using a fixed delay, as the file's other tests do with `expect_written` / `until`. Wait until the program has printed both the press report (`\x1b[<0;6;3M`) and a release report ending in `m`, with the file's usual 5 s `until` bound. Then make the same assertions. Check the file for any other `settle(cx)` followed by a one-shot `written(...)` read, and fix those the same way. No production code changes.
