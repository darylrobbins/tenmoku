---
task_uid: "01m5k9m8qkbwqgtkq9kw89ja4d"
task: "T1085"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "The test no longer depends on wall time: the preview of 'far' is given an effectively unlimited search budget (or the preview's outcome is asserted as work done), so a loaded machine cannot make it find no match."
    required: true
    manual: false
  - id: "C2"
    text: "No 'unhurried' guard or skip-on-slow branch is added to the test."
    required: true
    manual: false
  - id: "C3"
    text: "Proof: 50 consecutive runs of the test (cargo nextest, --retries 0) pass while a parallel full workspace build or other CPU load keeps load above 30 (no scripts/load.sh exists; use e.g. yes/xargs -P CPU burners)."
    required: true
    manual: false
---

# Flake: fe-gpui incsearch_fold_tests a_header_edit_and_a_later_edit_keep_the_fold fails under load (search preview hits its wall-clock limit, fold never opened)

Seen once in a macOS stack gate (gate-stack-52 run1, load 30-84), 0.35 s run:
crates/fe-gpui/src/vim/incsearch_fold_tests.rs:50 (the assert in the shared preview_then helper)
assertion left == right failed; left: [(390, 420)], right: []
i.e. after view.preview_search("far") the fold 390..=420 was still folded. Passed twice in isolation and on a full rerun.

Suspected cause: preview_search -> fe-vim cmdline.rs search_preview runs the match under Deadline::after(SEARCH_PREVIEW_LIMIT.min(search::time_limit())). Under heavy load the search is cut off by the wall clock, finds no match, so open_folds_at never runs and the fold stays. The sibling tests share preview_then, but this one was the one hit.

Fix direction (test-only): do not race the wall clock. There is a PREVIEW_LIMIT_OVERRIDE atomic in fe-vim cmdline.rs; if reachable from fe-gpui tests set it to a large value for the test (restoring after), or otherwise make the preview deterministic. Assert on the work (match found, fold opened), never add a guard that skips under load. Chore: test-only, no regression test.

## Implementation notes

`editor()` in `incsearch_fold_tests.rs` sets `fe_vim::set_search_preview_limit(Some(600 s))` and `fe_vim::search::set_time_limit(600 s)`; the preview takes the smaller of the two, so both are raised.

C3 proof: 50 consecutive `cargo nextest run -p fe-gpui a_header_edit_and_a_later_edit_keep_the_fold --retries 0` runs, all passed, under `yes` burners at 2x ncpu started 20 s earlier. Load1 sampled before every run: min 35.69 (run 1), 39.50, 40.42, ... 161.74, 159.68 (run 50), so above 30 throughout.
