---
task_uid: "01m5a64r5rpdkcdab81597f2q8"
task: "T228"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::incsearch_tests::a_previews_matches_paint_within_its_time_limit passes 20 of 20 runs, repeated and under cargo nextest -j high load, and no longer compares an elapsed time against a fixed bound"
    required: true
    manual: false
  - id: "C2"
    text: "The other tests in vim::incsearch_tests keep passing unchanged; cargo nextest run -p fe-gpui passes; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Flake: fe-gpui vim::incsearch_tests::a_previews_matches_paint_within_its_time_limit races its own deadline under load (left None, right Some(1..2))

## Problem

`fe-gpui vim::incsearch_tests::a_previews_matches_paint_within_its_time_limit` (`crates/fe-gpui/src/vim/incsearch_tests.rs:~270-290`, T199's test) failed once at the helium sync gate (2026-10-19, load average 12-18) with `assertion left == right failed: left: None, right: Some(1..2)` at line 285, then passed on the rerun of the identical tree. It builds 40 rows of 200,000 `a`, previews the pattern `a.*z\|a` under a 60 s test limit and asserts that the match the line lands on (`Some(1..2)`) is found at once while the rest of the row's matches take long (`highlight_ran_out`) and the whole call takes under 5 s.

The preview ran out its deadline before the first match was reported under load: the test measures the machine, not the behaviour.

## Change

Make the test deterministic: do not depend on how fast the first match is found. For example, give the preview a deadline-controlled seam (the `TEST_LIMIT` it already sets, or a counting probe of how many rows were scanned) and assert on the work done (the first match is reported before the slow part is attempted, and the slow part is cut at the limit), with no wall-clock bound; or scan a size for which the first match cannot miss its deadline on a loaded machine. Production code stays as it is unless the seam needs a small hook.

## Implementation notes

The test now sets a regex-call budget (`fe_vim::search::set_call_limit`) instead of timing the preview, and asserts the landing match `Some(1..2)`, `highlight_ran_out`, and that the highlight made fewer calls than the row has matches. No elapsed-time assertion remains; production code is untouched. The preview's own 100 ms clock still exists in fe-vim, so rows are 5,000 `a` (a single regex call is microseconds, nowhere near 100 ms under load).

Evidence, 20 of 20 under load: `cargo nextest run -p fe-gpui -E 'test(a_previews_matches_paint_within_its_time_limit)' --stress-count 20 -j 32` passed 20/20, and `-E 'test(/^vim::/)' --stress-count 20 -j 32` (112 tests x 20, heavy concurrency) passed 20/20.

Inlay hints, semantic tokens and multi-caret: the landing scan is fe-vim's `nth_match`, over the text from the caret and independent of the view; the highlight scans the frame's displayed rows. Neither depends on those features (inlay hints were already in the base).
