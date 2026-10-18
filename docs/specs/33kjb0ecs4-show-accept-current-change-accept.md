---
task_uid: "01m565kkfnerkc1133kjb0ecs4"
task: "T193"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test git::tests::conflicts::lenses::each_conflict_has_its_actions_above_it passes: an opened file with two conflicts has one ConflictLenses provider answering a lens per conflict at its start row with the three titles, commands and { row } args, and the start and end marker rows carry '(Current Change)' and '(Incoming Change)' after-line labels; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test git::tests::conflicts::lenses::a_lens_action_resolves_its_conflict passes: dispatching the second conflict's Accept Incoming Change invocation from the editor with Source::Mouse resolves only that conflict, leaving one lens on the first; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit test git::tests::conflicts::lenses::lenses_follow_edits passes: two rows inserted above the first conflict move both lenses down two rows; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_conflicts_tests.rs only gains the #[path] mod line); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show Accept Current Change, Accept Incoming Change and Accept Both Changes as clickable actions above each merge conflict, with the marker lines labelled Current Change and Incoming Change (Mac T862)

## Problem

VS Code shows CodeLens-style actions on a row of their own above each conflict's `<<<<<<<` line ("Accept Current
Change | Accept Incoming Change | Accept Both Changes | Compare Changes") and labels the marker lines
"(Current Change)" and "(Incoming Change)". fastedit gets the accept commands from the card this depends on
(`git.accept_current` / `_incoming` / `_both`, each taking `{ "row": N }`) and clickable rows above a line from the
lens-rows card (`fe_gpui::LensProvider`, `EditorView::add_lens_provider`). This card joins them. Compare Changes
is its own card after this one. Gap audit 2026-10-17 (user-approved: merge conflicts first).

## Change

In `crates/fe-workbench/src/git_conflicts.rs`:

1. `ConflictLenses`, a `fe_gpui::LensProvider` caching by snapshot version with the same `MAX_SCAN_BYTES` and
   `<<<<<<<` pre-check as `ConflictBands`: one `Lens` per conflict at its start marker's row, items in VS Code's
   order: "Accept Current Change" → `git.accept_current`, "Accept Incoming Change" → `git.accept_incoming`,
   "Accept Both Changes" → `git.accept_both`, each invocation's args `{ "row": <start marker row> }`.
2. `ConflictBands` adds an `AfterLine` decoration `(Current Change)` on each start marker's row and
   `(Incoming Change)` on each end marker's row, in `Tint::Muted`.
3. `crates/fe-workbench/src/git.rs` `GitState::track`: each newly tracked editor gets one `ConflictLenses` next to
   its `ConflictBands`; a closed editor's are dropped with it.
4. Tests: new unit-test file `crates/fe-workbench/src/git_conflicts_lens_tests.rs`, declared by
   `#[path = "git_conflicts_lens_tests.rs"] mod lenses;` at the end of `crates/fe-workbench/src/git_conflicts_tests.rs`:
   - `each_conflict_has_its_actions_above_it`: a file with two conflicts opened: the editor has exactly one
     `ConflictLenses` among its lens providers, which answers one lens per conflict at its start row with the three
     titles and invocations above (row args included), and the start and end marker rows carry the two after-line
     labels;
   - `a_lens_action_resolves_its_conflict`: dispatching the second conflict's "Accept Incoming Change" invocation
     as a click would (`RunCommand` from the editor, `Source::Mouse`) leaves the second conflict's incoming text,
     the first untouched, and afterwards one lens remains, on the first conflict's row;
   - `lenses_follow_edits`: inserting two rows above the first conflict moves both lenses down two rows.
