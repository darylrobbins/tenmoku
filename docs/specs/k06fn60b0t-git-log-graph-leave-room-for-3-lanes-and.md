---
task_uid: "01m78dyvdcfx2yvxk06fn60b0t"
task: "T361"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::git_log_graph_width (cargo nextest run -p fe-workbench) pass: a_linear_history_puts_every_summary_at_three_lanes, a_row_reaching_a_fifth_lane_pushes_only_its_own_summary_out, an_expanded_commits_file_rows_and_rail_follow_that_commits_width, the_file_letters_room_at_the_right_edge_is_unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "git_log_tests, git_log_rail_tests, git_log_rail_painted_tests and git_log_expanded_tests still pass; if the audit git_log scene or screenshot goldens move, they are regenerated"
    required: true
    manual: false
---

# Git log graph: leave room for 3 lanes, and push a row's text out only as far as that row's own lanes reach (Mac T1745)

# Git log graph: text no longer justified at the widest lane

User report (2026-11-12): "If there are a lot of branches, the text appears very far over. It looks at the maximum size of how far it goes out, and that's where everything is justified. We should aim to put it closer when we can and push it out more as we need to. Every one of the commits may not be at exactly the same level, but leave room for 3 branches by default, and anything more than that will push out the headline for that commit."

Today every row's graph cell is `self.lanes` wide (git_log.rs: field `lanes` ~:1287, `self.lanes = self.lanes.max(lanes(&row))` ~:1657, passed to graph_cell and rail_cell ~:2212-2270; LANE_WIDTH 12 px ~:167).

## Do
- Per row, the cell is `max(3, lanes(row)) * LANE_WIDTH` wide, where lanes(row) is that row's own reach (the `lanes` fn ~:387, counting pass-through lines in both halves). Rows may differ; there is no global maximum. Three lanes of room by default; more only pushes that row's summary out.
- An expanded commit's file rows and rail follow that commit's width, so the letter column keeps lining up under its commit. Keep the file letter's right-edge room (module doc ~:33).
- Update the module doc. The rail_cell/graph_cell canvas paints must still clip.

## Tests (new file)
`src/tests/git_log_graph_width.rs`, declared in `src/tests/mod.rs`, using git_log_tests.rs's helpers (`graph`, log window) with a repo built by the fe-git test helpers: names in C1.

Platform: Linux-OK.
