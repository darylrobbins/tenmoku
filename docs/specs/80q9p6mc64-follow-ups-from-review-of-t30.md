---
task_uid: "01m4q0snykd1j66380q9p6mc64"
task: "T36"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T30

Findings of the review of T30 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/tests/corpus/ex_lines.toml:591: All six new regression cases also pass with the original bug. The oracle compares buffer, cursor, mode and registers, but omits errors; the following typed `j` executes after either success or failure and clears the message. Consequently these cases verify neither zero-address success nor past-end E16, and do not exercise the macro cancellation behavior described in the spec.
  fix: Make all six oracle cases distinguish success from failure by executing the sort and subsequent movement inside a replayed macro, then regenerate expectations. Also add direct assertions immediately after submission that zero-address commands succeed and past-end commands report E16.
  instances: crates/fe-vim/tests/corpus/ex_lines.toml:591, crates/fe-vim/tests/corpus/ex_lines.toml:597, crates/fe-vim/tests/corpus/ex_lines.toml:603, crates/fe-vim/tests/corpus/ex_lines.toml:609, crates/fe-vim/tests/corpus/ex_lines.toml:615, crates/fe-vim/tests/corpus/ex_lines.toml:621, crates/fe-vim/tests/corpus/ex_lines.expected.json:1936, crates/fe-vim/tests/corpus/ex_lines.expected.json:1954, crates/fe-vim/tests/corpus/ex_lines.expected.json:1972, crates/fe-vim/tests/corpus/ex_lines.expected.json:1990, crates/fe-vim/tests/corpus/ex_lines.expected.json:2008, crates/fe-vim/tests/corpus/ex_lines.expected.json:2026
