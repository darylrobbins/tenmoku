---
task_uid: "01m4m2c9ys0k7snkc6h5v8g6f5"
task: "T337"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T307

Findings of the review of T307 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/ex.rs:523: Fully parsing command arguments before validating negative endpoints changes error precedence. From line 1, :-2d xxx reports E488, :-2d 0 reports E939, and :-2s/a/b/z reports E488; Neovim 0.12.4 reports E16 for all three. The range-validation fix therefore introduces incorrect messages for recognized commands with invalid arguments.
  fix: Separate command recognition and command-level checks from argument parsing. Validate applicable endpoints after recognition, preserving E492/E481/E477 precedence, but before delete counts and substitute arguments. Add message assertions for these combinations.
  instances: crates/fe-vim/src/ex.rs:523, crates/fe-vim/src/ex.rs:526

- F2 medium crates/fe-vim/src/ex.rs:526: The unconditional endpoint check rejects an unused first endpoint for Goto. From line 1, :-2,3 and :-2;3 should move to line 3, as verified against Neovim 0.12.4; this implementation returns E16 and remains on line 1. Range-only commands use the last address, so the negative-address fix remains incomplete.
  fix: Validate endpoints according to the command: Goto should validate its final address, while delete and substitute validate both retained endpoints. Preserve semicolon side effects and add comma/semicolon regression cases.
  instances: crates/fe-vim/src/ex.rs:526

- F3 medium crates/fe-vim/src/ex.rs:658: A zero offset at i32::MAX now fails because 0 >= max - line. Both +0 and -0 are represented as Offset(0). Neovim accepts :2147483647+0;2 and :2147483647-0;2 and moves to line 2; the candidate raises E1247 before applying the semicolon or final address.
  fix: Return the unchanged line for a zero offset before checking positive overflow, and add oracle cases for both signed zero forms at the maximum address.
  instances: crates/fe-vim/src/ex.rs:658
