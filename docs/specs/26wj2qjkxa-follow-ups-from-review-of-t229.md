---
task_uid: "01m4p5ejwpnrmr8526wj2qjkxa"
task: "T439"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T229

Findings of the review of T229 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/scroll.rs:183: Changing window height only bypasses the stored scroll count; it never invalidates it. From row 0, set 10 rows, feed 3<C-d>, resize to 4, feed <C-d>, resize to 10, and feed <C-d>: the candidate ends at top/cursor row 8, while Neovim 0.12.4 ends at row 10. Both half-page directions revive stale counts when an earlier height returns.
  fix: Invalidate the counted scroll amount when the window height changes, including in set_window_rows, and add resize-away-and-back tests for both directions.
  instances: crates/fe-vim/src/scroll.rs:183, crates/fe-vim/src/headless.rs:355

- F2 medium crates/fe-vim/src/scroll.rs:291: Visual counted zt, zz and zb use DocumentEnd, restoring the goal column instead of retaining the current byte column. On a 60-row buffer containing alphabet-length lines except row 2 containing "x", start at (0,20) and feed v2j20zt: Neovim ends at (19,1), but this branch moves to (19,20), changing the selected text. The same defect affects counted zz and zb; documenting the divergence does not establish a spec exception.
  fix: Preserve the visual cursor's byte column and remembered goal column for counted z positioning, and add fixed-window oracle cases after short-line clamping for zt, zz and zb, including subsequent vertical movement.
  instances: crates/fe-vim/src/scroll.rs:291
