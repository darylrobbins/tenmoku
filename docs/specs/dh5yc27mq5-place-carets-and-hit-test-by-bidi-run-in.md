---
task_uid: "01m6w2wfqp9n4n9qdh5yc27mq5"
task: "T305"
rev: 3
type: "bugfix"
complexity: "l"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-gpui --test non_latin_text passes on Linux, with the cross-platform twins rtl_reordered_caret_x_moves_right_to_left_through_an_rtl_span and rtl_reordered_click_on_an_rtl_glyph_lands_on_its_nearer_logical_edge (over a test text system that reorders RTL runs as CoreText does) failing at base and passing; the macOS-only rtl_caret_x_moves_right_to_left_through_an_rtl_span and rtl_click_on_an_rtl_glyph_lands_on_its_nearer_logical_edge replace rtl_carets_inside_an_rtl_span_share_one_x and rtl_click_on_the_last_painted_rtl_glyph_lands_at_the_row_end, and the other rtl_ tests there are unchanged. Documented limit: this Linux lane cannot run CoreText; the macOS run of that file is verified by the Mac mirror card (T1503)"
    required: true
    manual: false
  - id: "C2"
    text: "The fe-gpui Line computes caret x and index-for-x itself from its glyph positions and unicode-bidi embedding levels, only for rows with a strong RTL char (a byte scan skips the rest); a unit test pins that an ASCII row gives the same x_for_index and index_for_x as GPUI"
    required: true
    manual: false
  - id: "C3"
    text: "fe_display::layout::MeasuredLine gains the provided method visual_ranges(\u0026self, range: Range\u003cusize\u003e) -\u003e Vec\u003cRange\u003cf32\u003e\u003e (default: one ordered range between the x of the two ends), and Line overrides it with one x range per visual piece, left to right; unit tests cover a range inside an RTL run and one across a direction change"
    required: true
    manual: false
  - id: "C4"
    text: "Inside an RTL run the vim block caret (block_rect) is its glyph wide and FrameLayout::char_at returns the glyph under the point (unit tests). Documented limit: a grapheme is placed at its first char's glyph, so a mark that shares its base's glyph index and is offset left of it still shifts the edges (follow-up T317)"
    required: true
    manual: false
  - id: "C5"
    text: "unicode-bidi is a workspace dependency of fe-gpui and Cargo.lock gains no new package (0.3.18 is already locked)"
    required: true
    manual: false
---

# Place carets and hit-test by bidi run in right-to-left text: each caret on its letter logical edge, a click on the nearer logical edge, not the row end (T1431 G1, G2) (Mac T1503)

Follow-up of T1431 (docs/decisions/yzegn08wk2-non-latin-text-audit.md: G1, G2, and the sections "Bidi display, caret placement and hit-testing for RTL runs" and "Bidi fix"). CoreText paints RTL runs right. But gpui-pre LineLayout::x_for_index, closest_index_for_x and index_for_x assume glyph indices rise with x. The fe-gpui Line (crates/fe-gpui/src/element.rs: impl MeasuredLine for Line around :466; FrameLayout::char_at around :184) calls them, which causes:

- G1: buffer `let s = "שלום"; // مرحبا`, caret at 8, press Right 10 times. Offsets 9 to 31 all draw at one x, the left edge of the RTL span. There the vim block caret is a space wide (block_rect, around :2530), a selection is zero wide, and the IME window opens at that x. Expected: in logical order, each Right moves the caret one letter leftwards through the Hebrew, with the caret on the letter's logical edge, then the same through the Arabic.
- G2: buffer `let s = "שלום` (the row ends in Hebrew). Clicking the right half of the rightmost glyph ש (byte 9) puts the caret at 17, the row end. Expected 9, because ש's right edge is its logical start; the left half gives 11. Inside a span, a glyph's edges map to the reversed logical edges.

Fix, as the decision recommends:
- The fe-gpui Line does its own maths over its glyph positions (it already reads them, with_glyphs_moved) instead of calling the three GPUI functions.
- Each glyph's direction comes from unicode-bidi embedding levels: version 0.3.18, already in Cargo.lock. Add it to [workspace.dependencies] and to fe-gpui; that adds no new package. Compute levels only for rows with a strong RTL char (a byte scan); other rows keep today's path.
- A caret at a direction boundary goes on the edge of the run its logical predecessor is in (Chromium and VS Code affinity).
- Hit-testing returns the nearer logical edge of the glyph under x; x past the line width still gives the row end.
- Put the new code in a new module, crates/fe-gpui/src/element/bidi.rs.

Also add to fe_display::layout::MeasuredLine (crates/fe-display/src/layout.rs) the provided method `fn visual_ranges(&self, range: Range<usize>) -> Vec<Range<f32>>`. Its default is one range from x_for_index(start) to x_for_index(end), ordered. Line overrides it with one x range per visual piece, left to right. Nothing in fe-display calls it yet. The follow-up selection-rects card (T1431 G3) uses it, and its regression test overrides it, so keep exactly this name and signature.

Tests:
- Regression tests live in crates/fe-gpui/tests/non_latin_text.rs. It is its own test binary (fe-gpui has no tests/**/mod.rs layout), and the tests/** overlay carries it to base, so base APIs only; the helpers already there (caret_x via bounds_for_range, hit, click) are enough.
  - Replace rtl_carets_inside_an_rtl_span_share_one_x with rtl_caret_x_moves_right_to_left_through_an_rtl_span. On the G1 buffer, assert caret x at 11 > 13 > 15 > 17 and x at 9 < x at 11, and strictly decreasing across the grapheme boundaries of the Arabic.
  - Replace rtl_click_on_the_last_painted_rtl_glyph_lands_at_the_row_end with rtl_click_on_an_rtl_glyph_lands_on_its_nearer_logical_edge. On the G2 buffer: right half of ש gives 9, left half gives 11.
  - Both are macOS-only, like the tests around them. The first check that fails at base must be a bare assert(a > b) or assert_eq, with no custom message.
- Unit tests go in crates/fe-gpui/src/element/bidi_tests.rs, declared from bidi.rs, as the criteria list.
- The Linux shaper is unverified; verify this card on macOS only.

## Implementation notes

- `crates/fe-gpui/src/element/bidi.rs`: `Bidi` keeps a row's graphemes in byte order, each with its direction
  (the unicode-bidi level of its first byte, after rule L1, paragraph direction from the first strong char, as
  CoreText's natural direction) and where it's painted: from its leftmost glyph to the next painted grapheme's
  leftmost glyph, or the line's width. Graphemes with no glyph (bidi controls, zero-width chars) aren't painted
  and are passed over. `Line::new` builds it only when `has_rtl` finds an R, AL, RLE, RLO or RLI char, after a
  byte scan for a byte from 0xD6 up; `with_glyphs_moved` re-places it.
- Working per grapheme rather than per glyph keeps a click from landing between a letter and its vowel mark. A
  grapheme is placed at the glyph of its first char, so a mark offset left of its base moves neither its edges nor
  its neighbour's. A glyph-less visible grapheme after a painted one is a ligature's continuation (Arabic
  lam-alef): the glyph is split evenly between them in their direction, so every boundary inside it has its own
  caret and hit. That holds when the continuation has a mark glyph of its own (an alef with a fatha): what counts is
  that its first char has no glyph. A grapheme of only bidi controls, directional marks or BN chars is passed over.
- Limit (review round 2, F2, follow-up T317): GPUI keeps no glyph advances, so a mark that shares its base's glyph
  index and is offset left of it still moves the grapheme's left edge.
- A caret at the row's end on a row ending in right-to-left text is on the left edge of that run, by the
  predecessor rule; a click at or past the line's width still gives the row end. The bidi maths over an ASCII
  row agree with GPUI's except over the last glyph, whose left half GPUI's `closest_index_for_x` gives as the
  row end (pinned in `an_ascii_row_measures_as_gpui_does`); ASCII rows keep GPUI's maths anyway.
- `block_rect` on a bidi row covers the grapheme's painted extent (`painted_extent`, through `visual_ranges`),
  unless ghost text's inlay is at the caret; `FrameLayout::char_at` goes through `Line::char_index_at`.
- `decorations.rs` builds its after-line labels with `Line::new` now that `Line` has a third field (a one-call
  scope expansion).
- Regression tests: the spec's two macOS tests need CoreText, so the Linux gate can't run them. Their twins,
  `rtl_reordered_caret_x_moves_right_to_left_through_an_rtl_span` and
  `rtl_reordered_click_on_an_rtl_glyph_lands_on_its_nearer_logical_edge`, run everywhere in the same file over a
  test text system (`reordered`) that paints Hebrew and Arabic runs right to left as CoreText does, using base
  APIs only. Both fail at base on a bare assert. The bidi unit tests in `bidi_tests.rs` run the editor over a
  similar shaper built on unicode-bidi, and over hand-built layouts for a ligature and a mark left of its base.
- macOS: this lane runs on Linux, which can't run the CoreText tests, so `cargo nextest run -p fe-gpui --test
  non_latin_text` on macOS (C1's own run, the two new tests and the unchanged rtl_ ones) is for the Mac mirror
  card, T1503, or whoever lands this there. The Linux run of that file passes, the twins included.
- fe-display's selection rect loop orders its two x's, so a selection inside a right-to-left run, whose end's
  caret is now left of its start's, isn't an inside-out rect (`a_selection_inside_a_right_to_left_run_is_not_inside_out`).
  It is still one rect per row from x(start) to x(end): `visual_ranges` isn't called from fe-display yet.
- Not changed, for the follow-up cards: on rows with RTL text, selection rects (G3), diagnostic and syntax
  underlines (element.rs, `x_for_offset` from start to end) and the whitespace marks (`whitespace.rs`
  `xs_for_indexes`, which reads GPUI's glyph order) still span x(start)..x(end). The fold marker,
  `row_end_at` and end-of-row hints place themselves at the row end's caret, which on a row ending in RTL text
  is now that run's left edge, not the line's right edge.
