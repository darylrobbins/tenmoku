---
task_uid: "01m4q13kkfdzp9280mt1dvf54d"
task: "T37"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T31

Findings of the review of T31 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:932: Completion still selects a known invisible candidate, contrary to the requested fix. Using the new regression's inputs but shaping both buffer 'x' and inlay 'W' at 20px while the index measures them at 10px, the three attempts produce (scroll, caret) pairs (327.5, 165200), (164927.5, 164800), and (560, 164960). None is visible; the new minimum-miss fallback selects the second, leaving the caret 127.5px left of the text viewport. Retaining the shaped inlay width alone does not ensure convergence when buffer shaping also differs from indexed widths.
  fix: Reconcile and reshape at the actual caret position when all initial attempts miss, ensuring the selected layout shows the caret. Add a regression where both buffer and inlay shaped widths exceed indexed character widths.
  instances: crates/fe-display/src/layout.rs:932
