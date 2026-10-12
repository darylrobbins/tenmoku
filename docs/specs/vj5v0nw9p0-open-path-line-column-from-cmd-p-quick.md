---
task_uid: "01m4pzqn6gzm851bvj5v0nw9p0"
task: "T33"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test quick_open::line_tests::suffix_is_split: the queries main.rs:3 and main.rs:3:5 list the same rows and status as main.rs; main.rs:, main.rs:x and main.rs:0 match whole, as today"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test quick_open::line_tests::opens_at_the_line: confirming main.rs:3 opens main.rs with the caret at (2,0) and row 2 visible; main.rs:3:5 at (2,4); main.rs:99 in a 10-row file at the last row; with main.rs already open in a tab, that tab is activated and its caret moved"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Open path:line[:column] from Cmd-P: quick open matches the path part and opens the file with the caret at that line and column (Mac T452)

Filed on argon from Mac T452 (card ~/coordination/argon-cards/T452.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12-argon-h.md). Mac task uid 01m4pz59gat2dys4tjae0v9rp9. Title the host commit "... (Mac T452)".

## Problem

Cmd-P (`crates/fe-workbench/src/quick_open.rs`) fuzzy-matches the whole query against paths, so pasting
`src/main.rs:42` or `view.rs:120:9` from compiler output, a stack trace or ripgrep finds nothing useful, and a
match opens the file at its top (`QuickOpen::confirm`, :333, calls `pane.open`). M3 Browse (plan line 108) is the
read path; going from a reported location to the code is its most common jump. No card covers it (2026-10-11
scoping pass).

## Change

In `crates/fe-workbench/src/quick_open.rs` only:

- A query ending in `:N` or `:N:C` (digits only, N ≥ 1, C ≥ 1) is split before matching: the part before is
  matched exactly as a query today, so the rows and the status line are the same as for that part alone. Anything
  else (a trailing `:`, `:x`, `:0`) is matched whole, as today.
- Confirming a row with a suffix opens the file as today but through `Pane::open_at` with
  `Reveal { row: N - 1, columns: c..c }` (`crates/fe-workbench/src/pane.rs` :569), where `c` is `C - 1`, or 0
  without a column. `Reveal`'s columns are bytes of the row, so `C` counts bytes, as ripgrep's `--column` and
  compilers that report byte columns do; `Reveal::select` already widens to a char boundary and clamps a row or
  column past the end. A file already open in a tab is activated and its caret moved.

## Tests

New file `crates/fe-workbench/src/quick_open_line_tests.rs`, declared from quick_open.rs with `#[path]`, using the
existing quick-open test helpers.
