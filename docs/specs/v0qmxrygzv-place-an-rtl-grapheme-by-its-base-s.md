---
task_uid: "01m71mssgtcb3hqjv0qmxrygzv"
task: "T317"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "With advances on the line (Line::with_advances / Bidi::with_advances), a grapheme whose base and a negative-offset mark share one glyph index keeps its base's edges: caret x, index_for_x, block width and the neighbour's edge, resolved right to left from the line's end; the regression test a_mark_sharing_its_bases_index_does_not_move_its_edges passes (via the Line::new estimate path), and the two cases geometry cannot settle (adjacent marks whose offset exceeds the advance; unequal-width rows on the Line::new path, row mean as the estimate) are named as known limits in the spec and in a code comment at the heuristic in bidi.rs"
    required: true
    manual: false
  - id: "C2"
    text: "Existing element::bidi tests pass unchanged"
    required: true
    manual: false
---

# Place an RTL grapheme by its base's advance, not its leftmost glyph: a mark sharing its base's cluster index and offset left shifts carets and the block (T305 review F2)

Follow-up of T305 (review round 2, F2), which placed carets and hit-tested by bidi run in right-to-left rows
(crates/fe-gpui/src/element/bidi.rs).

`Bidi::place` puts each grapheme at the leftmost glyph whose byte index is the grapheme's first byte, so a mark with
an index of its own (the usual CoreText case) doesn't move the grapheme's edges. But a shaper can give a base and its
mark the same cluster index. Then a mark offset left of its base (base at x=20, mark at x=18, both at byte 2, advance
10) moves the grapheme's left edge to 18: the RTL trailing caret is at 18, the block caret is 12 wide instead of 10,
and the neighbour to the left loses 2 px. GPUI's ShapedGlyph keeps no advance, so position alone can't tell base from
mark.

Fix: place graphemes from base advance geometry. For example, Line could keep each glyph's advance, read through the
text system's `advance(font_id, glyph_id)` when it shapes, and take a cluster's extent from its widest glyph's origin
and advance rather than the minimum origin. Or find a way to tell mark glyphs apart, such as typographic bounds or a
zero advance. Add a unit test in bidi_tests.rs for a negative-offset mark that shares its base's index, checking
caret x, block width and the neighbour's edge. Verify on macOS with a font whose marks share their base's index,
if one exists.

## Decision (Mac lead, 2026-11-11): land the heuristic with documented limits

GPUI exposes no per-glyph advance (`TextSystem::advance` takes a char; `PlatformTextSystem::advance` by glyph id is private; `ShapedGlyph` has no advance), so the fix is the one on this lane: `Line::with_advances` / `Bidi::with_advances` take the exact first-char advance from the window text system, and `Bidi::place` resolves right to left from the line's end, picking the glyph that reaches the neighbour's x. The Mac files the card for per-glyph advances on `ShapedGlyph`/`TextSystem` (do not file it on neon).

Known limits, written down here and in a code comment at the heuristic in `bidi.rs`:
1. Overlapping adjacent marks whose offset exceeds the advance can't be told from geometry alone.
2. Unequal-width rows on the old `Line::new` path (no advances) fall back to the row mean as the advance estimate.

The regression test builds against pre-fix code, so it uses the old `Line::new` API and exercises the estimate path.
