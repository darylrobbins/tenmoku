---
task_uid: "01m5aq30khcjwqt1yajnbv64bm"
task: "T1006"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Flake: fe-gpui vim::incsearch_tests::a_preview_opens_the_fold_hiding_its_match fails assert!(!folded) under load (incsearch_tests.rs:233); preview search uses the 100ms wall-clock SEARCH_PREVIEW_LIMIT

Test-only fix, same class as T228 (a_previews_matches_paint_within_its_time_limit). In gate run 01m5ansj2kmr0w39bj7rxa8zf7 (fast CI of 6cf2cc6a, main-red) the test failed at incsearch_tests.rs:233 `assert!(!folded(&view, cx))` after a "far" preview, while a neighbouring test ran 82s (heavy load). It passes in isolation, 5x single-threaded and 5x under 10 busy loops. Cause: fe_vim::cmdline::SEARCH_PREVIEW_LIMIT is a 100ms wall-clock deadline; under load the preview search gives up, no match is revealed, the fold stays. Fix as T228 did: give the test's previews a counted (non-wall-clock) budget.
