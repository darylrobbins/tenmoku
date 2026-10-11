---
task_uid: "01m4p1v4gbf3b4jk63cyp3393a"
task: "T392"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T228

Findings of the review of T228 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/folds.rs:98: Every fold command returns Effect::Nothing even when it changes the display or moves the caret. The view dispatch path therefore skips repaint and cursor scrolling. Keyboard handling happens to request a repaint separately, but direct vim.fold dispatches do not, and keyboard folding still skips the usual cursor reveal.
  fix: Return Effect::Changed when folds or selections change, and Effect::Nothing only for a no-op; add assertions for dispatch effects.
  instances: crates/fe-vim/src/folds.rs:98

- F2 medium crates/fe-vim/src/folds.rs:135: Indentation fallback is disabled globally whenever the host returns any syntax candidate. If syntax candidates exist elsewhere but none contains the cursor, zc, za, zC and zA do nothing on an indented block that EditorModel::fold would fold through the row-specific fold_range_at fallback.
  fix: Resolve local candidates and indentation fallback at the cursor independently of candidates elsewhere, using the host's row-specific API where appropriate; test a host with an unrelated syntax candidate and an indented block at the cursor.
  instances: crates/fe-vim/src/folds.rs:117, crates/fe-vim/src/folds.rs:135

- F3 medium crates/fe-vim/src/folds.rs:205: Recursive opening follows only the first nested branch. With an outer closed fold containing two closed child blocks, zA on the outer header opens the outer fold and the first child's chain but leaves the second child closed, contrary to opening the fold recursively. zO uses the same incomplete selection. The fixture contains only one child per parent, so it cannot expose this.
  fix: Select all closed descendants for recursive opening rather than a single minimum-start chain, while keeping counted opening and reveal selection separate; add a branching nested-fold regression test for zA and zO.
  instances: crates/fe-vim/src/folds.rs:123, crates/fe-vim/src/folds.rs:130, crates/fe-vim/src/folds.rs:205
