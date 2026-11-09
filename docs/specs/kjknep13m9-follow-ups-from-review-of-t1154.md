---
task_uid: "01m6yxyw9qc9t9r3kjknep13m9"
task: "T1625"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1154

Findings of the review of T1154 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/element/whitespace.rs:124: Each mark performs two buffer-to-display conversions that rescan preceding tabs in ExpandedRow::expand. An allowed 8,192-tab row therefore visits 67,108,864 tab entries per frame, before horizontal clipping. Multiple such rows can stall redraws despite the long-row guard.
  fix: Use a batch conversion that walks tabs and inline spans once, or indexed prefix widths, and add a cost regression test for tab-heavy rows at the shaping limit.
  instances: crates/fe-gpui/src/element/whitespace.rs:124, crates/fe-gpui/src/element/whitespace.rs:125

- F2 medium crates/fe-gpui/src/element/whitespace.rs:59: Selection membership scans every preceding selection for every whitespace byte. After selecting many occurrences and scrolling near the document's end, even unselected whitespace repeatedly traverses all earlier ranges, making the default mode cost proportional to whitespace count times selection count.
  fix: Resolved selections are sorted and non-overlapping: check only the range immediately before the partition point, or advance a selection cursor alongside offsets. Add coverage with many offscreen selections.
  instances: crates/fe-gpui/src/element/whitespace.rs:59

- F3 medium crates/fe-gpui/src/element/whitespace.rs:124: to_display_after_hints excludes the non-hint inlay used for ghost text and IME composition. When that inlay is anchored before a marked space or tab, the start is before the inlay while the end includes it. The dot is centered over suggestion/composition text, or the tab arrow spans that text, instead of marking only the buffer character.
  fix: Start the mark after every inline span anchored before that character, including ghost text and composition. Add independent position assertions for a space and tab with an inlay at the same offset.
  instances: crates/fe-gpui/src/element/whitespace.rs:124

## Implementation notes

- F1/F3: marks no longer call `to_display_after_hints`/`to_display` per mark.
  `whitespace::display_indexes` walks the shown text once, tracking tab-expanded
  columns, and shifts each mark past every hint/inlay anchored at or before the
  char (`spans_before` recovers the anchors from `hint_ranges`/`inlay_range`).
  A mark is the char's own advance wide, so ghost text/composition at the same
  offset is excluded. fe-display is out of scope, so no new `ExpandedRow` API.
- F2: `is_selected` checks only the selection just before the partition point.
- Carried T1497 F2 (element.rs:3334) not fixed: `EditorTheme.syntax` is the
  resolved table (`fe_syntax::resolve_theme`) and keeps no record of which names
  the theme set explicitly, so the fix needs a new field in `EditorTheme`
  (`crates/fe-gpui/src/lib.rs`, `theme.rs`), outside this task's scope.
