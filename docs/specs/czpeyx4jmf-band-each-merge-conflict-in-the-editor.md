---
task_uid: "01m55efaddb60we4czpeyx4jmf"
task: "T178"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test git::tests::conflicts::conflict_rows_are_banded_by_side passes: RowBackground Added on exactly the start-marker and current rows, Muted on base rows, Modified on incoming and end rows, unchanged after typing inside a side, gone for a conflict whose end marker is deleted; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test git::tests::conflicts::next_and_previous_conflict_wrap_round passes: git.next_conflict visits each start-marker row in turn and wraps to the first with 'Wrapped to the first conflict'; git.previous_conflict from the first wraps to the last; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit test git::tests::conflicts::no_conflicts_says_so passes: in a file without markers the caret stays and the message is 'No merge conflicts found in this file'; git.next_conflict and git.previous_conflict are rows of typed_command_ids_tests.rs's table; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_live_tests.rs only gains the #[path] mod line; the typed-ID table only gains rows); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Band each merge conflict in the editor by side and go to the next or previous conflict with Git: Next Conflict and Git: Previous Conflict, wrapping round (Mac T860)

## Problem

A file with git conflict markers shows in fastedit as plain text; VS Code bands each conflict (current side green,
incoming side blue, a diff3 base grey) and has Next / Previous Conflict navigation (`merge-conflict.next` /
`.previous`, which wrap round). Gap audit 2026-10-17; the user approved "card ... merge conflicts first" and the
lead asked for a "Git: Next/Previous Conflict" command. The markers are parsed by `fe_git::conflict_markers::parse`
(the card this depends on).

## Change

New module `crates/fe-workbench/src/git_conflicts.rs`, declared from `crates/fe-workbench/src/git.rs` as
`#[path = "git_conflicts.rs"] pub(crate) mod git_conflicts;`:

1. **Bands.** `ConflictBands`, a `DecorationProvider` computing from the snapshot it's asked about and caching by
   `snapshot.version()`: it parses only when the text contains `<<<<<<<` at a row start and is at most 8 MiB
   (`MAX_SCAN_BYTES`, documented: larger files get no bands). For each conflict, `RowBackground` decorations:
   the start marker's row and the current side's rows `Tint::Added`; each base marker and base rows `Tint::Muted`;
   the incoming side's rows and the end marker's row `Tint::Modified`; the splitter row none. Priority below
   diagnostics' and the git change bar's, so they still show.
2. **Installed** by `GitState::track` (`git.rs:308`), which already keeps per-editor state: each editor it tracks
   for the first time gets one `ConflictBands` (`model_mut().add_decoration_provider`), whether or not the root is a
   repository; a closed editor's entry is forgotten with the others.
3. **Commands**, as methods in the `#[kiln::command(context = "workbench")]` block of
   `crates/fe-workbench/src/workbench_commands.rs` (the layout T704 leaves), each through `in_editor` like
   `lsp.rename`, with rows added to `crates/fe-workbench/src/typed_command_ids_tests.rs`:
   `git.next_conflict` "Git: Next Conflict" and `git.previous_conflict` "Git: Previous Conflict". They parse the
   active editor's text now and put the caret at the start of the start-marker row of the first conflict starting
   after (before) the caret's row, with `EditorView::select_and_reveal` as `go_to_change` does
   (`git_diff.rs:1381`); past the last (first) they wrap round to the first (last) and say so where `go_to_change`
   reports its wrap (`answer_git_op`'s surface for a command run from the palette or a key; own constants: "Wrapped to the first conflict" / "Wrapped to the last
   conflict"); a file with none says "No merge conflicts found in this file" (VS Code's words). No default keys.
4. **Tests**: new unit-test file `crates/fe-workbench/src/git_conflicts_tests.rs`, declared by
   `#[path = "git_conflicts_tests.rs"] mod conflicts;` at the end of the existing
   `crates/fe-workbench/src/git_live_tests.rs` (so `git::tests::conflicts::…`), in a headless workbench as that
   file's tests are:
   - `conflict_rows_are_banded_by_side`: a file with a two-way conflict and a diff3 one, opened: the editor's
     `model().decorations(..)` over the text have `RowBackground` `Added` on exactly the start and current rows,
     `Muted` on the base rows and `Modified` on the incoming and end rows; after typing a char inside the current
     side the bands are the same rows; after deleting the end marker's row that conflict has none;
   - `next_and_previous_conflict_wrap_round`: three conflicts, caret on row 0: `git.next_conflict` three times lands
     on each start-marker row in turn, a fourth wraps to the first with the wrap message; `git.previous_conflict`
     from the first wraps to the last;
   - `no_conflicts_says_so`: a file without markers: the caret stays and the message is "No merge conflicts found in
     this file".
