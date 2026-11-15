---
task_uid: "01m7fmqvakmy469gxe2e6pzz07"
task: "T371"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1203 (Mac T1716)

Findings of the review of T1203 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-editor/src/cursor_history.rs:139: The newly supported identity-only Cursor Undo/Redo transitions do not reveal the restored primary. commands.rs::Shown compares selection ranges but omits the primary, so dispatch converts Effect::Changed into Effect::Nothing when only identity changes. Consequently EditorView::changed is not called and autoscroll stays disabled. The new history tests call cursor_undo/redo directly, missing this integration failure.
  fix: Include the effective primary in the command dispatcher's Shown comparison. Add dispatched Undo/Redo tests asserting Effect::Changed for identity-only transitions and a headless-window regression proving an offscreen restored primary is revealed.
  instances: crates/fe-editor/src/cursor_history.rs:139
