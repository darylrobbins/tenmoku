---
task_uid: "01m4pdwy220297xqh5hp0y3kqr"
task: "T20"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T12

Findings of the review of T12 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/tests/keyboard.rs:333: The scrollback regression test never enters scrollback: open() runs a shell that prints only `ready` in an 800-pixel-high terminal, leaving no history, so scroll(-1) does nothing. The display_offset assertion passes even if scroll_to_bottom() is removed. The test also omits the requested verification that composition bounds match the visible cursor.
  fix: Generate enough terminal output to create history, assert a nonzero display_offset and an active selection before composing, then verify composition resets the offset, clears selection, writes no preedit bytes, and returns bounds at the visible cursor cell.
  instances: crates/fe-terminal-view/tests/keyboard.rs:333
