---
task_uid: "01m4txb22e04g2yd2x758c2pcm"
task: "T661"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "counted_replace_of_regional_indicators_stays_bounded passes 20 of 20 runs while fe-project's 100k-file tests run concurrently, and still fails if the counted replace is made quadratic (shown by a temporary local change, noted in the evidence)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-vim --test replace_regional and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-vim replace_regional counted_replace_of_regional_indicators_stays_bounded fails under load on its plain*4+1s wall-clock bound

## Problem

`crates/fe-vim/tests/replace_regional.rs:79`, `counted_replace_of_regional_indicators_stays_bounded`, checks a
wall-clock bound: the counted replace with backspaces must take at most plain*4 + 1 s. Under gate load (it
runs alongside the 100k-file tests a_100k_file_tree, m3_signoff, only_visible_rows) it fails:
25.63 s against 5.48 s plain (run 01m4tw6adhhvj335wna6vwqmwc, T615's gate on 5a3ff4b1); 16.7 s vs 3.0 s
(run 01m4rd874e…, 2026-10-12); 17.6 s vs 3.6 s (run 01m4svc7ex…, 2026-10-13 01:31). 3 failures in 360 runs.
Alone it passes (3/3).

## Change

Make the bound robust to load without losing its purpose (catching quadratic/unbounded work): e.g. time
each side as the minimum of N runs, interleaved, and/or count work (iterations, allocations) instead of wall
time, as T621 did for its wall-clock flakes. A fix to the test itself: no regression test; prove with repeated
runs under load (e.g. 20 runs while `cargo test -p fe-project` 100k tests run).
