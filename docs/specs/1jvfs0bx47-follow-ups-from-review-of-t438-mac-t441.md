---
task_uid: "01m4ptxt41eavp6r1jvfs0bx47"
task: "T61"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T438 (Mac T441)

Findings of the review of T438 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/tests/corpus/increment.toml:821: The new tab and empty endpoint cases do not exercise endpoint geometry loss. The tab case replays a one-column block whose endpoints are both ordinary digits; its tab lies outside the selected columns. The empty case starts on the empty row and ends on a nonempty row. Both produce the same results with the original endpoint-based reconstruction, leaving the explicitly requested tab-widening and empty-final-endpoint regressions uncovered.
  fix: Add cases whose final replay row is empty or whose final endpoint falls inside a tab. Make the preceding replay row contain numbers that distinguish the saved bounds from widened or left-shifted bounds, record Neovim expectations, and verify these regressions fail with the original implementation.
  instances: crates/fe-vim/tests/corpus/increment.toml:815, crates/fe-vim/tests/corpus/increment.toml:821, crates/fe-vim/tests/corpus/increment.expected.json:3107, crates/fe-vim/tests/corpus/increment.expected.json:3135

## Implementation notes

F1: added four block-replay corpus cases (empty final row, empty row after a gap, final and start endpoints inside a tab), recorded with nvim 0.12.4. With the block branch of `replay_area` disabled, all four fail (as do the earlier block cases).
