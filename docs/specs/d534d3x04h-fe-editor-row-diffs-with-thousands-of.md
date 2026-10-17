---
task_uid: "01m55bq4gm84vmssd534d3x04h"
task: "T136"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor integration test interleaved_changes_diff_in_linear_time: 20,000 rows with every other row changed give 10,000 TextDiff hunks (applying all of them gives the right text) and 10,000 Modified row_changes() through EditorModel::set_diff_base, each in under 2 s; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor integration test small_regions_keep_histogram_hunks: a 600-row region on which Histogram and Myers differ diffs exactly as Algorithm::Histogram plus postprocess_lines"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor integration test large_regions_use_myers: a region over HISTOGRAM_WORK (min(b, a) * (b + a) \u003e 1 \u003c\u003c 21 after stripping the common prefix and suffix) diffs exactly as Algorithm::Myers plus postprocess_lines"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-editor, -p fe-gpui and -p fe-workbench pass with their existing diff and row-change tests unchanged; fe-git blame.rs, fe-workbench git_blame.rs and fe-buffer proposed.rs are untouched and keep Histogram"
    required: true
    manual: false
  - id: "C5"
    text: "The decision file records release timings, base and candidate, of 20k and 40k rows every other changed and 100k rows with 200 scattered edits; each candidate case's diff step (row_diff, as the Mac card's table measured imara directly) is at most 5 ms. InternedInput::new (row split and hashing, unchanged from the base and outside the scope) is timed and recorded separately, not bounded"
    required: true
    manual: false
---

# fe-editor: row diffs with thousands of interleaved changes take milliseconds, not seconds, by falling back from Histogram to Myers past a work bound (Mac T812)

Filed on argon from Mac T812 (card ~/coordination/argon-cards/zzl/T812.json rev 1; note zzl, perf track). Mac task uid 01m55b5jw7phnp743fh8470apc. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced). Title the host commit "... (Mac T812)".

## Problem

`fe_editor::row_changes::diff_rows` (the editor's change bar, `EditorModel::row_changes`) and
`TextDiff::new` (diff views, hunk staging, `EditorModel::revert_changes`) both diff rows with imara-diff
0.2's `Algorithm::Histogram`. Histogram is quadratic on many interleaved changes: each round of its loop
(`imara-diff-0.2.0/src/histogram.rs`, `Histogram::run`) re-populates the whole remaining left region and
scans the whole remaining right region, then splits off as little as one common row. With every other row
changed that is one round per hunk.

Measured on the Mac (M3 Max, release, load ~35, 2026-10-17, imara-diff 0.2.0 directly, rows `"{i}"` vs
`"x{i}"` on odd rows):

| Case | Histogram | Myers (imara, non-minimal) |
|---|---|---|
| 20,000 rows, every other changed (10,000 hunks) | 417 ms | 0.4 ms |
| 40,000 rows, every other changed (20,000 hunks) | 1,736 ms | 0.7 ms |
| 100,000 unique rows, 200 scattered one-row edits | 31 ms | 1.1 ms |

T533 measured 6 s at 40k and 27 s at 80k rows in release, and ~59 s at 40k in a debug build (imara isn't
optimised in `[profile.dev]`). `EditorModel::row_changes` runs `diff_rows` synchronously for each new buffer
version, from the change bar's paint (`crates/fe-gpui/src/view/change_bar.rs`), for any text up to
`MAX_DIFF_BYTES` (5 MiB). So typing in a file with a git diff base and many changes against it blocks the
UI thread per keystroke: 31 ms in a 100k-row file with 200 changed rows, seconds after a reformat. The plan's
M6 target is p99 keystroke under 8 ms.

## Change

In `crates/fe-editor/src/row_changes.rs`, add one `pub(crate) fn row_diff(input: &InternedInput<&str>) ->
Diff` that both `diff_rows` and `TextDiff::new` (`crates/fe-editor/src/text_diff.rs`) call in place of
their `Diff::compute(Algorithm::Histogram, &input)`; each still calls `postprocess_lines` after it.

- Strip the common prefix and suffix of `input.before` and `input.after` (token comparison, O(n)). Let `b`
  and `a` be the rows left on each side.
- Histogram's total work is bounded by (rounds) x (region): each round consumes at least one row of each
  side, so at most `min(b, a) + 1` rounds, each scanning at most `b + a` rows. Use `Algorithm::Histogram`
  when `min(b, a) as u64 * (b + a) as u64 <= HISTOGRAM_WORK`, a `const HISTOGRAM_WORK: u64 = 1 << 21`
  (about 4 ms worst case at the ~2 ns per row-visit measured above); otherwise `Algorithm::Myers`
  (non-minimal: its preprocessing drops rows only one side has, and its heuristics bound pathological
  inputs).
- The bound is on the stripped region, so a single edit in a 5 MiB file still uses Histogram, and every
  diff in today's tests (all far below 1,000 rows of region) keeps its exact hunks.
- Doc comments on `diff_rows`, `TextDiff::new` and `row_diff` say which algorithm runs when.
- `crates/fe-git/src/blame.rs`, `crates/fe-workbench/src/git_blame.rs` and `crates/fe-buffer/src/proposed.rs`
  keep Histogram: out of scope (blame mirrors git blame's own matching).

New integration test file `crates/fe-editor/tests/row_diff_cost.rs`:

- `interleaved_changes_diff_in_linear_time` (the regression test): 20,000 rows, every other row changed.
  `TextDiff::new(left, right)` has 10,000 hunks, `apply` of all of them gives `right`, and it returns in
  under 2 s; an `EditorModel` over `right` with `set_diff_base(Some(left))` gives 10,000 `Modified`
  `row_changes()` in under 2 s. At base this takes ~15 s in the debug test build; with the fix, well under
  0.1 s. The 2 s bound is wide on both sides so load can't flip it.
- `small_regions_keep_histogram_hunks`: for an input with a 600-row region where Histogram and Myers give
  different hunks (find one; e.g. repeated `}` and blank rows around moved blocks), `TextDiff::new`'s hunks
  equal `Diff::compute(Algorithm::Histogram, ..)` + `postprocess_lines` computed in the test.
- `large_regions_use_myers`: for a region over the bound, `TextDiff::new`'s hunks equal imara's `Myers` +
  `postprocess_lines` on the same input.

Record in the lane's decision file a release timing of the three table cases above, base vs candidate.
