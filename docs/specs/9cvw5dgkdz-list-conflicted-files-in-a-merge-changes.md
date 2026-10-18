---
task_uid: "01m55ef9tp1rgrf99cvw5dgkdz"
task: "T177"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test git_panel::git_panel_groups_tests::merge::conflicted_files_are_in_merge_changes_first passes on a repository stopped on a real git merge conflict: groups Merge Changes (the conflicted file) then Changes, with counts, the conflicted file not in Changes; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit tests git_panel::git_panel_groups_tests::merge::staging_a_file_with_markers_asks_first and git_panel::git_panel_groups_tests::merge::a_resolved_file_stages_without_asking pass: Stage Changes on a Merge row with markers prompts 'Are you sure you want to stage \u003cname\u003e with merge conflicts?', Cancel leaves git ls-files -u listing it, Stage stages it and the refresh moves it to Staged Changes; with the markers removed it stages without a prompt; they fail at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit test git_panel::git_panel_groups_tests::merge::a_merge_row_opens_the_file passes: Enter on a Merge row opens the file's editor, not a diff view; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_panel_groups_tests.rs only gains the #[path] mod line); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# List conflicted files in a Merge Changes group above Staged Changes, opening the file on click and asking before staging one that still has conflict markers, as VS Code does (Mac T863)

## Problem

During a merge, VS Code's Source Control view lists conflicted files in a "Merge Changes" group above Staged
Changes and Changes; staging one (its "+" button, or "Stage All Merge Changes" on the header) marks it resolved,
first asking "Are you sure you want to stage <file> with merge conflicts?" if the file still holds conflict
markers. fastedit's Changes panel puts a conflicted file in Changes with a `C` (`crates/fe-workbench/src/git_panel.rs:7`
"a conflicted file is only in Changes", `:663`), opens it as an Index ↔ Working Tree diff like any change, and
stages it with no check. Gap audit 2026-10-17 (user-approved: merge conflicts first, "Changes panel integration
(the Merge Changes group)").

## Change

1. `crates/fe-workbench/src/git_panel.rs`:
   - `Group::Merge`, titled "Merge Changes", listed first, shown only when it has rows (as Staged Changes is),
     with its count; a `ChangeKind::Conflicted` file goes there instead of Changes, still lettered `C` in the error
     colour.
   - A Merge row's click (or Enter) opens the file itself, not a diff (the bands and actions are in the editor);
     its buttons are "Open File" and "Stage Changes"; the header's is "Stage All Merge Changes". No discard on these
     rows. Vim keys: `s` / `-` stage a Merge row; `u` and `X` do nothing on it.
   - Update the module docs' groups paragraph.
2. `crates/fe-workbench/src/git_stage.rs`: staging Merge rows reads each file's text (the open buffer's if it is
   open, else the file on disk) and checks it with `fe_git::conflict_markers::parse`; files that still have a
   conflict get one prompt, "Are you sure you want to stage <name> with merge conflicts?" (several: "Are you sure
   you want to stage <n> files with merge conflicts?"), buttons "Stage" and "Cancel"; files with none are staged
   without asking. Staging is the existing stage path (`git add`), after which the next status refresh moves the
   file to Staged Changes as git reports it.
3. Tests: new unit-test file `crates/fe-workbench/src/git_panel_merge_tests.rs`, declared by
   `#[path = "git_panel_merge_tests.rs"] mod merge;` at the end of the existing
   `crates/fe-workbench/src/git_panel_groups_tests.rs`, on a repository the tests build with the `git` CLI as that
   file's tests do, stopped on a real conflict (`git merge` of two branches editing one line):
   - `conflicted_files_are_in_merge_changes_first`: the panel's groups are Merge Changes (the conflicted file),
     then Changes (an unrelated unstaged edit), headers with counts; the conflicted file isn't in Changes;
   - `staging_a_file_with_markers_asks_first`: "Stage Changes" on the Merge row shows the prompt; "Cancel" leaves
     the index unmerged (`git ls-files -u` still lists it); "Stage" stages it and after the refresh it's in Staged
     Changes and Merge Changes is gone;
   - `a_resolved_file_stages_without_asking`: the markers removed and saved first: staging asks nothing;
   - `a_merge_row_opens_the_file`: Enter on it opens the file's editor, not a diff view.
