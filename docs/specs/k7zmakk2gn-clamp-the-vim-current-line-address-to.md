---
task_uid: "01m5tm46mf1nq0gxk7zmakk2gn"
task: "T350"
rev: 3
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In newline-terminated text with the caret on the final empty row, :.,$bf and the other Ordered commands with that range succeed as nvim 0.12.4 does instead of returning E493. Test current_line_clamps_to_logical_count_at_eof"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Clamp the vim current-line address to the logical line count so :.,$ at a terminating newline is not backwards (T344 F4)

## Problem
T344 review round 3 left F4 (medium, `crates/fe-workbench/src/vim_buffers.rs:480`), carried as a documented limit of T344: the corrected `$` count is inconsistent with `.` at a terminating newline. Fastedit's G reaches the final empty text row, so in `one\n`, G followed by `:.,$bf` resolves the range as 2,1 and wrongly returns E493 (nvim 0.12.4 accepts it); other Ordered commands with this range regress the same way.

## Change
Clamp the current-line address to the logical line count used for `$`.

## Tests
Host regression in `crates/fe-workbench/src/vim_buffers_cancel_tests.rs`: caret at EOF in newline-terminated text, `:.,$bf` succeeds.
