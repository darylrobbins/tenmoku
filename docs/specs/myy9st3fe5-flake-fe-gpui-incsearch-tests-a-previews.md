---
task_uid: "01m66t1c9qdca6v8myy9st3fe5"
task: "T1248"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "With cargo test -p fe-gpui --lib (one shared process) the five incsearch tests named in the body pass 20 runs in a row; counts recorded in the spec notes, with whether any nextest gate log ever showed them"
    required: true
    manual: false
  - id: "C2"
    text: "Existing tests pass; clippy and fmt pass for fe-gpui"
    required: true
    manual: false
---

# Flake: fe-gpui incsearch_tests a_previews_matches_paint_within_its_time_limit and three preview tests fail under cargo test load (shown(..) none or scroll <= 300) at base

Found by T1245's diagnosis (lane bsj2pf5qgf, 2026-10-30). Under `cargo test -p fe-gpui --lib` (one shared process, the rest of the lib tests alongside), fe-gpui incsearch tests fail intermittently at base 1de0c9c: 4 of 10 runs had an incsearch failure (2 of 10 with T1245's thread-local preview-limit change; an earlier 20-run batch on that change failed in 9 runs).

Panics:
- incsearch_tests::a_previews_matches_paint_within_its_time_limit (crates/fe-gpui/src/vim/incsearch_tests.rs:330): assertion `left == right` failed, left: None, right: Some(1..2) on shown(..).0 after the preview, although the test sets a 3600 s preview limit, so the 100 ms deadline is not the cause.
- a_preview_reveals_its_match_and_a_cancel_puts_the_view_back (:144), turning_vim_off_ends_the_preview (:367), incsearch_set_through_the_shared_registers_reaches_another_editor (:391): assertion failed: shown(&view, cx).2 > 300.
Also seen once: incsearch_fold_tests::edits_on_both_sides_keep_the_fold.

First check whether these ever fail in the nextest gate (grep stored run logs under ~/.local/state/regatta/runs/*/test.*.log). If they fail only under cargo test, look for shared process state between tests (statics, thread-locals reused across tests on one thread, draw/scroll timing). Likely a draw-timing issue in how the test reads the previewed match after a frame. Fix the test (or production code if a real race; then it becomes a bugfix with a regression test).

## Implementation notes

- Cause: `set_call_limit` and `set_time_limit` are process-wide atomics. `a_previews_matches_paint_within_its_time_limit` set a 500-call budget and its drop reset the time limit, so previews on other test threads in the shared `cargo test` process ran out of budget (`shown(..)` none, scroll <= 300). The preview limit is per thread, so it was not shared.
- Fix: one `LIMITS` mutex in `incsearch_tests.rs`; a `Limits` guard holds it and restores the preview, time and call limits on drop (panic-safe). `unhurried()`, the call-budget test, the fold tests and the sticky-scroll preview test take it. Fold tests no longer leave 600 s limits behind (T1085 F1).
- Not done: T1006 F1 (the counted budget with wall-clock expiry disabled) is as before; the test still sets a 3600 s limit.
- Review r1 F1: the call budget (`set_call_limit`) and its counter (`regex_calls`) in `fe-vim/src/search.rs` are now thread-local, like the preview limit, so the counted-budget test's override and accounting are isolated to its own test thread. `LIMITS` stays for the process-wide wall-clock limit. This touches fe-vim (outside the first scope); `fe-vim`'s search_deadline_and_ranges and `fe-workbench`'s find_replace_interrupted set and read the budget on one thread and ran clean after the change (5 and 7 tests passed). Every caller of `set_call_limit`/`regex_calls` in the workspace is a test (fe-vim lib and integration tests, fe-workbench, fe-gpui incsearch_tests); none is production code, and none searches on another thread than the one that set the budget.
- `a_cold_fence_seen_only_in_the_minimap_gets_its_colours` now fences Ruby, which no other fe-gpui test loads, instead of Python, so its "cold" assertion holds in a shared process.
- C1: `cargo test -p fe-gpui --lib`, 20 runs in a row, no skips, 710 passed / 0 failed each (logs kept). Before the fix: 4 of 10 runs failed (spec). No nextest gate log in ~/.local/state/regatta/runs/*/test.*.log (3561 files) shows these tests failing.
