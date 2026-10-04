---
task_uid: "01m417z4c57bw7btq540szs3yk"
task: "T23"
rev: 2
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax: syncing one multi-edit batch applies one InputEdit per edit with exact bytes and points, and a two-batch sync still applies the merged span (test syntax::input_edit_tests::one_batch_gives_one_edit_per_cursor)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax: random single batches of 2-4 edits spread from first to last row, synced one batch at a time, give the same tree and highlights as a parse from scratch (test syntax::input_edit_tests::multi_cursor_reparse_matches_a_full_parse); tests::incremental_reparse_matches_a_full_parse still passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo bench -p fe-syntax -- two-cursor runs to completion, and in the same run a top-and-bottom two-cursor keystroke on 10k lines of Rust synced as one batch (one InputEdit per cursor) takes at most half the time of the same two edits synced as two batches (the merged-span path). Both times, the ratio and the load average are recorded in the decision file beside the 0.87 ms one-cursor figure. No absolute-time threshold: on this loaded machine perf criteria are same-session ratios (the user's rule)"
    required: true
    manual: false
---

# Apply one tree-sitter edit per cursor when a single batch separates the tree from the snapshot

Source: Paddock T-11 (`3bdf21b1-d12c-4be6-a447-c855db57a1e0`), P3, draft (needs the user's approval).

## Problem

`input_edit` (`crates/fe-syntax/src/syntax.rs`) folds every batch since the last sync into one
`InputEdit` hull, because tree-sitter needs rows and columns and only the first and last texts are
known. For a single multi-cursor batch with cursors near the top and bottom of a file, that hull covers
the whole file, so the reparse treats everything in between as edited. PERF.md: a one-cursor keystroke
reparse on 10k lines of Rust takes 0.87 ms against the view's 1 ms `SYNC_PARSE_BUDGET`
(`crates/fe-gpui/src/view.rs`); a whole-file hull blows that budget and the parse goes to the
background, so colours lag.

The common case is exactly one batch between the tree's version and the snapshot: one keystroke with
several cursors.

## Change

- Replace `input_edit` with a function returning the edits to apply in order (`Vec<InputEdit>`), and
  apply each with `Tree::edit` at both call sites (`Syntax::edit` and `Syntax::finish`).
- When `new.edits_since(from)` yields exactly one batch, emit one `InputEdit` per `fe_buffer::Edit`,
  applied first to last. Edits in a batch are sorted with non-overlapping `old` ranges and `new` ranges
  in post-batch coordinates (`crates/fe-buffer/src/edit.rs`), so after the earlier edits are applied,
  edit k starts at `new.start`:
  - `start_byte = new.start`, `new_end_byte = new.end`, `old_end_byte = new.start + old.len()`;
  - `start_position` and `new_end_position` from the new text's points;
  - `old_end_position` = start point plus the old text's row/column delta between `old.start` and
    `old.end` (same row: add the column difference; otherwise add the row difference and take
    `old.end`'s column).
- With more than one batch, keep the merged hull exactly as today.
- Tests go in a new module declared from `syntax.rs` as `#[cfg(test)] mod input_edit_tests;`
  (`crates/fe-syntax/src/syntax/input_edit_tests.rs`), so they can call the private function.
- Add a bench case `reparse two-cursor keystroke` to the Rust/JSON/Markdown groups in
  `crates/fe-syntax/benches/syntax.rs`: the same inside-a-word keystroke as `reparse keystroke`, typed
  at one random row in the first 1% and one in the last 1% of the file in a single `buffer.edit`
  batch, deleted again untimed.

`tests::incremental_reparse_matches_a_full_parse` already makes 1–3 edits per batch and syncs 1–3
batches at a time, so it covers both paths randomly; the new test forces the single-batch,
far-apart case on every step.
