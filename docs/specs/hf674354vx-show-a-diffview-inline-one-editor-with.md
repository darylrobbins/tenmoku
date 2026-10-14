---
task_uid: "01m4xew5ze7zdw7yhf674354vx"
task: "T118"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test diff_view_inline_tests::removed_rows_above: inline, only the right editor is shown; a hunk replacing left rows 'b c' with right row 'X' paints a 2-line Lines block 'b','c' with the Removed tint directly above 'X', and no fillers"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test diff_view_inline_tests::toggle_keeps_place: diff.toggle_inline, run through DiffView's command-name dispatch, from side by side with the right caret on row 40 keeps the caret, selection and top visible right row; toggling back restores side by side with fillers; with the left side focused the caret moves to the paired right row"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test diff_view_inline_tests::narrow_start: a DiffView opened in a window 800 px wide starts inline, at 1200 px side by side; toggling overrides it; diff.next_change works inline"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-gpui passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show a DiffView inline: one editor with removed rows as blocks above their place, toggled per view and starting inline in narrow windows (Mac T546)

## Problem

The user (2026-10-12) asked for the diff view to have "side-by-side and inline modes". The DiffView card shows only
side by side.

## Change

In `crates/fe-gpui/src/diff_view.rs` and new file `crates/fe-gpui/src/diff_view/inline.rs`:

- `diff.toggle_inline` (title "Toggle Inline Diff"), in the `DiffView` key context, switches the view between side by
  side and inline; `DiffView::set_inline(bool)` for hosts. The mode is per view.
- Inline (VS Code's inline diff editor): only the right side's editor shows; each hunk's removed left rows show as a
  `HostBlockKind::Lines` block with the `Removed` tint above right row `hunk.right.start`
  (`TextDiff::removed_above`); added and modified right rows keep their tints; no fillers.
- Switching keeps the right side's caret, selection and the top visible right row; focus stays on the right side
  (switching from inline to side by side focuses the right side). If the left side had focus when switching to
  inline, focus moves to the right side at the row paired with the caret's.
- `diff.next_change`/`previous_change` and vim's `]c`/`[c` work in inline mode on the right side.
- When the window is narrower than 900 px a newly opened DiffView starts inline (VS Code's
  `diffEditor.useInlineViewWhenSpaceIsLimited`); the user's toggle wins afterwards.

## Tests

New file `crates/fe-gpui/src/diff_view_inline_tests.rs` (declared `#[cfg(test)]` from diff_view.rs).

## Out of scope (lead, 2026-10-14)

Registering `diff.toggle_inline` in the command catalog and palette (`crates/fe-gpui/src/commands.rs`
`command_catalog()` / `command_list()`) is not this task's: helium T121 (Mac T660), which depends on this task, adds the
diff registry there, and its criterion C3 requires the catalog and palette to know `diff.toggle_inline`,
`diff.next_change` and `diff.previous_change`. At this base `diff.next_change` is likewise dispatched only by name from
`DiffView`. Review finding F1 is disputed on that ground. Don't touch `commands.rs`; instead, drive the toggle in the
tests through `DiffView`'s command-name dispatch for `diff.toggle_inline` (the `TOGGLE_INLINE` arm), not by calling
`toggle_inline()` directly.

## Implementation notes

- `diff.toggle_inline` is handled in the `DiffView` key context like `diff.next_change`; neither is in the command
  catalog (`crates/fe-gpui/src/commands.rs`), which is outside this task's scope. Registering both with their titles
  ("Toggle Inline Diff") is left to a follow-up.
- `DiffView::set_inline` takes the window and context, to move focus.
