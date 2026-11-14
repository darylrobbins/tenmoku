---
task_uid: "01m7c4yvmcmv9wybvrhqychn2y"
task: "T380"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Every test named in the spec runs in under 10 s alone in a debug build on the host that measured it, or is in .config/nextest.toml's slow set with a reason; before and after times recorded in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim --profile ci passes"
    required: true
    manual: false
---

# fe-vim: size down the long-row tests that take minutes under load (caret_window, insert keys replay, replace_regional), so the Linux gate's test stage finishes again (Mac T1838)

Reported by the argon lead (landing.md, batch 93): the Linux gate on mac/main no longer completes its test stage inside 30 minutes. About ten fe-vim tests run for over 120 s each under the gate's parallel load; alone on an idle host `replace_regional::counted_replace_of_regional_indicators_caps_at_max_put` takes 67 s (mac/main 08065e78). The earlier full test stage took about 10 minutes. Cause: the HeadlessVim long-row baseline, a per-key cost on MB-long rows.

Tests named in the report:
- crates/fe-vim/tests/caret_window.rs: a_visual_counted_l_goes_over_the_windows_edge, columns_far_along_a_huge_row_are_the_rows_own, first_non_blanks_past_a_long_indent_land_as_past_a_short_one, graphemes_joining_across_the_windows_edge_stay_whole, graphemes_joining_across_the_windows_left_edge_stay_whole
- crates/fe-vim/src/insert/keys_tests.rs: counted_replay_of_an_oversized_register_is_refused and its dot_replay twin
- crates/fe-vim/tests/replace_regional.rs: counted_replace_of_regional_indicators_caps_at_max_put

Fix: size each test's row or count down to the smallest that still proves its claim (a window edge needs a row a little wider than the window, not MB; a cap needs a count just past the cap), keeping every assertion. Measure each before and after (alone, debug, `cargo nextest run -p fe-vim <name>`) and record the numbers in the decision file. A test whose claim truly needs a huge row moves to the slow set in .config/nextest.toml's default-filter (post-merge only), with its reason; keep that to as few as possible. Find any other fe-vim test over 10 s alone with `cargo nextest run -p fe-vim` and treat it the same way.
