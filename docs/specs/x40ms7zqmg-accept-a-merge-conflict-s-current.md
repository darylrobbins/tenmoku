---
task_uid: "01m55efb19x96k0hx40ms7zqmg"
task: "T179"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test git::tests::conflicts::accept::accept_current_incoming_and_both_at_the_caret passes: each of git.accept_current, git.accept_incoming and git.accept_both inside the first of two conflicts (one diff3) leaves exactly the expected text with the base dropped and the other conflict untouched, and one undo restores the markers byte for byte; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test git::tests::conflicts::accept::a_row_argument_picks_the_conflict passes: git.accept_incoming with args { row: \u003csecond start row\u003e } resolves the second conflict whatever the caret; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit tests git::tests::conflicts::accept::accept_all_resolves_every_conflict_in_one_undo_step and git::tests::conflicts::accept::outside_a_conflict_says_so pass: the three accept-all commands each resolve every conflict in one undoable step; a caret outside any conflict changes nothing and says 'Editor cursor is not within a merge conflict'; the six IDs are rows of the typed-ID table; they fail at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_conflicts_tests.rs only gains the #[path] mod line; the typed-ID table only gains rows); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Accept a merge conflict's current, incoming or both sides at the caret, or for every conflict in the file, in one undo step, as VS Code's merge-conflict commands do (Mac T861)

## Problem

With conflicts banded and navigable (the card this depends on), there is still no way to resolve one but editing
the markers out by hand. VS Code's merge-conflict commands resolve the conflict at the cursor
(`merge-conflict.accept.current`, `.incoming`, `.both`) or every conflict in the file (`.accept.all-current`,
`.all-incoming`, `.all-both`): the conflict's whole marker block is replaced by the chosen side's lines (both:
current, then incoming), the base dropped. Gap audit 2026-10-17 (user-approved: merge conflicts first).

## Change

In `crates/fe-workbench/src/git_conflicts.rs`:

1. `fn accept(&mut self, side: Side, which: Which, ..)` on `Workbench`, `Side` being Current, Incoming or Both and
   `Which` the conflict at a row or every conflict. It parses the active editor's text now
   (`fe_git::conflict_markers::parse`) and replaces each chosen conflict's `range()` with its `current` text, its
   `incoming` text, or `current` followed by `incoming`, all in one `Origin::User` transaction on the editor's
   model, so one `editor.undo` restores the markers; the caret goes to the start of the replacement (of the first,
   for all). The conflict "at a row" is the one whose `range()` holds that row's start, the start and end marker
   rows included.
2. Commands, as methods in the workbench block of `crates/fe-workbench/src/workbench_commands.rs` through
   `in_editor`, with rows in `crates/fe-workbench/src/typed_command_ids_tests.rs`:
   `git.accept_current` "Git: Accept Current Change", `git.accept_incoming` "Git: Accept Incoming Change",
   `git.accept_both` "Git: Accept Both Changes", `git.accept_all_current` "Git: Accept All Current",
   `git.accept_all_incoming` "Git: Accept All Incoming", `git.accept_all_both` "Git: Accept All Both". The three
   single ones take an optional args table `{ "row": <u32> }` (read from `kiln::current_invocation`), which the
   CodeLens-style actions card passes; without it they act on the conflict holding the primary caret's row.
   Messages, VS Code's: no conflict there, "Editor cursor is not within a merge conflict"; none in the file for the
   all-commands, "No merge conflicts found in this file". A read-only editor (a diff's left side) refuses as its
   other edits are refused.
3. Tests: new unit-test file `crates/fe-workbench/src/git_conflicts_accept_tests.rs`, declared by
   `#[path = "git_conflicts_accept_tests.rs"] mod accept;` at the end of `crates/fe-workbench/src/git_conflicts_tests.rs`
   (which the dependency creates), in the same headless workbench:
   - `accept_current_incoming_and_both_at_the_caret`: on a file with two conflicts (one diff3), each command with
     the caret inside the first leaves exactly the expected text (base dropped, the other conflict untouched), one
     undo restores the markers byte for byte;
   - `a_row_argument_picks_the_conflict`: `git.accept_incoming` with `{ "row": <second conflict's start row> }`
     and the caret elsewhere resolves the second;
   - `accept_all_resolves_every_conflict_in_one_undo_step` for each of the three;
   - `outside_a_conflict_says_so`: caret between conflicts: text unchanged, "Editor cursor is not within a merge
     conflict".
