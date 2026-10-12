---
task_uid: "01m4pyvrk3t0fhc85a4apmq66h"
task: "T21"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-editor test tests::batch_structure::many_batches_recorded_together_take_linear_work has no absolute wall-clock bound: it times 1,000- and 10,000-batch recordings alternately for 5 rounds, asserts the fastest 10,000 run is under 50x the fastest 1,000 run (linear ~10x, quadratic ~100x, fails), and keeps the folds and text-length assertions on every run"
    required: true
    manual: false
  - id: "C2"
    text: "The hand-in records at least 20 runs of that test under load (load average stated per run, other lanes building) with no failure, plus best-of-5 interleaved per-batch timings at 1,250, 5,000, 20,000 and 40,000 batches showing per-batch cost does not grow with size"
    required: true
    manual: false
  - id: "C3"
    text: "Of the worker's own changes, only crates/fe-editor/src/tests/batch_structure.rs changes; no production code changes"
    required: true
    manual: false
---

# Bound fe-editor many_batches_recorded_together_take_linear_work by a best-of-interleaved-runs ratio, not a 2-s wall clock, so it passes under load on the Mac (Mac T445)

Filed on helium from Mac T445 (~/coordination/helium-cards/T445.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-h.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

`fe-editor tests::batch_structure::many_batches_recorded_together_take_linear_work`
(`crates/fe-editor/src/tests/batch_structure.rs:138`) records 10,000 one-char batches through one
`EditorModel::buffer_mut` borrow, `sync_display`s a folded split, and asserts it all takes under 2 s
of wall time. That bound is load-sensitive. Neon's T51 (commit d6374327, branch `neon-batch-7`)
swapped it for a ratio: one run at 2,500 batches and one at 10,000, asserting the larger one takes
under 9x the smaller. That passes on Linux. On this Mac at load 75–107 it failed 5 of 5 times:
31 vs 365 ms (11.7x), 25 vs 474 (19x), 60 vs 1010 (17x), 16 vs 774 (49x) and 34 vs 483 (14x). Each
size was timed once, one after the other. T51 was left out of the Mac landing of neon batch 7 until
this task decides between the two tests.

The drafter checked whether the work is really superlinear. It is not.

- **Code.** Every step on this path does constant or logarithmic work per batch:
  - `fe_buffer::Buffer::edit` → `apply`, `record` and `emit_batch`. `trim_history` only runs over the
    limit, and `Text` is a `ropey` rope, so a clone is O(1).
  - `EditLog::push`, on a sum tree.
  - `History::record` and `Shape::of` in `crates/fe-editor/src/lib.rs`, which do O(log n) rope lookups.
  - `catch_up` → `surviving_folds`, one pass over the batches.
  - `fe_display::display_map::row_edits`, which merges the batches into one hull in one pass.
- **Measured.** A scratch binary ran the test's exact workload against `main`. It is
  `/private/tmp/claude-501/-Users-daryl-code-fastedit/8c493315-3a57-4f22-a373-f3e05cbd7da5/scratchpad/batchprobe/src/main.rs`,
  a crate outside the workspace with path dependencies on fe-editor and fe-buffer. It may be gone
  by the time you read this; rebuild it if you need it. It
  took the best of 7 runs per size, each size's runs back to back, at load 57–80:
  - In the quietest run, best-of-7 cost per batch was flat: 6.0, 5.6, 5.6, 26.6 (a spike), 9.9 and
    7.5 µs at 1,250, 2,500, 5,000, 10,000, 20,000 and 40,000 batches.
  - In the other runs, 40,000 batches cost 7.5–27 µs per batch against 6–11 µs at 1,250. That is
    nowhere near the 32x that quadratic work would add.
  - Single sizes spiked from run to run (10,000: 192 ms in one run, 266 ms in another).

So best-of-N alone isn't enough when each size's runs are back to back. One load burst over the
large size's block gave a 19x ratio (266 vs 14 ms) even as a best of 7. The test has to interleave
the sizes so that load hits both alike.

## Change

This is a test-only change, in `crates/fe-editor/src/tests/batch_structure.rs`. Production code
stays as it is.

Replace the absolute 2-s bound in `many_batches_recorded_together_take_linear_work` with a
load-robust ratio:

- Keep the workload: a fresh `folded()` pair, N one-char batches appended through one `buffer_mut`
  borrow, then `b.sync_display()`. Keep its folds assertion and its text-length assertion on every
  run.
- Time it at 1,000 and at 10,000 batches. Linear work gives about 10x and quadratic about 100x.
- Interleave the sizes: run 1,000 then 10,000, five rounds in all. Take the fastest time of each
  size.
- Assert that the fastest 10,000 run takes under 50x the fastest 1,000 run (T442 found fastest-of-N ratios reaching 29x for linear work at load 120-200, so 30x has no headroom; quadratic work is ~100x). Put both fastest times
  and every sample in the failure message.
- Say in a comment why the test takes the best of interleaved rounds rather than one run per size.

The ratio, sizes and bound follow Mac T336 and T442. The whole test must stay well under 2 s at
light load in a debug build.

Before writing the hand-in, re-check the evidence above. Take the best of 5 interleaved rounds at
1,250, 5,000, 20,000 and 40,000 batches, and record the per-batch cost and the load average. If
per-batch cost grows steadily with size (40,000 costs over 3x per batch what 1,250 does, in two
separate runs), stop and block with the numbers. That is a production bug, and the lead will respec
this task as a bugfix.

There is no regression test: a test-only fix can't be made to fail at base on demand.

## Implementation notes

- Test now times 1,000 and 10,000 batches alternately for 5 rounds, asserts fastest-10,000 < 50x fastest-1,000, keeps folds/length assertions every run; failure message lists both fastest times and all samples.
- 20 consecutive runs, all passing (~0.49 s each, debug), with 8 busy-loop processes running alongside; loadavg (1-min, lagging) per run: 1.90, 1.90, 2.47 x7, 2.99 x8, 3.48 x3. Linux box: load did not reach the Mac's 75-107, so this shows no flakiness here, not under Mac load.
- Best-of-5 interleaved per-batch cost, two runs (load 1.99 and 2.07): 1,250: 6.45/6.48 us; 5,000: 6.98/6.92 us; 20,000: 7.91/8.00 us; 40,000: 8.22/8.18 us. 40,000 is 1.27x the per-batch cost of 1,250 (limit for a block was 3x): no production bug.
- Rerun under real concurrent load (other lanes' builds on this 24-core Linux box; Mac not available to this worker). 20 runs, all passed. Load avg (1/5/15 min) and test time per run: 1-2: 20.32/11.74/5.01, 0.83/0.87s; 3-7: 21.09/12.04/5.14, 0.95/0.71/0.87/0.92/0.90s; 8-11: 21.41/12.26/5.25, 1.06/0.92/0.96/1.00s; 12-17: 24.50/13.05/5.54, 0.45-0.48s; 18-20: 22.77/12.88/5.53, 0.46-0.47s. The earlier busy-loop runs are superseded.
