---
task_uid: "01m5tyhprp99e4hwdfc7f2wak9"
task: "T354"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Reopen with Encoding keeps a visual selection suspended by a / or ? command line, keeps a block selection extended with $ at the same extent on ragged rows, and leaves each editor's scroll position alone. Tests reopen_keeps_search_suspended_visual, reopen_keeps_block_dollar_goal, reopen_keeps_viewport"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Remap vim visual selections after Reopen with Encoding: search-suspended visual, block $ goal, and the viewport (T348 F11-F13)

## Problem
T348 (Reopen Editor with Encoding, Mac T1161) review round 5 left F11-F13 (medium, `crates/fe-workbench/src/encoding_picker.rs`), carried as documented limits of T348; they need an fe-vim API, out of T348's scope:
- F11 (:523): the vim visual remap after a reopen skips a visual selection suspended by a `/` or `?` search command line (mode CmdLine); remap held visual anchors in both active visual mode and a command line suspended from visual mode, preserving the command line state.
- F12 (:555): restoring through `select_visual` resets the visual goal to None, so a block selection extended with `$` (goal usize::MAX) loses its extent ("abcdef\nabc", start col 1, Ctrl-V j $ then reopen shrinks row 1 to "bcd"); add a vim state remapping API that preserves the visual goal while replacing anchor offsets.
- F13 (:554): `update_vim` unconditionally calls `EditorView::changed`, which autoscrolls to the caret; restore anchors without requesting cursor reveal, preserving each editor's scroll.

## Change
Add the fe-vim remap API in `crates/fe-vim/src/visual.rs` and use it from `encoding_picker.rs`.

## Tests
`crates/fe-vim/tests/visual_remap.rs` for the API; `crates/fe-workbench/src/encoding_picker_tests.rs`: search-suspended visual selection, ragged-row `$` block selection, scrolled-away cursor keeps the viewport.
