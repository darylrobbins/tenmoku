---
task_uid: "01m55ycbw1cjksprk2m135revm"
task: "T192"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-display/benches/display.rs has fold/one more fold among 200k, wrapped, fresh tree and fold/one more fold among 200k, wrapped, after 100k keystrokes: each builds its own DisplayMap wrapped at 80 columns; the second types 100,000 seeded untimed keystrokes (buffer.edit + map.sync, no folds, history limit lifted) before fold_every(5); both time fold as the existing 200k case does"
    required: true
    manual: false
  - id: "C2"
    text: "On helium, base (with this card's bench file) and candidate bench binaries run alternately for 3 rounds (--warm-up-time 1 --measurement-time 3 'fold/'), fastest sample per case: in every candidate round the after-100k-keystrokes case is at most 1.5x the fresh-tree case; no fold/ case is more than 10% slower than base; the decision file and PERF.md's Display map section have the tables (machine, commits, date, load), with base's ratio beside the candidate's"
    required: true
    manual: false
  - id: "C3"
    text: "Same protocol with 'sync/': no sync/ case's fastest sample is more than 10% slower than base; the slowest single map.sync over the 100k typed keystrokes is at most 4 ms in the candidate (no whole-tree repack lands on one keystroke), with base's slowest beside it in the decision file, plus the leaf counts fresh, after 5k and after 100k keystrokes"
    required: true
    manual: false
  - id: "C4"
    text: "Unit test wrap_tree_matches_a_fresh_map_after_random_splices (new file crates/fe-display/src/wrap_map/compact_tests.rs; cargo test -p fe-display --lib wrap_map::compact_tests) passes: after thousands of random splices on wrapped text, with the compaction thresholds lowered so compaction runs often, row_count and every segment equal a freshly rebuilt WrapMap of the same text and width"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-display passes with its existing wrap, fold and round-trip tests unchanged (T181's fold_rows_across_edits included); DisplayMap's public API is unchanged and gpui-pre-sum-tree is neither vendored nor patched"
    required: true
    manual: false
---

# fe-display: keep the wrap tree compact under edits, so a fold among 200k after 100k keystrokes is within 1.5x of one on a fresh tree (follow-up to T181 / T159 / Mac T813)

## Problem

Every edit leaves fe-display's wrap tree more fragmented, and anything that walks the whole tree slows
down as a result. `FoldMap::rebuild` (`crates/fe-display/src/fold_map.rs`) is one such walk: it reads
every fold's rows through `WrapMap::first_rows`. `fold`, `fold_many`, `unfold`, `unfold_all`,
`set_wrap_width`, `rewrap`, `wrap_pending` and `replace_text` all call it, and still will after T181.

T181's worker found this while measuring (lane fgw53sd0zv,
`docs/decisions/fgw53sd0zv-fold-rows-across-edits.md`, "Why `fold/` reads slower"). The run used one
process, T181's candidate build, 200k folds and 1M lines wrapped, and took the fastest of 30 `fold` calls:

| Wrap tree | `fold` |
|---|---|
| fresh | 4.06 ms |
| after 5,000 bench-style keystrokes | 7.34 ms |
| after 170,000 keystrokes | 15.97 ms |

The cause is `WrapMap::splice` -> `replace_rows` (`crates/fe-display/src/wrap_map.rs`). It slices the
`SumTree<LineItem>` up to the edit, pushes the new items, then appends `cursor.suffix()`. That allocates new
nodes along the path and splits a leaf at each seam: `sum_tree` (the external crate `gpui-pre-sum-tree =0.3.7`,
`Cargo.toml` `sum-tree`, not vendored) has no in-place update by position. After many edits the tree has
more, emptier leaves, and its nodes are spread across the heap. A walk that is memory-bound (T159's decision
file, `docs/decisions/ez74dvmwrv-fold-rows-one-cursor-pass.md`) then takes 2-4x longer. Base does this
too, so it is not T181's regression. It is also why T181's full-bench `fold/` figures depend on how many
keystrokes criterion fits in before them.

The lead's decision (2026-10-17): "Keep fe-display's wrap tree compact under edits, so a fold after many
keystrokes is as fast as on a fresh tree."

## Change

Against T181's `wrap_map.rs`:

1. **Measure first.** Using a test-only or bench-only count, record in the decision file how many leaves
   and how many items per leaf the wrapped 1M-line tree has fresh, after 5,000 keystrokes and after
   100,000. Also record how much of the `fold` slowdown a full repack (`SumTree::from_iter` over the items)
   wins back. This shows whether leaf count or heap scatter is the main cost.
2. **Keep the tree compact in `WrapMap`**, with no change to `sum_tree`. Start with the seams: after
   `replace_rows`, re-pack the few items around each seam into full leaves, as Zed's `SumTree` merges
   underfull nodes when appending. If that doesn't meet C2, also re-pack in bounded windows that rotate
   through the tree, a fixed number of items per `splice`. A whole-tree repack must never land on one
   keystroke: a 1M-item `from_iter` costs more than a frame. A full repack may run only where a full pass
   already happens (`rebuild`, `set_wrap_width`). Keep the trigger thresholds in `WrapMap`, and give the
   tests a way to lower them.
3. If neither approach can work through `gpui-pre-sum-tree`'s public API, block with `rgt task block --note "<why>"` for the lead. Do not
   vendor or patch the crate: that is the lead's decision.
4. **Bench.** In `crates/fe-display/benches/display.rs`, add two cases to the `fold` group:
   `fold/one more fold among 200k, wrapped, fresh tree` and
   `fold/one more fold among 200k, wrapped, after 100k keystrokes`. Each builds its own `DisplayMap` from the
   bench's buffer and wraps it at 80 columns. The second one then types 100,000 single-character
   keystrokes, untimed, with a fixed seed: `buffer.edit` plus `map.sync` with no folds, so the typing is
   cheap. Both then add the 200k folds with `fold_every(5)` and time `fold` the way the existing
   `fold/one more fold among 200k, wrapped` case does. Keep `buffer.set_history_limit(usize::MAX)`, because
   ~50k+ edits into one `Buffer` otherwise hit fe-buffer's history trim. Time each `map.sync` during the
   typing and print the slowest one.

DisplayMap's public API and the wrap rows it reports stay the same.

## Implementation notes

- **Step 1 found heap scatter, not leaf count, to be the main cost.** After 5k keystrokes the tree
  had 5% more leaves and a 2x slower walk. After 100k it had 40% more leaves and a 5–6x slower walk,
  with nearly every leaf away from the one before it in memory. A full repack won most of that back
  (3.9–4.4 ms against 16–22 ms; fresh 2.8–3.0). Windowed re-packs gave full leaves but left every
  one of them scattered, because the allocator puts the new leaves in recent splices' holes. Seam
  re-packs wouldn't help either: `sum_tree`'s `append` already merges leaves under half full, and
  the public API can't find leaf boundaries.
- So the main change is the one T181's decision file suggested: **an edit that keeps each row's
  number of wrap rows rewrites the rows' items in place** (`rewrite_rows`), and the tree is left
  as it was. A wrapped row's wrap points are now `Breaks(Box<[AtomicU32]>)`, because `sum_tree`
  hands out only `&` to items. `DisplayMap` stays `Send + Sync`, and its API is unchanged.
- Splices that do move items (a new row, or a changed wrap count) are followed by a rotating
  window re-pack (`Compactor`: 384 items every 8 such splices, set lower by the tests). It bounds
  the leaf count (101k against 129k after 100k Enters) but can't restore memory order. No
  whole-tree repack runs anywhere.
- Extra tests: `windows_sweep_a_growing_tree` (a window that reached the end used to stay there
  while edits added rows), and `an_edit_keeping_wrap_rows_leaves_the_tree_in_place`. The ignored
  `measure` test prints the leaf counts and walk times recorded in
  `docs/decisions/k2m135revm-wrap-tree-compact.md`.
- The bench also prints the 10th-slowest sync and the number over 1 ms, to tell preemption from
  work.
