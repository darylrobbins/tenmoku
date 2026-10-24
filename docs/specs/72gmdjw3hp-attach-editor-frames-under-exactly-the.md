---
task_uid: "01m5q9nf2kfs0hqb72gmdjw3hp"
task: "T316"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "clashes marks a negative-only editor condition under every view that holds editors (DiffView, file_viewer, chat composer, panes) and none under a view that holds none. Tests diffview_negative_only_editor_condition_is_marked, non_editor_view_stays_unmarked"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Attach editor frames under exactly the editor-holding key contexts when marking shortcut conflicts (T305 F3)

## Problem
T305 (key-context frames for shortcut conflicts) review F3: editor frames attach under another named view only where a binding names both or a term needs an editor key, so a condition that only rules out editor keys under a view that holds editors (for example `DiffView && !vim.enabled` against an editor binding) goes unmarked.

## Change
- fe-gpui / fe-workbench expose the set of key contexts an EditorView can sit in (DiffView, file_viewer, chat composer, panes, ...), and `clashes` in `crates/fe-workbench/src/shortcuts_editor.rs` attaches editor frames under exactly those.

## Tests
Regressions in `crates/fe-workbench/src/shortcuts_editor_frames_tests.rs` (`#[path]`): the DiffView negative-only case is marked; a view that holds no editor stays unmarked.
