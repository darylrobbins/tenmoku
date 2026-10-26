---
task_uid: "01m5vngjyk7kj32g6v3ncs4sy9"
task: "T1182"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The cause of the loaded failures is named in the task notes with the evidence that shows it (which timing source builds the extra tiles 1,3/1,4 after the 'x' edit, drops partial tile 4 from drawn, and gives built=[1] where [] is expected)"
    required: true
    manual: false
  - id: "C2"
    text: "The three tests pass 30 of 30 runs under load (6 busy loops plus --test-threads 32), and the other tile_tests, including the big-file fill_and_fit and live_tiles_stay_bounded tests, still pass"
    required: true
    manual: false
  - id: "C3"
    text: "a_frame_past_its_budget_leaves_tiles_for_the_next still proves the budget, and replacing the minimap cache keeps a test-set budget (review F1)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-gpui and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-gpui minimap tile_tests typing_and_inserting_a_row_rebuild_only_their_tile_and_those_below, with_no_time_an_edit_keeps_every_tile_drawn, fill_settles_with_every_row_shown_and_redraws_tiles_in_place fail together under load (pass alone)

## Problem
Three fe-gpui minimap tile_tests (typing_and_inserting_a_row_rebuild_only_their_tile_and_those_below, with_no_time_an_edit_keeps_every_tile_drawn, fill_settles_with_every_row_shown_and_redraws_tiles_in_place) failed together in a loaded `cargo nextest run -p fe-gpui -p fe-workbench --profile ci` on the Mac on 2026-10-26, and pass alone.

Rev 1 found (worker, attempt 1): pinning the per-frame tile budget (1 s of wall clock in test builds) is not enough. Under 6 busy loops plus `--test-threads 32` the tests still fail in about 25-30% of 30 runs, even after waiting for the background reparse and language load (epoch plus needs_parse settle). Failures: extra tiles 1,4 / 1,3 built after the 'x' edit; tile 4 (the partial last tile) missing from drawn in with_no_time_an_edit_keeps_every_tile_drawn; built=[1] where [] is expected. So a second timing source exists besides the budget. Codex review F1 (replacing the cache resets the budget) is real; a fix for it is in the rev 1 WIP diff, /var/folders/1q/qsg5wyqs56z3d808y1c2c3r80000gn/T//t1182-wip.diff. The big-file tests (fill_and_fit..., live_tiles_stay_bounded) never reach needs_parse == false, so a plain reparse wait breaks them.

## Change
Diagnose the second timing source first and record it in the task notes. Candidates to rule in or out: wall-clock reads elsewhere in minimap.rs (other than the budget), highlight or syntax results arriving between frames and marking rows changed, tiles invalidated by a theme or language load, frame-to-frame reuse keyed on time.
- If the cause is in the tests (they assert before a nondeterministic async step settles), make the tests wait for that step deterministically, test-only.
- If the cause is in minimap.rs (it reads wall-clock time, or rebuilds tiles it should keep, in a way that a loaded machine changes), fix it there. Keep the production per-frame budget behaviour, and add a test that shows the fixed behaviour without relying on load.
Keep the F1 fix. No behaviour change beyond this.

## Tests
Prove with 30 runs of the three tests under load (6 busy loops plus `--test-threads 32`), before and after; record the before failure rate in the notes.

## Implementation notes

Cause (C1): fe-syntax's highlight query budget. `QUERY_BUDGET` in `crates/fe-syntax/src/syntax.rs` is 3600 s only under fe-syntax's own `cfg!(test)`; when fe-gpui's tests build fe-syntax as a dependency it is the 2 ms production budget (doubling to 16 ms). Under load a query runs out, `Syntax::highlights` returns partial spans and retries the range with twice the time, so rows are rebuilt between frames: extra tiles (1,3 / 1,4) built after the 'x' edit, partial tile 4 not drawn or built when the budget is ZERO, and built=[1] where [] is expected. It is not minimap.rs reading the clock beyond its own frame budget, and not a late reparse: waiting for the epoch and needs_parse to settle left the failure rate unchanged (8-9 of 30 runs), and the 3 suspected tests failed differently each run.

Before: 8-9 of 30 runs of `tile_tests` failed under 6 busy loops and `--test-threads 32`, with only the 3600 s frame budget set (also with the cache-replace fix and the reparse wait).
After: 0 of 30 with fe-gpui's dev-dependency on fe-syntax's `no-query-budget` feature (as fe-markdown-view already does), plus the test budget kept across the cache replacement (F1). Cargo.toml is outside the listed scope; the feature is the existing switch for this.

### Load experiment (reproduction, rerun for review F2)

Script (`load.zsh <label>`, zsh, from the worktree root; it starts 6 busy loops, kills them on exit, builds once, then runs the module 30 times and prints the failed tests of each run):

```zsh
export PATH=$HOME/.cargo/bin:$PATH
pids=()
for i in 1 2 3 4 5 6; do (while :; do :; done) & pids+=($!); done
trap 'kill $pids 2>/dev/null' EXIT
cargo nextest run -p fe-gpui --no-run --cargo-quiet
for n in {1..30}; do
  cargo nextest run -p fe-gpui --test-threads 32 --no-fail-fast \
    -E 'test(/tile_tests::/)' > $1.run$n.log 2>&1
  echo "run $n: $(rg -o 'FAIL \[[^]]*\] \([^)]*\) fe-gpui \S+' -r '$0' $1.run$n.log | sed 's/.*tile_tests:://' | sort -u | tr '\n' ' ')"
done
```

Before = HEAD with the `fe-syntax ... "no-query-budget"` dev-dependency line deleted from crates/fe-gpui/Cargo.toml (cache-replace budget fix kept). After = HEAD. All 12 `tile_tests` run each time.

Before: 5 of 30 runs failed (runs 4, 14, 15, 24, 28). Failures by test across the 30 runs:
- typing_and_inserting_a_row_rebuild_only_their_tile_and_those_below: 3 (runs 14, 24, 28)
- a_frame_past_its_budget_leaves_tiles_for_the_next: 2 (runs 4, 15)
- with_no_time_an_edit_keeps_every_tile_drawn: 1 (run 28)
- fill_settles_with_every_row_shown_and_redraws_tiles_in_place: 0 this time (it failed in the earlier 8-9/30 sample; rate varies with the other jobs on this host)

After: 0 of 30 runs failed; no test failed in any run.
Host note: other regatta gates run on this Mac, so the rate varies between samples (earlier sample 8-9/30, this one 5/30).

### Load experiment: exact commands and per-run results (verbatim)

Load generation (inside `load.zsh`, started before the runs):

    pids=(); for i in 1 2 3 4 5 6; do (while :; do :; done) & pids+=($!); done
    trap 'kill $pids 2>/dev/null' EXIT      # the six loops are killed when the script exits

Per run (30 repetitions, `for n in {1..30}`), after one `cargo nextest run -p fe-gpui --no-run --cargo-quiet` build:

    cargo nextest run -p fe-gpui --test-threads 32 --no-fail-fast -E 'test(/tile_tests::/)'

The filter selects all 12 tests in `element::minimap::tile_tests`, run together each time. The failed tests of a run are read from the `FAIL [...] fe-gpui element::minimap::tile_tests::<name>` lines. Both samples ran back to back on the same host, same build, with no other change.

"Before" is HEAD (6f6108be + notes) with the single line `fe-syntax = { workspace = true, features = ["no-query-budget"] }` deleted from crates/fe-gpui/Cargo.toml (the base 56679c6 has no test budget pinning at all; this isolates the fe-syntax query budget, with the test frame budget and the F1 fix kept). "After" is HEAD as committed.

Before-fix results (5 of 30 runs failed):

| run | result | failed tests |
|---|---|---|
| 1 | pass | none |
| 2 | pass | none |
| 3 | pass | none |
| 4 | FAIL | a_frame_past_its_budget_leaves_tiles_for_the_next |
| 5 | pass | none |
| 6 | pass | none |
| 7 | pass | none |
| 8 | pass | none |
| 9 | pass | none |
| 10 | pass | none |
| 11 | pass | none |
| 12 | pass | none |
| 13 | pass | none |
| 14 | FAIL | typing_and_inserting_a_row_rebuild_only_their_tile_and_those_below |
| 15 | FAIL | a_frame_past_its_budget_leaves_tiles_for_the_next |
| 16 | pass | none |
| 17 | pass | none |
| 18 | pass | none |
| 19 | pass | none |
| 20 | pass | none |
| 21 | pass | none |
| 22 | pass | none |
| 23 | pass | none |
| 24 | FAIL | typing_and_inserting_a_row_rebuild_only_their_tile_and_those_below |
| 25 | pass | none |
| 26 | pass | none |
| 27 | pass | none |
| 28 | FAIL | typing_and_inserting_a_row_rebuild_only_their_tile_and_those_below with_no_time_an_edit_keeps_every_tile_drawn |
| 29 | pass | none |
| 30 | pass | none |

After-fix results (0 of 30 runs failed):

| run | result | failed tests |
|---|---|---|
| 1 | pass | none |
| 2 | pass | none |
| 3 | pass | none |
| 4 | pass | none |
| 5 | pass | none |
| 6 | pass | none |
| 7 | pass | none |
| 8 | pass | none |
| 9 | pass | none |
| 10 | pass | none |
| 11 | pass | none |
| 12 | pass | none |
| 13 | pass | none |
| 14 | pass | none |
| 15 | pass | none |
| 16 | pass | none |
| 17 | pass | none |
| 18 | pass | none |
| 19 | pass | none |
| 20 | pass | none |
| 21 | pass | none |
| 22 | pass | none |
| 23 | pass | none |
| 24 | pass | none |
| 25 | pass | none |
| 26 | pass | none |
| 27 | pass | none |
| 28 | pass | none |
| 29 | pass | none |
| 30 | pass | none |
