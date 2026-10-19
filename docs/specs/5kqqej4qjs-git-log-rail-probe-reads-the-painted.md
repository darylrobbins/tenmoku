---
task_uid: "01m58tc4nyvm6h055kqqej4qjs"
task: "T208"
rev: 3
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench unit test workbench::git_commands::git_log::git_log_open_tests::rail::painted::a_shortened_stroke_breaks_the_rail passes: with the shared segment operation's end point deliberately pulled in by one pixel in the test (through the same helper the painter calls), the continuity assertion over an expanded commit's file rows fails, and with the real endpoints it passes; it fails at the base because the probe's segment is computed apart from the painted path"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test workbench::git_commands::git_log::git_log_open_tests::rail::painted::the_probe_and_the_painter_share_one_segment passes: every segment the probe records for a lane equals the endpoints handed to the paint call"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test weakened (git_log_rail_tests.rs only gains the #[path] mod line); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Git Log rail probe reads the painted stroke, so shortening the stroke fails the continuity test (T205 review F1)

## Problem

T205 (Git Log graph rail through expanded file rows) landed on a user findings_override after its third review round, with its criterion C3 relaxed by the lead. One medium finding stayed open (codex, F1, `crates/fe-workbench/src/git_log.rs:~1226`): `rail_cell` paints each lane with `path.move_to/line_to` from `bounds.top()` to `bounds.bottom()` and then reports the same segment to the test probe through a second, separate computation (`probe.segment(ix, top, bottom, ..)`). The rail tests therefore check the probe's endpoints, not the painted path: shortening `path.line_to` at line ~1223 would break the visible rail while the continuity and full-height assertions still pass.

## Change

Route painting and probing through one shared segment operation that consumes the same endpoints (for example one helper that takes `(top, bottom, color)`, paints the path and records the segment), so a shortened stroke changes what the probe sees. Production code stays behaviour-identical.

New tests go in a new file `crates/fe-workbench/src/git_log_rail_painted_tests.rs`, declared from `git_log_rail_tests.rs` with one `#[path = "git_log_rail_painted_tests.rs"] mod painted;` line (a test file declares it, so the new module is visible at the base).
