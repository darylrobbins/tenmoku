---
task_uid: "01m5gxejvserk3q1h8na7gd95s"
task: "T218"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::macros::replays_end_cleanly and tests::macros::record_and_play each take under 4 s in a debug build on an idle machine, still proving what they proved (cargo nextest run -p fe-vim macros)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# fe-vim: cut the macro tests and the MAX_PUT-bound insert tests T215 could not reach (HeadlessVim::feed_key fast path) (T215 follow-up)

## Problem

T215 (Mac T1069) cut most of fe-vim's 10 to 22 s unit tests but left these, because the cut is not reachable in its scope (see its spec Implementation notes):

- `insert::keys::tests` that need 64 MiB MAX_PUT of work (the replace-copies-count and opened-rows tests, `session_overwrite_and_backspace`): changing the counts (7000/8000) did not change the time.
- `tests::macros::replays_end_cleanly` (15 s) and `tests::macros::record_and_play` (10 s): they push more than 500k keys through `HeadlessVim::feed_key` at about 10 us each.

## Fix

Add a fast path in `crates/fe-vim/src/headless.rs` for command-line character keys (the dominant cost per T215's measurement) so the macro tests reach their key counts in under 4 s, and make the MAX_PUT-bound tests take a smaller budget through a test-only override of the cap if one can be added without changing production behaviour. Keep what each test proves; no wall-clock asserts.

## Implementation notes

- Added a command-line character fast path to `HeadlessVim::feed_key` (skips sync_visual/follow_cursor, which a line character cannot change). Macro tests: 2.9 s / 5.2 s -> 0.9 s / 1.0 s.
- The MAX_PUT-bound insert tests were left alone: no test-only cap override was added, to keep production code unchanged.
