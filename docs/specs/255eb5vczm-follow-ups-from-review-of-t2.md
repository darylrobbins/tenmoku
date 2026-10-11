---
task_uid: "01m4p77bke93zf9t255eb5vczm"
task: "T11"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T2

Findings of the review of T2 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/src/element.rs:293: Passing cell_width as GPUI's force_width does not guarantee terminal cell positions. GPUI counts glyphs using a half-cell advance heuristic, which treats sufficiently narrow base glyphs as combining marks, and leaves the shaped line's natural width unchanged. For example, DejaVu Sans at 14 px places an equal-style "iiii" run off-grid; underline and strikeout extents also use the natural width instead of the occupied cells. Cross-cell ligatures can likewise invalidate glyph counting. This affects both ordinary runs and cursor glyphs through the shared helper.
  fix: Position shaped clusters using explicit cell-column mappings, preserving combining marks and wide characters, and derive decoration extents from the occupied cells. Prevent cross-cell shaping where necessary. Add shaping or painting regressions covering narrow glyphs, ligatures and decoration widths.
  instances: crates/fe-terminal-view/src/element.rs:293

- F2 medium crates/fe-terminal-view/src/element.rs:131: FocusHandle::is_focused only checks the window's retained focus handle; it remains true when that window becomes inactive. Switching to another app or window therefore leaves the terminal's filled block, beam or underline cursor displayed instead of the required hollow unfocused cursor.
  fix: Require both the terminal's focus handle and an active window when computing focused. Add a view regression that deactivates and reactivates a focused terminal window without terminal output.
  instances: crates/fe-terminal-view/src/element.rs:131
