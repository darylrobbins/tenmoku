---
task_uid: "01m5jp0z89d0eakgbfr1qwjt8m"
task: "T228"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "a_previews_matches_paint_within_its_time_limit passes 200 times in a row under load 30+ (stress-ng or parallel nextest loops) and no longer reads wall time for its 'paints within the limit' half; the 'gives up past the limit' half stays asserted (in this test or a separate one on a forced-slow search); unhurried() is not added to the limit tests"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui -p fe-vim passes and cargo clippy -p fe-gpui -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-gpui vim::incsearch_tests::a_previews_matches_paint_within_its_time_limit fails under load: the 100 ms preview limit expires before the first match (left None, right Some(1..2)) (Mac T1084 follow-up)

Flake, requested by the Mac lead (z48). Mac T1084 (on mac/main and, after the sync, argon's trunk) gave the other incsearch preview tests the `unhurried()` guard (search preview limit 60 s) and left the tests of the deadline itself unguarded on purpose.

## Failure
```
fe-gpui vim::incsearch_tests::a_previews_matches_paint_within_its_time_limit
panicked at crates/fe-gpui/src/vim/incsearch_tests.rs:309:5: assertion `left == right` failed
  left: None, right: Some(1..2)
```
0.28 s on Mac main under load (run 01m5jkm46jj2ahvspr4pfc33zr); passes alone in 0.1 s.

## Cause to confirm
The test counts regex calls (`CallLimited::of(500)`, `set_call_limit`) so the highlight deadline does not depend on machine speed, but the preview itself still runs against `SEARCH_PREVIEW_LIMIT` (100 ms of wall time, `crates/fe-vim/src/cmdline.rs:37`). Under load the 100 ms wall limit expires before the first match is found, so nothing is shown (`None`).

## Work
Make it load-proof without losing what it proves (do NOT just add `unhurried()`: then the test says nothing about the limit). Options: a test-only fake clock or deadline hook in fe_vim instead of wall time; or a long wall limit for this "paints within the limit" half, with the call limit doing the cut, plus a separate test that keeps the "gives up past the limit" half on a forced-slow search. Both halves must stay asserted. Prove it with repeated runs under load (stress-ng or parallel nextest loops, load 30+, at least 200 runs).

Files: `crates/fe-gpui/src/vim/incsearch_tests.rs`; `crates/fe-vim/src/cmdline.rs` and `crates/fe-vim/src/lib.rs` only for the test hook.

## Implementation notes

- The test now lifts both wall limits (`set_search_preview_limit` and `search::set_time_limit`, 3600 s) and restores them on drop, so only the 500-call budget cuts the search. `element.rs` caps the highlight by the new `fe_vim::search_preview_limit()` instead of the fixed 100 ms.
- Evidence: `cargo nextest run -p fe-gpui vim::incsearch_tests --stress-count 200` with 40 busy-loop processes (`while :; do :; done`, 40 on this host) running: 200/200 iterations passed (41.7 s; 1-min loadavg 2.7 at start, 15.0 at end, still climbing). An earlier 200-run of the single test under the same 40 loops passed 200/200 (84 s).
- Measured-load evidence (12 cores): 100 busy-loop processes (`while :; do :; done`) started first; waited until the 1-min loadavg reached 32. Then `cargo nextest run -p fe-gpui vim::incsearch_tests::a_previews_matches_paint_within_its_time_limit --stress-count 200`: 200/200 passed in 375.7 s. Loadavg at start `36.89 19.41 15.77`, at end `104.29 81.08 46.14` (the 100 loops sustained for the whole run).
