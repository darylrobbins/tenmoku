---
task_uid: "01m6ezv791pd9cf4pkneyb3tqq"
task: "T410"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "find_bar::in_selection_tests::matches_outside_the_scope_are_ignored: with the toggle on, count, next/previous and highlights cover only matches inside the captured selections (several selections too)"
    required: true
    manual: false
  - id: "C2"
    text: "find_bar::in_selection_tests::replace_all_stays_in_scope: Replace All edits only matches inside the scope, with regex and preserve case on"
    required: true
    manual: false
  - id: "C3"
    text: "find_bar::in_selection_tests::scope_follows_edits_and_clears: edits inside move the scope's ends; toggling off or closing the bar drops it; no non-empty selection leaves the toggle off"
    required: true
    manual: false
  - id: "C4"
    text: "find_scope_tests::scope_is_painted: the scope band is drawn while the toggle is on"
    required: true
    manual: false
  - id: "C5"
    text: "find_in_selection_keys: alt-cmd-l (macOS) / alt-l resolves to find.toggle_in_selection in the find_bar context"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy -p fe-workbench -p fe-gpui -p fe-keymap --all-targets is clean"
    required: true
    manual: false
---

# Add a Find in Selection toggle (alt-cmd-L) to the find bar that limits matches and replace-all to the captured selections, with the scope drawn (Mac T1316)

## Problem

The in-file find bar (crates/fe-workbench/src/find_bar.rs, `FindBar`) has Match Case, Whole Word, Regex and Preserve Case toggles (`FindOp::ToggleCase` etc., find.toggle_* bound in crates/fe-keymap/keymaps/fastedit.toml around line 1905, context `find_bar`) but no Find in Selection (VS Code's alt-cmd-L toggle): matches and Replace All always span the whole file. Approved 2026-11-02. (crates/fe-config/tests/whitespace_rulers_settings.rs only mentions render_whitespace "selection"; unrelated, not touched.)

## Change

1. `FindBar` gains `in_selection: bool` and `scope: Vec<Range<usize>>`; new `FindOp::ToggleInSelection` and command `find.toggle_in_selection` (find_commands.rs), bound `alt-cmd-l` (macOS) / `alt-l` elsewhere in context `find_bar`, with a toggle button beside the others in `render`. Turning it on captures the editor's non-empty selections as `scope` (anchored so edits inside the scope move its ends; fe-editor anchors or a mapped Range as multi-cursor does); with no non-empty selection it stays off. Turning it off, or closing the bar, drops the scope.
2. Matching: `EditorView::set_find` (crates/fe-gpui/src/view/find.rs:80) takes an optional scope; matches, the count label, next/previous, the minimap marks and Replace / Replace All consider only matches that lie wholly inside a scope range. The scope itself is drawn as a faint band (theme token reuse from the selection-highlight colours; no new theme key unless the theme test lists one).
3. Preserve Case, Regex, Whole Word and Case keep working inside the scope. vim's `FindQuery` (fe-vim/src/search.rs) is not changed; vim's `:s` is out of scope.

## Tests

New files only: crates/fe-workbench/src/find_in_selection_tests.rs (declared in find_bar.rs beside `preserve_case_tests`), crates/fe-gpui/src/view/find_scope_tests.rs (declared from view/find.rs), crates/fe-keymap/tests/find_in_selection_keys.rs.

## Implementation notes

- `EditorView::set_find` keeps its signature, so its callers stay as they are; the scope goes through a new `EditorView::set_find_scoped`. `FindBar` holds one `scope: Option<Vec<Range<Anchor>>>`, `None` meaning off, in place of a bool beside a list.
- Scope anchors are `anchor_before(start)..anchor_after(end)`, so a replacement of a match at either end stays inside. An unreadable anchor or a wholly deleted selection leaves less scope, never the whole file: an empty scope finds nothing.
- A selection ending at a row's start is kept as it is, so a selected blank row is a scope, but an empty match at that end isn't inside it (`EditorView::in_find_scope`), so whole rows selected don't give `^` a match on the next row. That end's anchor stays before text inserted there.
- A find on a remembered query moves the selection when the bar opens, so the bar remembers the non-empty selections the user last made: before each move of its own it compares all the selections with how it left them, and if any differ, those are the ones remembered.
- Scoped work searches only the rows the scope has text on and steps through a row only as far as it needs. That needs additions to fe-vim's search, none touching `FindQuery`: `visit_reaching`, which hands matches over one at a time and stops when told; `replace::replace_where`, which expands only the matches its caller keeps; `RowMatches::up_to`, which stops the search past the part of a row that matters, the row staying whole for its context; and `Deadline::cancelled_by`, which a background count stops by within a row, before any regex call. Going back still steps through a row up to where it started, as going back unscoped does, but keeps only the latest match.
- Following an editor remembers the selections it already had, as they're the user's.
- Following another editor drops the scope as well, since it's in the old editor's buffer.
- Beyond the spec's file list: `element.rs` (the highlight filter and the band), `terminal_find.rs` (its exhaustive `FindOp` match; the toggle does nothing there), the two tests that list every find command, and fe-vim's `search.rs` and `search/replace.rs` for the additions above.
