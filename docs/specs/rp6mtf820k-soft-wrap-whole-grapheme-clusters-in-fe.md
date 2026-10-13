---
task_uid: "01m4tej14cjr220crp6mtf820k"
task: "T112"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-display test wrap_clusters::soft_wrap_keeps_grapheme_clusters_whole: a ZWJ family emoji is never split across rows, and with Monospace at width 5 abcde plus a combining acute wraps as abcd / e plus the accent"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test wrap_clusters::soft_wrap_measures_clusters_with_cluster_width: with a CharWidth answering 1 per cluster, abcde plus a combining acute at width 5 is one row"
    required: true
    manual: false
  - id: "C3"
    text: "fe-display test wrap_clusters::soft_wrap_never_breaks_at_a_no_break_space: with Monospace at width 6, x 10 NBSP kg wraps as x space / 10 NBSP kg"
    required: true
    manual: false
  - id: "C4"
    text: "fe-display test wrap_clusters::soft_wrap_still_breaks_after_spaces: with Monospace at width 5, ab cd ef wraps as ab cd space / ef; the existing fe-display wrap_map tests pass unchanged"
    required: true
    manual: false
---

# Soft wrap whole grapheme clusters in fe-display, measured with CharWidth::cluster_width, and never break at a no-break space (Mac T646)

## Problem

Soft wrap in fe-display breaks rows inside grapheme clusters and at no-break spaces.
`break_points` (crates/fe-display/src/wrap_map.rs) walks the row with `char_indices()`, measures each char
alone through the `char_width` closure `Wrapper::wrap_next` passes, and may break before any non-space
char. So a zero-width joiner, a combining mark or the second regional indicator can start a continuation
row, and a multi-char cluster is never measured with `CharWidth::cluster_width`, which exists for exactly
this ("which a shaper may draw narrower or wider than its chars measured alone"; fe-gpui's `GlyphWidths`
implements it "for soft wrap and long rows"). Separately, `let is_space = ch.is_whitespace()` treats
U+00A0, U+2007 and U+202F as break opportunities, though UAX #14 forbids a break on either side of them.

Checked 2026-10-13 at Mac e6988aba (wrap_map.rs is identical on helium main df738a8), with `DisplayMap`
over `Buffer::new(text)`, tab size 4, all pending wrap done:

- width 5, every char and every cluster 1 wide: `"abcd" + family emoji (man ZWJ woman ZWJ girl) + "x"`
  wraps as `["abcd<man>", "<ZWJ>woman<ZWJ>girl x"]`, expected `["abcd<family>", "x"]`; `"abcde\u{301}"`
  wraps as `["abcde", "\u{301}"]`, expected one row.
- width 5, `Monospace`: `"abcde\u{301}"` wraps as `["abcde", "\u{301}"]`, expected `["abcd", "e\u{301}"]`.
- width 6, `Monospace`: `"x 10\u{a0}kg"` wraps as `["x 10\u{a0}", "kg"]`, expected `["x ", "10\u{a0}kg"]`.

## Change

In crates/fe-display/src/wrap_map.rs:

- `break_points` walks grapheme clusters (`unicode-segmentation`'s `grapheme_indices(true)`, already a
  fe-display dependency) and breaks only before a cluster. A single-char cluster is measured as today
  (`\t` included); a multi-char cluster with `CharWidth::cluster_width`. Columns still advance by chars
  (`cluster.chars().count()`), so `ExpandedRow` columns are unchanged.
- The width callback becomes a char measure plus a cluster measure (two closures or a small trait), so
  `Wrapper::wrap_next` routes clusters to `widths.cluster_width`. A cluster longer than
  `MAX_CLUSTER_BYTES` is measured as its chars summed, as `RowIndex` caps it, so the cost stays bounded.
- A cluster counts as space only when it is one whitespace char other than U+00A0, U+2007 and U+202F;
  those glue their neighbours (an over-wide glued run breaks mid-word as any long word does).
- The all-ASCII fast path is unchanged. The in-crate wrap_map tests pass unchanged.

## Tests

New file crates/fe-display/tests/wrap_clusters.rs, using `DisplayMap::new`, `set_wrap_width`,
`wrap_pending` until done, and `row(r).range` over the buffer text; `Cells` is a test `CharWidth` that
answers 1 for any char and any cluster:

- `soft_wrap_keeps_grapheme_clusters_whole` (regression, fails at base): with `Cells` at width 5,
  `"abcd" + family + "x"` gives `["abcd" + family, "x"]`; with `Monospace` at width 5, `"abcde\u{301}"`
  gives `["abcd", "e\u{301}"]`.
- `soft_wrap_measures_clusters_with_cluster_width` (regression, fails at base): with `Cells` at width 5,
  `"abcde\u{301}"` is one row.
- `soft_wrap_never_breaks_at_a_no_break_space` (regression, fails at base): with `Monospace` at width 6,
  `"x 10\u{a0}kg"` gives `["x ", "10\u{a0}kg"]`.
- `soft_wrap_still_breaks_after_spaces` (control): with `Monospace` at width 5, `"ab cd ef"` gives
  `["ab cd ", "ef"]` (spaces still hang past the edge).
