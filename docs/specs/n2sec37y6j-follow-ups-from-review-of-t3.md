---
task_uid: "01m4p8eamxkh1wqsn2sec37y6j"
task: "T12"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T3

Findings of the review of T3 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/src/input.rs:295: Composition updates ignore both replacement and selection ranges. For example, replacing 1..2 of marked "ab" with "c" produces "c" instead of "ac"; an IME-selected range is always reported as a caret at the end. This breaks input methods that edit or select part of their preedit.
  fix: Apply replacement ranges to the marked document in UTF-16 units, store the supplied selection, and return it from selected_text_range. Add tests for partial replacements and nonterminal selections, including supplementary Unicode characters.
  instances: crates/fe-terminal-view/src/input.rs:295, crates/fe-terminal-view/src/input.rs:259

- F2 medium crates/fe-terminal-view/src/input.rs:303: Starting IME composition does not scroll to the bottom or clear the selection, contrary to the specification's requirement for any input. In scrollback, the preedit is painted at a clamped viewport row while bounds_for_range can place the candidate window outside the terminal because the actual cursor is offscreen.
  fix: Prepare the terminal viewport and clear its selection when nonempty composition input arrives, without writing preedit bytes. Test composition while scrolled back with an active selection and verify that its bounds match the visible cursor.
  instances: crates/fe-terminal-view/src/input.rs:303

- F3 medium crates/fe-terminal-view/src/input.rs:211: Paste writes bytes without resolving or clearing marked text. Both keyboard paste and platform paste leave the old preedit painted and marked_text_range active after the shell receives the paste, allowing a later commit to send stale composition after the pasted bytes.
  fix: Resolve or cancel active composition consistently before writing a paste in the shared paste_text path, notify the view, and test both paste entry points with active marked text.
  instances: crates/fe-terminal-view/src/input.rs:211, crates/fe-terminal-view/src/input.rs:185, crates/fe-terminal-view/src/input.rs:278
