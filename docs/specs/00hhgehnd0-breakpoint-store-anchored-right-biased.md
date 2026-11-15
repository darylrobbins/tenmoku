---
task_uid: "01m7eqkqwd01ta3a00hhgehnd0"
task: "T656"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test tests::debug_breakpoints::a_breakpoint_follows_rows_inserted_and_deleted_above_it passes (cargo nextest run -p fe-workbench debug_breakpoints)"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_breakpoints::a_newline_at_the_rows_start_moves_it_down_with_its_text passes, both by O on that row and by Enter at column 0"
    required: true
    manual: false
  - id: "C3"
    text: "Tests tests::debug_breakpoints::deleting_its_row_moves_it_to_the_surviving_row and tests::debug_breakpoints::joining_its_row_to_the_one_above_moves_it_there pass, and two on one row merge with the first's settings"
    required: true
    manual: false
  - id: "C4"
    text: "Tests tests::debug_breakpoints::a_restart_restores_breakpoints and tests::debug_breakpoints::an_unknown_watches_list_survives_a_write pass, the file at \u003cstate dir\u003e/debug/\u003clayout_key(root)\u003e.json written atomically and debounced"
    required: true
    manual: false
  - id: "C5"
    text: "Test tests::debug_breakpoints::a_file_reloaded_from_disk_keeps_its_breakpoints_on_the_right_rows passes"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Breakpoint store: anchored, right-biased breakpoints per project, persisted in the state dir with the watch list (debugger D5) (Mac T1846)

# Breakpoint store: anchored, right-biased breakpoints per project, persisted in the state dir with the watch list (debugger D5)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

Breakpoints must stay on their code as the user edits, survive restarts, and never
land in the project's own files.

## What to build (§5 "The breakpoint model"; §11 D5)

- `fe_dap::Breakpoint { path, line, enabled, condition, hit_condition, log_message }`
  (the last three carried and persisted only; D17 edits and sends them).
- `BreakpointStore` in fe-workbench, one per root: an `fe_buffer::Anchor` with
  **`Bias::Right`** at the row's start while a file is open (§5 explains why right
  bias), rows written back on save and close, collapsing and merging on deleted or
  joined rows, the empty-row known limit (§5).
- Persistence in `<state dir>/debug/<layout_key(root)>.json`
  (`kiln::core::layout::layout_key`), atomic and debounced, with a `breakpoints` list
  and a `watches` list that D5 keeps unchanged and D9 fills.
- An API the session (D7), the gutter (D6) and the commands (D8) use: toggle at a
  row, enable/disable, remove, remove all, the file's set for `setBreakpoints`, and a
  change notification.

## Not in this card

Drawing (D6), sending to an adapter (D7), commands and keys (D8), conditions (D17).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).

## Implementation notes

- `fe_dap::Breakpoint` in `crates/fe-dap/src/breakpoint.rs`; `BreakpointStore`
  in `crates/fe-workbench/src/debug_breakpoints.rs`, wired from `lib.rs`'s
  `init` (no change to `workbench.rs`): `toggle`, `set_enabled`, `remove`,
  `remove_all`, `breakpoints`/`enabled` (a file's set), `all`, `files`,
  `watches`, and `BreakpointEvent::{Changed, Moved, Saved}`.
- Design calls: `docs/decisions/00hhgehnd0-breakpoint-store.md`.
- Strings: `workbench-debug-breakpoints-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the state file's read and write
  failures).
- Review round 1: each breakpoint keeps its line on disk (what's written) apart
  from its place in each open buffer, set from saves through the buffer's
  events; `breakpoints_in`, `toggle_in`, `set_enabled_in` and `remove_in` take
  a buffer for a file open in two windows apart. Batches replacing rows with
  rows (reloads, their undo and redo) map by line diff; failed writes and
  writes still running are written again at quit. Regressions in
  `tests/debug_breakpoints.rs`.
- Review round 2: a save sets the file on disk for every open buffer of it;
  a save landing after more edits uses the rows recorded at the version saved;
  `enabled`, `all` and the new `on_disk` read lines on disk; a failed write
  with newer changes queued is tried again.
