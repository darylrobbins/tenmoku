---
task_uid: "01m4pccfx0he9k4qrk2pffyxgf"
task: "T444"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T439

Findings of the review of T439 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/visual.rs:646: jump_cursor stores a character-boundary offset without normalizing it to a visual grapheme position. For buffer "abcde\néxxxxx", starting at (0,1) and feeding v2zt lands the stored offset inside é. visual_cursor resolves this to column 0, but select computes the selection end from the unnormalized offset and includes the following x. The model selection is therefore inconsistent with the reported visual cursor until sync_visual runs again.
  fix: Normalize the destination through visual::position before constructing Ends and computing movement; add a combining-grapheme regression that checks the model selection immediately after the jump, alongside cursor and goal preservation.
  instances: crates/fe-vim/src/scroll.rs:301, crates/fe-vim/src/visual.rs:646

- F2 medium crates/fe-vim/src/scroll.rs:177: For hosts other than HeadlessVim, invalidation still happens only when vim.scroll runs at the changed height. Set a count at height 10, resize to 4, optionally execute a non-scroll motion, resize back to 10, then execute an uncounted half-page command: the stored height matches again and the stale count survives. Only HeadlessVim receives immediate resize invalidation, leaving the original away-and-back defect in the host-facing path.
  fix: Provide a shared viewport-height-change notification that invalidates VimState's counted scroll and invoke it from host resize handling, including HeadlessVim; add host-facing resize-away-and-back tests for both half-page directions without an intervening scroll command.
  instances: crates/fe-vim/src/scroll.rs:177
