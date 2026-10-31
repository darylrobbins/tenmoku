---
task_uid: "01m5zzbx6cd2w7mdsgj6hw3y1n"
task: "T1208"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "replace_all_over_many_empty_matches no longer depends on the wall-clock search budget under a loaded parallel suite (e.g. a test-only budget or a smaller input that keeps the many-empty-matches case)"
    required: true
    manual: false
  - id: "C2"
    text: "the test passes 20 runs in a row under load (nextest with the full fe-workbench suite running alongside)"
    required: true
    manual: false
---

# Flake: fe-workbench find_bar::replace_tests::replace_all_over_many_empty_matches fails under load (Search took too long after 3.5 s)

T1193's gate (run 01m5zy4m6wmezwgsz1a2ma6n10, candidate integrated 6efcdde on main 59a3bfefc) failed only on fe-workbench find_bar::replace_tests::replace_all_over_many_empty_matches: panic at tests/find_bar.rs:138, got Some(("Search took too long", true)) after 3.5 s while the suite ran in parallel; 2511/2512 passed. Rerun alone in the lane: passes in 1.9 s. The find bar's search has a wall-clock budget; under load the many-empty-matches replace exceeds it. Fix the test (not the production budget) unless the production budget is wrong for a real user. Test-only fix: prove with repeated runs under load, no regression test.
