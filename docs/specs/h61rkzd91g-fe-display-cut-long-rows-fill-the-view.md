---
task_uid: "01m5fzznffpgxzz0h61rkzd91g"
task: "T268"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "layout::inlay_hints_tests::long_row_hint_inlay_tests::long_rows_fill_the_view_with_hints_and_the_inlay_anywhere (163 s) takes under 10 s in a debug build on an idle machine (cargo nextest run -p fe-display long_rows_fill_the_view), still covering the inlay before, between, after and at a hint's offset, and the fills and x/offset round-trip checks"
    required: true
    manual: false
  - id: "C2"
    text: "folds_hidden_rows_after_random_edits_match_a_fresh_map in the fold_rows_across_edits binary (45 s) takes under 10 s (cargo nextest run -p fe-display fold_rows_across_edits)"
    required: true
    manual: false
  - id: "C3"
    text: "layout::cluster_row_tests::proportional_ascii_finds_narrower_clusters (31 s) and long_row::tests::index_matches_measuring_the_prefix (26 s) each take under 5 s"
    required: true
    manual: false
  - id: "C4"
    text: "display_map::pending_wrap_tests::fold_rows_tests::batched_fold_rows_match_per_row_lookups, block_props::no_blocks_equals_layout_frame, block_props::offset_at_round_trip, inlay_props::offset_at_round_trip, inlay_props::shift_by_inlay_width and read_only::long_row_is_sliced (10 to 14 s each) each take under 4 s"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-display passes"
    required: true
    manual: false
---

# fe-display: cut long_rows_fill_the_view_with_hints, fold_rows_across_edits, cluster_row proportional_ascii, long_row index proptest and the block/inlay proptests

Test-only speed-up, user-approved 2026-10-21. Keep what each test proves; shrink inputs or sample the oracle. Measure each with cargo nextest in a debug build; --no-capture shows where the time goes.

- long_rows_fill_the_view_with_hints_and_the_inlay_anywhere: 5 inlay offsets x ~47 scroll positions (step 2,000 over ~95,000 px) = ~235 layouts of a 9,000-char row, and for each it round-trips x_for_offset/offset_for_x over EVERY offset in row.shown(). Raise the scroll step to ~11,000 (the cases that matter are the first, last and mid-row views, and positions on either side of each hint and the inlay: add those few explicitly instead of a dense sweep), and round-trip every 13th shown offset plus both ends. Build the Buffer/DisplayMap once per call as now, but hoist the 9,000-char text if cheap.
- folds_hidden_rows_after_random_edits_match_a_fresh_map: 200 rounds on 4,000 rows, each round rebuilding a fresh DisplayMap and comparing every row (O(rows) x 200, twice for wrapped/unwrapped). Keep every round's random edit, but run assert_matches_a_fresh_map only every 5th round and after the last; keep the 4,000 rows and 300 folds so folds stay dense.
- proportional_ascii_finds_narrower_clusters: the boundary loop does five index queries per grapheme over a 12 KB row, and the slice sweep steps 97. Check every 7th boundary pair (always the first 10 and last 10, so the chunk edges at CHUNK and 2*CHUNK stay covered) and step the slice sweep by 997.
- index_matches_measuring_the_prefix: ProptestConfig::with_cases(48) with ten runs of up to 1,500 pieces, three measure kinds each. Drop to 12 cases; keep runs long enough that rows cross several CHUNK (4 KiB) checkpoints, since that is what the property exercises.
- The five props and fold_rows_tests ones (10 to 14 s): lower proptest cases (e.g. 256 -> 48) or shrink the generated document sizes in doc(); no change to the assertions. batched_fold_rows_match_per_row_lookups: shrink the row count that makes it quadratic if it loops per-row.
Pure fe-display (no GPUI), runs on Linux.

## Implementation notes

Debug nextest, each test alone (load ~38, not idle): long_rows_fill 3.0 s,
proportional_ascii 3.1 s, index_matches_measuring_the_prefix 3.8 s,
batched_fold_rows 0.7 s, block_props no_blocks 1.2 s / offset_at_round_trip 1.4 s,
inlay_props offset_at_round_trip 1.4 s / shift_by_inlay_width 0.8 s,
read_only long_row_is_sliced 1.8 s (32 MiB file cut to 4 MiB), fold_rows_across_edits 4.3 s.
No test stayed superlinear after shrinking.
