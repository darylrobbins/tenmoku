---
task_uid: "01m565kqsmff6t9gkznp9gt31m"
task: "T194"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test git::tests::conflicts::compare::compare_opens_the_two_sides_in_a_diff passes: git.compare_conflict inside a Rust file's conflict opens a focused diff tab whose left text is exactly the current side and right the incoming side, both read-only, titled '\u003cname\u003e: Current Changes' and '\u003cname\u003e: Incoming Changes', both highlighted as Rust, the file's buffer unchanged; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test git::tests::conflicts::compare::the_fourth_lens_item_compares passes: each conflict's lens has four items, the last 'Compare Changes' invoking git.compare_conflict with its row; git.compare_conflict is a row of the typed-ID table; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test weakened: the only edits to existing tests are added rows or the fourth lens item added to what they expect (git_conflicts_tests.rs only gains the #[path] mod line; the typed-ID table only gains rows; git_conflicts_lens_tests.rs's expected titles and commands gain the fourth 'Compare Changes' / git.compare_conflict item); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Compare a merge conflict's current and incoming sides in a read-only diff view, from Git: Compare Conflict Changes and a fourth Compare Changes action above each conflict (Mac T864)

## Problem

VS Code's fourth conflict action, Compare Changes (`merge-conflict.compare`), opens a diff editor of the conflict's
current side against its incoming side, titled after the file. fastedit has the diff view
(`fe_gpui::DiffView::new(file_name, left, right, cx)`, `crates/fe-gpui/src/diff_view.rs:196`, opened by the
workbench's `new_diff` / `show_diff` in `crates/fe-workbench/src/git_diff.rs:785-860`) but nothing to feed it a
conflict's two sides. Gap audit 2026-10-17 (user-approved: merge conflicts first; the lead's brief names
"accept current/incoming/both/compare").

## Change

1. `crates/fe-workbench/src/git_diff.rs`: `pub(crate) fn open_text_compare(&mut self, name: SharedString, left:
   (SharedString, String), right: (SharedString, String), language_of: &Path, window, cx)`: a diff view of two
   in-memory texts, both sides read-only (`DiffSide { read_only: true, .. }`), each highlighted in the language of
   `language_of` (the conflicted file), opened and focused in the active pane as the other diffs are, through the
   same `show_diff` path. No file is written and no git command runs.
2. `crates/fe-workbench/src/git_conflicts.rs`: `git.compare_conflict` "Git: Compare Conflict Changes", taking the
   optional `{ "row": N }` args as the accept commands do (else the conflict at the caret, with the same "Editor
   cursor is not within a merge conflict" message): opens `open_text_compare` with the left side the conflict's
   current text titled `<file name>: Current Changes` and the right its incoming text titled `<file name>: Incoming
   Changes` (VS Code's `… : Current Changes ⟷ … : Incoming Changes`). `ConflictLenses` adds a fourth item,
   "Compare Changes" → `git.compare_conflict` with the row.
3. `crates/fe-workbench/src/workbench_commands.rs`: the method for `git.compare_conflict`, through `in_editor`; its
   row in `crates/fe-workbench/src/typed_command_ids_tests.rs`.
4. Tests: new unit-test file `crates/fe-workbench/src/git_conflicts_compare_tests.rs`, declared by
   `#[path = "git_conflicts_compare_tests.rs"] mod compare;` at the end of `crates/fe-workbench/src/git_conflicts_tests.rs`:
   - `compare_opens_the_two_sides_in_a_diff`: on a Rust file with a conflict, `git.compare_conflict` with the
     caret inside opens a focused diff tab whose left side's text is exactly the current side and right side's the
     incoming side, both read-only, titled as above, both highlighted as Rust, and the file's buffer unchanged;
   - `the_fourth_lens_item_compares`: each conflict's lens has four items, the last "Compare Changes" with its row.
