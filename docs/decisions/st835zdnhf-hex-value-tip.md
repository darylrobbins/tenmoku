# Hex viewer value tooltip fits the window (T692)

The byte inspector's value tooltip (`ValueTip` in
`crates/fe-gpui/src/hex_viewer.rs`) used to cut a value into 40-char lines
and let GPUI place the result. GPUI lays a tooltip out at its min-content
size and, when it overflows the window, only moves it: it never shrinks it.
`f64::MIN_POSITIVE` (326 chars) at 40 chars a line is 9 lines; in a short
window those ran past the bottom, and a 40-char line is wider than a narrow
window.

## What ships

- **Width capped to the window.** The tooltip's box is at most the
  viewport's width less 8 px at each side (`TIP_MARGIN`). The text is
  shaped once on one line (`shape_line`, 12 px in the window's text style),
  and `value_tip_lines` breaks it greedily at char boundaries where a line
  would pass the room inside the box (the cap less padding, border and a
  pixel of slack). The box is set to its widest line's width (`.w`), since
  GPUI's min-content layout would otherwise lay wrapped text out one line
  high. The fixed 40-char wrap is gone: an ordinary window shows
  `f64::MIN_POSITIVE` on three lines.
- **Height capped, the text scrolls.** The box has `max_h` of the viewport's
  height less the same margins. Its lines sit in an `overflow_y_scroll` part
  with a `ScrollHandle` kept by the tooltip's view, so a value taller than the
  cap scrolls inside the tooltip.
- **Hoverable.** The cell uses `hoverable_tooltip`, so the mouse can move into
  the tooltip and scroll it without it closing; the box `occlude()`s what is
  under it.

### Why scroll in the tooltip, not another presentation

The spec left it open: scroll the tooltip, or show the value somewhere that
scrolls (a popover, a copy action). Scrolling the tooltip keeps the one
gesture users already have, hover, and needs no new command, key binding or
string. The tall case is rare: at the cap's width only a very short window
(under about 85 px tall for `f64::MIN_POSITIVE` at 1000 px wide) makes it
scroll. The same pattern ships for git log's commit cards (T196).

## Small windows

There is no minimum window size, so `TipFit` gives way axis by axis. When
the window has no room for the 8 px margins, the padding, the border and one
glyph across (or one line down), the margins go. When it can't hold the
padding and border and a glyph (or a line), those go too, and the box takes
the whole window. The text part scrolls on both axes, and its column is as
wide as its widest line. So even in a window narrower than one glyph, or
shorter than one line, every character can be scrolled into view (T692
review F1).

Below about 17 px across, or about 33 px down at an ordinary width, the
strip's cells are no longer in the window, so no tooltip can be opened by
hovering. The rendered tests go down to 20×600 and 1000×34; `TipFit` is
unit-tested below that, down to 0×0.

## Limits

- Placement is GPUI's: below and right of the mouse, flipped or put at the
  window's edge when that overflows. With the box capped, every placement
  lies inside the window, but not always inside the margin.
- A wheel event scrolls one axis at a time (GPUI's rule), so a window
  narrower than a glyph scrolls across and down in separate gestures.

## Tests

`crates/fe-gpui/src/hex_viewer/value_tip_tests.rs` hovers the f32 and f64
cells for `f32::MIN_POSITIVE` and `f64::MIN_POSITIVE` in an ordinary
(1000×600), a narrow (200×600), a short (1000×80), a narrow, short (220×260)
window, and the smallest windows that still show a cell: 30×600 (no room
for the margins), 20×600 (none for the padding either) and 1000×34 (two
pixels of the cell show). Each test checks the following:

- the tooltip's drawn bounds lie inside the window;
- its text part has room in it;
- its lines join up to the whole value;
- each line fits across or scrolls across.

When the tooltip scrolls, the test moves the mouse in and wheels to the
end. It then checks that the last line and the widest line's end are drawn
inside the text part, with the tooltip still where it was. Two unit tests
check `TipFit`'s tiers and windows smaller than a glyph.
`inspector_tests::a_long_float_wraps_in_its_tooltip_whole` checks
`value_tip_lines` with fixed glyph widths.
