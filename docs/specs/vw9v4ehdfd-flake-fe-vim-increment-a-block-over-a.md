---
task_uid: "01m4vpa7zdxjpwzqvw9v4ehdfd"
task: "T709"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_block_over_a_long_row_is_quick passes 20 of 20 runs while the workspace's heavy tests run in parallel"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-vim and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-vim increment a_block_over_a_long_row_is_quick still fails under heavy load after T442's ratio bound

## Problem

`fe-vim tests::increment::a_block_over_a_long_row_is_quick` failed T657's merge gate (run 01m4vnmpp7x966bz6pb9apb2ez,
2026-10-13 22:31Z) after 63.5 s, with the machine at load ~170 (three gates at once after a daemon restart). T657
changes only fe-keymap menus. T442 already bounded this test by a ratio instead of wall time, so it still fails under
heavy load: either the ratio's two timings see load very differently, or the slow side does real quadratic work on a
loaded machine.

## Change

Find why the T442 ratio still trips under load (run it 20 times while the workspace's heavy tests run in parallel and
record both timings) and make the test robust without weakening what it guards, the block increment over a long row
staying linear. If the measurement shows the code itself is quadratic, say so in the hand-in rather than loosening the
bound.

## Tests

Test-only fix unless the code is at fault: prove it with 20 runs under parallel load.

## Implementation notes

Diagnosis: the test is in-process (HeadlessVim, no nvim), so the code is linear; the ratio tripped on wall-clock scheduling skew. A 10x longer run is descheduled ~10x more often, so under load its fastest wall-clock sample drifts above 50x the short one's. Wall-clock ratios measured under 12 `yes` burners on a machine already at load 60-90: 10 to 19 (e.g. short=61.8ms long=616ms; short=167ms long=2.89s; short=481ms long=6.79s), the load-dependent spread that can reach 50 at load ~170.

Fix: time thread CPU time (CLOCK_THREAD_CPUTIME_ID, as fe-editor batch_structure does); bound (50x), 6 alternating samples and 10x sizes unchanged.

Evidence for C1: 20 consecutive runs of the built test binary (`fe_vim-11cca5fba40e3120 a_block_over_a_long_row --nocapture`) while 12 `yes >/dev/null` burners ran alongside the other lanes' workspace builds and nextest runs (visible in `pgrep`: cargo build --workspace, cargo nextest run --workspace). All 20 passed; fastest-sample CPU times per run:

1. short=63.29075ms long=889.679708ms
2. short=76.660375ms long=764.103542ms
3. short=64.672459ms long=725.512292ms
4. short=75.806125ms long=839.476916ms
5. short=64.951042ms long=680.139209ms
6. short=63.306541ms long=641.489417ms
7. short=61.619417ms long=617.58325ms
8. short=60.854125ms long=689.919959ms
9. short=60.585791ms long=652.483833ms
10. short=65.239541ms long=663.190375ms
11. short=62.545833ms long=650.994708ms
12. short=60.190667ms long=677.412333ms
13. short=68.475084ms long=714.9755ms
14. short=60.625208ms long=710.149583ms
15. short=60.443542ms long=677.847708ms
16. short=63.88875ms long=694.921125ms
17. short=79.096792ms long=957.488125ms
18. short=80.973583ms long=889.060208ms
19. short=70.998375ms long=789.953667ms
20. short=64.216459ms long=812.173334ms

Ratios were all 9.5-14.1 (linear ~10, quadratic would be ~100). Script: scratchpad load.sh. `cargo test -p fe-vim` on the candidate: all test results ok (321 passed in the lib), clippy -D warnings clean.
