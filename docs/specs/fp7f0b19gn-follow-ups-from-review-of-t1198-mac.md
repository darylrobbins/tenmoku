---
task_uid: "01m730kx2qy6p2kffp7f0b19gn"
task: "T566"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "Normal mode: with \"abc\\ndefghijkl\\nmnopq\\nrstuvwxyz\", rows 1-3 (zero-based) closed in one fold and the cursor at (0,0), 4g_ lands on (3,8), the buffer end, as on base and in Neovim; a new test in crates/fe-vim/src/tests/line_motions.rs checks it"
    required: true
    manual: false
  - id: "C2"
    text: "Visual mode: in the same buffer and fold, v4g_ extends the selection to (3,8); a new test checks it"
    required: true
    manual: false
  - id: "C3"
    text: "Operator: in the same buffer and fold, d4g_ deletes all the text; a new test checks the resulting buffer"
    required: true
    manual: false
  - id: "C4"
    text: "An oversized count with j (e.g. 999j) past a terminal closed fold clamps to the buffer end the same way, and counts that stay within the visible rows keep their current fold-aware targets (existing line_motions tests pass unchanged)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# Follow-ups from review of T1198 (Mac T1718)

Findings of the review of T1198 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/line_motions.rs:327: Oversized counts now stop at a terminal fold's header instead of the buffer's last row. With "abc\ndefghijkl\nmnopq\nrstuvwxyz", rows 1–3 folded (zero-based), and cursor (0,0), `4g_` and `v4g_` now target (1,8); the base implementation and locally checked Neovim 0.12.6 target (3,8). Consequently, `d4g_` now leaves "\nmnopq\nrstuvwxyz" instead of deleting all text. The overflow branch breaks at the header, and the final normalization keeps it there.
  fix: Preserve buffer-end clamping when the count exceeds the remaining visible rows, without mapping that overflow destination back to the fold header. Add normal, visual, and operator regressions with a terminal closed fold and an oversized count.
  instances: crates/fe-vim/src/line_motions.rs:327, crates/fe-vim/src/line_motions.rs:338
