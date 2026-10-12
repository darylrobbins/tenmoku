---
task_uid: "01m4r444m25y0tykfv9r1esfhk"
task: "T43"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T39

Findings of the review of T39 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:981: The fallback changes scroll without reconciling the retained shaped slices. In the new Double regression at offset 8000, the nearest miss retains the slice near offset 29940, whose caret coordinate comes from the index because 8000 is outside that slice. Moving scroll from 299727.5 to 79960 makes the cursor rectangle visible, but leaves all shaped text far offscreen: the caret appears over a blank row. Other long rows also retain slices for the old viewport. The regression checks only cursor bounds, so it passes despite this failure.
  fix: Reconcile the primary slice and all other long-row slices with the final viewport, preserving the shaped displacement and appropriate scroll bounds. Extend the regression to assert that the caret offset is actually shaped and that rendered text covers the viewport around it; also cover another visible long row.
  instances: crates/fe-display/src/layout.rs:981
