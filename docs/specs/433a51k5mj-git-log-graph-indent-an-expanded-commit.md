---
task_uid: "01m7j14vrvez6xw7433a51k5mj"
task: "T403"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "An expanded commit's file rows (and its Loading/error note row) start their icon/text FILE_INDENT = 16 px further right than today: icon left \u003e= rail right + 8 px gap + 16 px; tests::git_log_graph_width::an_expanded_commits_file_rows_are_indented_past_its_text (cargo nextest run -p fe-workbench) passes and fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Commit rows do not move, the file rows' rail keeps its commit's graph-cell width and paints its lanes unbroken, and the 12 px letter column stays at one right edge: tests::git_log_graph_width (all four existing tests), git_log_file_rows_tests, git_log_rail_tests, git_log_rail_painted_tests, git_log_expanded_tests and git_log_tests pass"
    required: true
    manual: false
  - id: "C3"
    text: "git_log.rs's module doc states the indent"
    required: true
    manual: false
---

# Git log graph: indent an expanded commit's file rows 16 px past the commit text so files read apart from commits (Mac T1893)

## Ask

The user, verbatim: "In the graph view, let's indent the files a bit when we expand a commit. Keeping them at the same indent makes it too difficult to differentiate what's a file."

Today in the bottom panel's "Graph: main" tab (`crates/fe-workbench/src/git_log.rs`), an expanded commit's file rows are its rail (`rail_cell`, the same width as the commit's graph cell, `LANE_WIDTH` x `cell_lanes`) followed by `gap_2` (8 px) and the file icon. A commit row is its graph cell followed by `gap_2` and then its short SHA (after any ref pills). So a file's icon starts at the same x as the SHA of a commit without pills, and files read as commits.

## Change

- Add a `FILE_INDENT: Pixels = px(16.)` constant next to `LANE_WIDTH` and put it between a file row's rail and its icon, so the icon starts at rail right + 8 px gap + 16 px. Add it as a left margin on the icon cell or as padding, **not** as a new flex child inside `before`. `file_cell`'s `probe.bounds(ix, 0, &drawn[1..])` maps drawn children to cells by index.
- Give the `Loading…` and error note rows under an expanded commit (the `Line::Note` arm) the same indent, so everything under a commit lines up.
- No other changes. The rail keeps its width and keeps painting its lanes through the file rows unbroken. Commit rows don't move. The letter keeps its 12 px room at the right edge, and the shrink order (directory, name, icon and counts, never the letter) stays.
- Update the module doc's "listed under it, indented" paragraph to give the amount.

## Tests

Add `an_expanded_commits_file_rows_are_indented_past_its_text` to `src/tests/git_log_graph_width.rs`, which `src/tests/mod.rs` already declares. Do not add a new file, because T1852 holds `tests/mod.rs`. Use the `branchy()` fixture. Expand a commit and read `rows_seen()`. Assert that the file row's icon cell (`cells[ix][0].bounds.left()`) is at least the row's rail right edge (`graph_cells[ix].right()`) + 24 px. Also assert that the rail's width still equals the commit's graph cell width. At the base the gap is 8 px, so the test fails there.

No screenshot golden shows an expanded commit. `examples/screenshots/golden/*` don't include the git log, and the `git_log` and `git_log_card` audit scenes only open the log and a card, so their probes don't land on file rows. If the audit moves anyway, re-point its probes.
