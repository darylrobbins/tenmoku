---
task_uid: "01m4bfbtgvpjj8gybnwgj14gc7"
task: "T167"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Padding: fe-display test layout::fold_column_tests::the_column_pads_the_chevron (em 10, line height 32): frame.fold_column is 37.5 wide (3.75 em), ends at text_left and spans the frame's height; line numbers end at its left edge; text_left is 87.5 for a file under 1000 rows (gutter_width = em × (digits + 5.75)); each fold toggle's rect is the column's width on its row; chevron_box is a 22.5 square (2.25 em) centred on the column and the row, leaving 7.5 (0.75 em) on each side"
    required: true
    manual: false
  - id: "C2"
    text: "Size: fe-display test layout::fold_column_tests::the_chevron_fits_short_rows (em 10, line height 20): chevron_box is 17.5 square (0.875 × line height), still centred on the column and the row"
    required: true
    manual: false
  - id: "C3"
    text: "Check: cargo test -p fe-display passes, with only the gutter constants changed in layout/tests.rs (GUTTER 70 → 87.5) and fold_gutter_tests.rs (the 2-em toggle width → 3.75 em); cluster_row_tests.rs and long_row_tests.rs change only where an expectation depends on the gutter's width"
    required: true
    manual: false
  - id: "C4"
    text: "Expanded chevrons on hover: fe-gpui test view::fold_hover_tests::open_fold_chevrons_show_only_while_the_pointer_is_over_their_column: no unfolded chevron is painted before the pointer moves; moving into the fold column, on a candidate's row or on another row, paints every visible candidate's chevron; moving over the line numbers or the text paints none; moving back in and then a MouseExitEvent paints none; a second move within the column doesn't notify the view"
    required: true
    manual: false
  - id: "C5"
    text: "Collapsed chevrons always: fe-gpui test view::fold_hover_tests::folded_chevrons_always_show: with a row folded, its chevron is painted while the pointer has never moved, is over the text, and after a MouseExitEvent, while an unfolded candidate in the same frame isn't"
    required: true
    manual: false
  - id: "C6"
    text: "Centred: fe-gpui test view::fold_hover_tests::chevrons_paint_in_their_boxes: each painted chevron's recorded bounds equal Frame::chevron_box of its toggle in window coordinates; the chevron is a stroked path, down when unfolded and right when folded, not a font glyph"
    required: true
    manual: false
  - id: "C7"
    text: "Clicks: fe-gpui test view::fold_hover_tests::the_whole_column_toggles: clicks 1 px inside the column's left and right edges on a candidate's row fold it and then unfold it, without hovering first; the existing fold_gutter_tests pass unchanged"
    required: true
    manual: false
  - id: "C8"
    text: "Check: after UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots, a plain cargo run -p fe-gpui --example screenshots passes; the 8 regenerated goldens show text 1.75 em further right and no chevrons; the finish notes say so"
    required: true
    manual: false
---

# Fold chevrons: pad and centre them like VS Code's, and show open-fold chevrons only while hovering the fold column

The user asked for cosmetic changes to the gutter's fold chevrons (T31, review follow-ups T51 and T57).
They want more space on either side of each chevron. They want chevrons drawn larger and centred in
their column, the way VS Code draws its fold chevrons. Chevrons of folds that are open should show
only while the pointer is over the chevron column.

## Decisions (approved by the user, 2026-10-07)

- **Hover area**: only the fold column, the strip between the line numbers and the text, shows the
  chevrons. Hovering the line numbers or the text doesn't.
- **Expanded vs collapsed**: chevrons of rows that can fold, but aren't folded, show only while the
  pointer is over the fold column. Chevrons of folded rows always show, so a fold can still be found
  without hovering. The ⋯ marker after a folded row doesn't change.
- **No fade**: chevrons appear and disappear at once. VS Code fades them over 0.5 s, but we don't
  animate. Out of scope: an "always show" setting.

## Today

- `crates/fe-display/src/layout.rs`: `gutter_width` is `em * (digits + 4)`. Line numbers end
  `em * 2` left of `text_left`. Each `FoldToggle.rect` is that `2 em × line_height` strip on the row.
- `crates/fe-gpui/src/element.rs`: each toggle is drawn as the text glyph `›` (folded) or `⌄`
  (unfolded). It uses the editor font and `gutter_foreground`, centred across the 2-em strip and
  sitting on the text baseline. With a monospace font the glyph is 1 em wide, so there is about
  0.5 em (≈4 px at 13 px Menlo) on each side. `⌄` sits low in the row. All chevrons are always
  painted.
- `crates/fe-gpui/src/view.rs`: `on_mouse_move` does nothing unless a drag is in progress.
  `toggle_fold_at` hit-tests `Frame::fold_click_areas` on mouse down.

## VS Code's chevrons (reference)

These come from `src/vs/editor/contrib/folding/browser/folding.css` and `editorOptions.ts`.

- The folding column adds 16 px to the line-decorations width, which defaults to 10 px. So the column
  is 26 px wide.
- The chevron is the `chevron-down` or `chevron-right` codicon at `font-size: 140%` of the editor font
  size, centred with `display: flex; align-items: center; justify-content: center`. At 12 px / 18 px
  line height, that is a 16.8 px box (about 0.93 × line height). The glyph's stroke spans about 60% of
  the box across and 30% along.
- It has `opacity: 0` until `.margin-view-overlays:hover`. `codicon-folding-collapsed` always has
  `opacity: 1`.

## Change

### Metrics (`crates/fe-display/src/layout.rs`)

The new metrics are in ems, so layout still needs only `em` and `line_height`. The table gives values
at the default 13 px font, 1.6 line height (20.8 px), em ≈ 7.8 px, and at the tests' em of 10 and line
height of 20:

| | ems | default | tests |
|---|---|---|---|
| chevron box `S` = min(2.25 em, 0.875 × line height) | 2.25 | 17.6 px (VS Code at 13 px: 18.2) | 17.5 (clamped) |
| padding each side of the box | 0.75 | 5.9 px | 10 when clamped |
| fold column `W` | 3.75 | 29.4 px (was 15.7) | 37.5 (was 20) |
| `gutter_width` = em × (digits + 5.75) | +1.75 | text moves 13.7 px right | 87.5 (was 70) |

- Add constants `FOLD_COLUMN_EMS = 3.75`, `FOLD_CHEVRON_EMS = 2.25` and
  `FOLD_CHEVRON_LINE_FRACTION = 0.875`. Use them in `gutter_width`, the line-number origin (numbers
  now end `W` left of `text_left`, so they stay where they were), and each toggle rect (`x:
  text_left - W`, `width: W`). The whole column on a row is the hit area, which is wider than before.
- `wrap_width` follows `gutter_width`, so soft-wrapped text is 1.75 em narrower.
- `Frame` gains `fold_column: Rect` (`x: text_left - W`, `y: 0`, `width: W`, `height: frame height`)
  and `fold_chevron: f32` (`S`). It also gains `Frame::chevron_box(&FoldToggle) -> Rect`: an `S × S`
  square centred on the toggle rect's centre, which is the column's centre across and the row's
  centre down.
- Update the tests that pin the old gutter: `layout/tests.rs` (`GUTTER` 70 → 87.5, plus the "Numbers
  end 2 em left" comment) and `layout/fold_gutter_tests.rs` (`GUTTER - 2. * CELL` / `2. * CELL` → the
  3.75-em column). `cluster_row_tests.rs` and `long_row_tests.rs` are in scope only in case an
  expectation there depends on the gutter's width. Change nothing else in them.

### Paint (`crates/fe-gpui/src/element.rs`)

- Draw each chevron as a stroked path (`gpui::PathBuilder::stroke`, `window.paint_path`) in
  `chevron_box`. Don't use a font glyph, so the size and centring don't depend on the font. Use
  `gutter_foreground` and a stroke width of `max(1 px, S / 12)` (≈1.5 px), with the box centre at
  `(cx, cy)`:
  - Unfolded (down): `(cx - 0.3S, cy - 0.15S) → (cx, cy + 0.15S) → (cx + 0.3S, cy - 0.15S)`.
  - Folded (right): `(cx - 0.15S, cy - 0.3S) → (cx + 0.15S, cy) → (cx - 0.15S, cy + 0.3S)`.
  - Drop `FOLDED_CHEVRON`, `UNFOLDED_CHEVRON` and the shaped `chevrons` lines.
- Paint folded toggles always, and unfolded toggles only while `EditorView::fold_column_hovered` is
  set.
- In prepaint, insert a hitbox over `frame.fold_column`. In paint, register a window mouse-move
  listener (`window.on_mouse_event::<MouseMoveEvent>`, bubble phase) and a `MouseExitEvent` listener.
  They set the view's `fold_column_hovered` to `hitbox.is_hovered(window)` (`false` on exit), and
  `cx.notify()` only when the flag changes. The listener is window-wide, so leaving the editor for
  another part of the window also clears the flag. The pointing-hand hitboxes over toggles and markers
  stay as they are.
- `#[cfg(test)]`: record what was painted as `painted_chevrons: Vec<(u32, bool, Bounds<Pixels>)>`
  (buffer row, folded, box in window coordinates) on the view, as `painted_carets` is recorded.

### View (`crates/fe-gpui/src/view.rs`)

- Add `fold_column_hovered: bool`, set to `false` in `new`.
- `toggle_fold_at` is unchanged. Clicking a toggle anywhere in its (now wider) rect still toggles,
  whether or not its chevron is showing.
- New GPUI tests go in `crates/fe-gpui/src/view/fold_hover_tests.rs`, declared from view.rs by
  `#[cfg(test)] mod fold_hover_tests;`, next to the existing `language_tests` / `window_tests` /
  `reveal_tests`. That keeps lib.rs out of scope, since ready T145, T146 and T147 hold it. Copy the
  small `editor()` / `center_of()` harness from `fold_gutter_tests.rs`. Move with
  `cx.simulate_mouse_move(.., None, Modifiers::none())` and leave with
  `cx.simulate_event(MouseExitEvent { .. })`. Count notifications with `cx.observe(&view, ..)`, as
  `project_panel_hover_tests.rs` (T149) does.

### Golden screenshots

All 8 goldens in `crates/fe-gpui/examples/screenshots/golden/` change. The text starts 1.75 em further
right, and no unfolded chevrons show, because no scene hovers the gutter or folds a row. Regenerate
them with `UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots` (TESTING.md
"Screenshots"; it renders headless, so agents can run it). Then run it again without the variable to
confirm the scenes match. Look at the PNGs before handing in.

## Conflicts

- Ready T114, T145, T146 and T147 hold element.rs and view.rs. Ready T162 holds view.rs. T114 (hover
  underline), T145 and T146 (hover popovers) also add mouse-move handling to the editor. Whichever
  lands second rebases onto the other's mouse-move listener.
- Ready T81 adds a workbench golden that shows the editor gutter. If T81 lands first, its
  `workbench.png` needs regenerating after this card. That golden is outside this scope, so it would
  be a follow-up. If this card lands first, nothing more is needed.
