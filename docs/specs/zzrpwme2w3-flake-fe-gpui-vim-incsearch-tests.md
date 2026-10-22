---
task_uid: "01m5hy0ytpx9gqbyzzrpwme2w3"
task: "T276"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "turning_vim_off_ends_the_preview and every incsearch_tests test whose assertion needs a preview to land run under the Unhurried guard (one shared helper, T1006's guard moved into it); the deadline tests keep the real limit; no production code changes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui -E 'test(/incsearch_tests/)' --stress-count 30 --retries 0 passes under load (24 yes burners), and cargo nextest run -p fe-gpui passes; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-gpui vim::incsearch_tests::turning_vim_off_ends_the_preview fails under load (100 ms SEARCH_PREVIEW_LIMIT); use T1006's Unhurried guard (Mac T1084)

## Problem

`fe-gpui vim::incsearch_tests::turning_vim_off_ends_the_preview` failed T1074's post-merge test_all on Mac main e6c7fda5 (heavy load) at `crates/fe-gpui/src/vim/incsearch_tests.rs:344`, `assertion failed: shown(&view, cx).2 > 300`.

Same cause as T1006 (`a_preview_opens_the_fold_hiding_its_match`): `VimState::search_preview` runs under the 100 ms wall-clock `SEARCH_PREVIEW_LIMIT`, so on a loaded machine the preview of `far` gives up, nothing is revealed and the view doesn't scroll. T1006 added `fe_vim::set_search_preview_limit` and an `Unhurried` guard (incsearch_tests.rs ~:205-215) to that one test only.

## Change

Test-only, in `crates/fe-gpui/src/vim/incsearch_tests.rs`: move T1006's `Unhurried` guard into a small helper (e.g. `fn unhurried() -> impl Drop`) and use it in `turning_vim_off_ends_the_preview`, in `a_preview_opens_the_fold_hiding_its_match`, and in every other test in the file whose assertion needs a preview to land (scroll, painted match, revealed range). Leave the tests that check the deadline itself (the slow-pattern / CallLimited ones, ~:295-315) on the real limit. No production change.

## Implementation notes

- `unhurried()` in `incsearch_tests.rs` holds T1006's guard; every test that needs a preview to land takes it. Only `a_previews_matches_paint_within_its_time_limit` (call-limited deadline) and `incsearch_off_previews_nothing` (no preview) don't.
- Load validation (C2): with 24 `yes >/dev/null` burners running (load average 30.6), `cargo nextest run -p fe-gpui -E 'test(/incsearch_tests/)' --stress-count 30 --retries 0` gave `30/30 stress run iterations: 30 passed`.
