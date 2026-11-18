---
task_uid: "01m7qd6tmbjfgy0wst835zdnhf"
task: "T692"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "A long value's tooltip (f64::MIN_POSITIVE, a long f32) lies wholly inside the window in an ordinary, a narrow and a short window, and every character of the value stays reachable (wrapped, and scrolling when taller than the window)"
    required: true
    manual: false
  - id: "C2"
    text: "New rendered hover tests in crates/fe-gpui/src/hex_viewer/value_tip_tests.rs cover f32 and f64 in ordinary, narrow and short windows and check the tooltip's bounds against the window"
    required: true
    manual: false
  - id: "C3"
    text: "Existing hex_viewer and inspector tests, clippy -D warnings and the l10n lint still pass"
    required: true
    manual: false
---

# Hex viewer byte inspector: keep a long value's tooltip inside the window (cap width and height to the viewport, scroll when taller), with rendered hover tests for long f32 and f64 values in narrow and short windows (T1896 F1) (Mac T1936)

Follow-up from T1896 review (F1, overridden by the user 2026-11-17).

The byte inspector's value tooltip (`ValueTip` in crates/fe-gpui/src/hex_viewer.rs) wraps a value at 40 characters (`value_tip_lines`), but nothing constrains it to the window. In a narrow or short window a long value (f64::MIN_POSITIVE prints 326 characters; long f32 values too) can still be clipped, because GPUI only repositions an oversized tooltip.

Make the full value always readable inside the current window:
- Cap the tooltip's width and height to the window's viewport (less a margin). Wrap to the capped width instead of a fixed 40 characters.
- When the wrapped text is still taller than the cap, the tooltip scrolls, or the value is shown in a presentation that does (a lead/worker design call; record it in the decision file). Every digit must stay reachable.

Tests: render the real tooltip by hovering an inspector cell (see crates/fe-gpui/src/view/fold_hover_tests.rs for a hover pattern), for long finite f32 and f64 values, in an ordinary, a narrow and a short window, and check the tooltip's bounds lie inside the window. Put them in a new crates/fe-gpui/src/hex_viewer/value_tip_tests.rs.

## Helium filing note

- Lent from the Mac as y146 (follow-up to T1896 F1, user-approved). T1896 is already on helium main; no helium dependency.
- Tooltip pitfalls already hit here (T196 review): GPUI lays a tooltip out at min-content, so give the text a
  definite width or wrapped lines overlap; GPUI only repositions an oversized tooltip, never shrinks it. Cap
  `max_h` (and width) from the window viewport less a margin, put the text in an `overflow_y_scroll` part with
  a `ScrollHandle`, use `hoverable_tooltip` so the mouse can enter it to scroll, and `.occlude()`. Check the
  drawn bounds in tests (e.g. `on_children_prepainted` under cfg(test)).
- Register the new test module the way hex_viewer.rs registers inspector_tests.rs (a `#[cfg(test)] #[path]` mod).
- C3's clippy part is the gate's `vet` stage in a criteria file; there is no clippy stage.
- A real-window screenshot, if you take one, uses the run-fastedit-linux skill (xwd2png.py directly: `ss` parks
  the pointer and closes the tooltip). Kill only processes you started, by PID.
- Ask the lead before rgt finish if anything in the criteria can't be met as written.

## Implementation notes

- Revision 3 added `crates/fe-gpui/src/hex_viewer/inspector_tests.rs` to the scope.
  Its `a_long_float_wraps_in_its_tooltip_whole` asserted the fixed 40-char wrap that
  this card replaces. It now checks the width-based `value_tip_lines`.
- The design call (scroll inside a hoverable tooltip) is recorded in
  `docs/decisions/st835zdnhf-hex-value-tip.md`.
