---
task_uid: "01m55v37cw03cad99rcnxr03ep"
task: "T168"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display unit tests layout::inlay_hints_tests::hints_shift_the_rest_of_their_row and layout::inlay_hints_tests::the_caret_never_lands_inside_a_hint pass: hints lay out at their columns' x plus the widths before them, later offsets shift by every hint before them, offset_for_x inside a hint gives its offset, and rows without hints are as layout_frame_with_inlay lays them; they fail at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display unit tests layout::inlay_hints_tests::long_row_mappings_count_every_hint_before_the_slice and layout::inlay_hints_tests::hints_and_the_inlay_share_a_row pass"
    required: true
    manual: false
  - id: "C3"
    text: "layout_frame_with_inlay and layout_frame_without_gutter keep their signatures and results; inlay_tests, tests/inlay_props.rs, blocks.rs and block_props.rs pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-display passes with no existing test edited beyond what the spec names; cargo clippy -p fe-display --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-display: lay out any number of inlay hints inside a row beside the one inlay, with the caret, clicks and long rows stepping around each (Mac T931)

Filed on argon from Mac T931 (card ~/coordination/argon-cards/zzr/T931.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txr3s7xc0h4mknz4ae0m3. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Held in the inbox: waits until Mac T816 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T931)".

## Problem

LSP completeness track, item 3a (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
"Inlay hints ✗ (fe-display lays out a single inlay: layout.rs:605)"). VS Code draws a server's inlay hints
(rust-analyzer's `: Vec<String>` after a binding, `name:` before an argument) as text inside the row that the
caret and clicks step around. fe-display can put only one piece of text inside one row per frame:

- `layout_frame_with_inlay` (`crates/fe-display/src/layout.rs:605`) and `layout_frame_without_gutter` (`:631`)
  take `inlay: Option<&Inlay<S>>` (`Inlay`, `:206`), used for the IME composition or the AI ghost text's
  first line (`crates/fe-gpui/src/element.rs:674-680`);
- `ExpandedRow` holds `inlay: Option<InlaySpan>` (`crates/fe-display/src/lib.rs:52`), and `insert_inlay`
  (`:122`) asserts "a row takes one inlay"; `to_display` (`:140`) and `to_buffer` (`:151`) handle one span;
- `RowLayout.inlay_width` (`layout.rs:261`), used by `x_for_offset` (`:311`) and `offset_for_x` (`:336`), the
  long-row slice extension (`:816-853`), autoscroll and scroll limits (`:932-1072`), and `Frame.inlay:
  Option<Rect>` (`:200`, set at `:1234`) are all one inlay.

This card makes fe-display lay out any number of hints per row, beside the one inlay. Drawing them is the next
card.

## Change

1. `crates/fe-display/src/lib.rs` `ExpandedRow`:
   - Keeps a sorted `Vec<InlaySpan>` of hints beside today's single inlay span. New
     `insert_hints(&mut self, hints: &[(usize /* buffer col */, &str)])`, each one line, sorted by column
     (ties in the order given); the composition or ghost-text inlay is still inserted by `insert_inlay`, after
     any hint at the same column.
   - `to_display` adds the length of every span before the column (a span at the column stays after it, as
     the inlay does today); `to_buffer` maps a display column inside any span to that span's buffer column,
     and subtracts the spans before it otherwise.
   - `inlay_range()` keeps meaning the composition or ghost text; new `hint_ranges()` gives each hint's
     display columns, in order.
2. `crates/fe-display/src/layout.rs`:
   - New `pub fn layout_frame_with_hints(input, blocks, inlay, hints: &[Inlay<S>], measure, long_rows, widths,
     highlights, fold_candidates)`: as `layout_frame_with_inlay`, with each hint whose offset is in a shown
     row's shaped text drawn inside that row in its style. `layout_frame_with_inlay` and
     `layout_frame_without_gutter` call through with `&[]` and are otherwise unchanged; a gutterless variant
     with hints takes the same new parameter (`layout_frame_without_gutter_with_hints`).
   - `RowLayout`: `inlay_width` becomes the shaped width of every span in the row, so `x_for_offset` and
     `offset_for_x` are right past each hint; a caret never lands inside a hint.
   - The long-row slice and the scroll limits count every span in the row before the slice.
   - `Frame` gains `hints: Vec<(usize /* index into the hints given */, Rect)>`, one per hint laid out.
   - Hints never wrap the row (as the inlay doesn't): a row with hints may run past the wrap width. The docs
     say so; VS Code wraps them, which this leaves for later.
3. Tests: new `crates/fe-display/src/layout/inlay_hints_tests.rs`, declared in `layout.rs` beside
   `mod inlay_tests;` (`:1452`), with the monospace measure the other layout tests use:
   - `hints_shift_the_rest_of_their_row`: hints `: u8` at col 5 and `x: ` at col 9 of `let a = f(1);` lay out
     at the x positions their columns had plus the widths before them; every later offset's x shifts by both,
     and rows without hints are as `layout_frame_with_inlay` lays them.
   - `the_caret_never_lands_inside_a_hint`: `offset_for_x` at every x inside a hint gives its offset;
     `x_for_offset` of the offset is before the hint, of the next offset after it.
   - `long_row_mappings_count_every_hint_before_the_slice`: on a row longer than the shaped slice, with hints
     before and inside the slice, `x_for_offset` and `offset_for_x` round-trip every offset in the slice.
   - `hints_and_the_inlay_share_a_row`: a hint and the composition inlay at one offset lay out hint first;
     both rects are in `Frame.hints` and `Frame.inlay`, side by side.
   - The existing `inlay_tests`, `tests/inlay_props.rs`, `blocks.rs` and `block_props.rs` pass unchanged.

Linux-OK (fe-display is GPUI-free).

## Implementation notes

- `ExpandedRow` keeps one sorted `spans` list (hints and the inlay, in display order); a hint inserted at a
  column goes after the hints already there and before the inlay, whichever is inserted first.
- In `hints_shift_the_rest_of_their_row` the `x: ` hint is at col 10, not 9: col 9 of `let a = f(1);` is the
  `(`, and the hint goes before the argument `1`.
- Long rows: hints in the shaped slice are shaped in it; the row's other hints are shaped alone (once per
  frame) and kept in `RowLayout` as `outside_hints`. `start_x()` adds the ones before the slice, and
  `x_for_offset` / `offset_for_x` past the slice add every one before the offset, so positions agree inside
  and outside the slice, and a click on an unshaped hint gives its offset. The view is mapped into the
  index's coordinates past the hints before the slice is picked; the scroll limit adds the outside hints.
  The single inlay keeps its rules (only drawn when its slice can hold it).
- `inlay_width` is now the shaped width of every span in the row; the inlay's own width, which the
  inlay-slice search uses, is measured from `inlay_range()`.
- `layout_frame_with_annotation_column` still lays out no hints (it calls through with `&[]`); the drawing
  card can add a hints parameter there when the workbench needs both.
