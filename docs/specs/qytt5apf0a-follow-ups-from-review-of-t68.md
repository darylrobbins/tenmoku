---
task_uid: "01m46xvven2mc1f0qytt5apf0a"
task: "T84"
rev: 3
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T68

Findings of the review of T68 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-command/src/registry.rs:337: Typed args are passed directly to serde without enforcing the table/null-free contract. Serde accepts positional arrays for derived structs, so editor.insert with args ["x"] validates and edits the buffer. Objects containing ignored null-valued fields also dispatch successfully and reach observers, producing records that cannot round-trip through TOML.
  fix: Before typed deserialization, require an object or omitted args and recursively reject contained nulls. Apply the same checks through validate and dispatch, with tests proving malformed shapes run neither handlers nor observers.
  instances: crates/fe-command/src/registry.rs:337

- F2 medium crates/fe-editor/src/commands.rs:444: editor.move silently substitutes 10,000 for larger valid counts while reporting success and recording the original invocation. For example, moving right 10,001 times through sufficiently long text stops at offset 10,000, violating the requested repeat count. The huge-count test uses short text and misses this truncation.
  fix: Either execute the requested count exactly or reject counts above the supported limit during argument validation with BadArgs; expose any limit in the schema and test the boundary on text where movement remains possible.
  instances: crates/fe-editor/src/commands.rs:444

- F3 medium crates/fe-editor/src/commands.rs:290: Handlers report Changed even when nothing changed: movement at a boundary, backspace at offset zero, undo/redo with no matching history, unfolding without folds, and inserting or pasting empty text at a caret all exhibit this. This violates the specified no-op behavior and makes the future adapter notify and autoscroll unnecessarily.
  fix: Determine effects from actual text, selection and display changes, preserving the specified copy/save behavior. Add no-op tests for movement, editing, folding and history commands, including empty selections.
  instances: crates/fe-editor/src/commands.rs:290, crates/fe-editor/src/commands.rs:301, crates/fe-editor/src/commands.rs:311, crates/fe-editor/src/commands.rs:321, crates/fe-editor/src/commands.rs:331, crates/fe-editor/src/commands.rs:341, crates/fe-editor/src/commands.rs:346, crates/fe-editor/src/commands.rs:350, crates/fe-editor/src/commands.rs:354, crates/fe-editor/src/commands.rs:364, crates/fe-editor/src/commands.rs:374, crates/fe-editor/src/commands.rs:395, crates/fe-editor/src/commands.rs:402, crates/fe-editor/src/commands.rs:408, crates/fe-editor/src/commands.rs:414, crates/fe-editor/src/commands.rs:430, crates/fe-editor/src/commands.rs:456

- F4 medium crates/fe-editor/src/lib.rs:820: run_as restores origin and transaction collection only after the closure returns normally. If an EditorHost method panics during an AI command and the embedding host catches the unwind, subsequent direct model edits remain stamped as AI and direct undo targets that AI origin; later dispatches also restore the leaked origin.
  fix: Use an unwind-safe scope guard to restore both fields on every exit, and test a panicking mock host followed by direct edits and actor-scoped undo.
  instances: crates/fe-editor/src/lib.rs:820
