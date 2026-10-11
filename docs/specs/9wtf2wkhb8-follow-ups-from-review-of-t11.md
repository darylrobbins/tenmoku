---
task_uid: "01m4pcksr3nrpkjb9wtf2wkhb8"
task: "T18"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T11

Findings of the review of T11 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/tests/layout.rs:391: The requested shaping or painting regressions are missing. This test only checks byte slices and column numbers from a manually constructed Run; it never exercises GPUI shaping, glyph origins, or decoration bounds. There is also no ligature case. The original rendering defect could return while this test still passes.
  fix: Add regressions exercising the production shaping or painting path with narrow glyphs, a ligature-capable sequence, combining marks and wide characters, and assert cell-aligned origins and occupied-cell underline/strikeout widths for ordinary text and cursor glyphs.
  instances: crates/fe-terminal-view/tests/layout.rs:391

- F2 medium crates/fe-terminal-view/tests/view.rs:127: The activation regression only calls is_focused directly. It does not draw or inspect the cursor, so it passes even if the activation subscription is removed or the element stops using the predicate. It therefore does not establish that an idle terminal redraws its cursor when deactivated and reactivated.
  fix: Extend the regression to render a focused terminal with no intervening terminal output, deactivate and reactivate the window, and verify redraw notification and filled-to-hollow-to-filled cursor transitions through the production rendering path.
  instances: crates/fe-terminal-view/tests/view.rs:127
