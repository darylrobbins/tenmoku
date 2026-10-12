---
task_uid: "01m4rgwgg4rka7aa3x26kqm6vz"
task: "T39"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test inlay_props: proptests for unchanged-elsewhere, shift by inlay_width, and offset_at round trip (including a point inside Frame::inlay) pass at 256 cases each"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test block_props: proptests for row placement under blocks (and equality with layout_frame with no blocks) and offset_at round trip pass at 256 cases each"
    required: true
    manual: false
  - id: "C3"
    text: "Generators cover multi-byte text, tabs, soft wrap on and off, folds, horizontal scroll and a row past WRAP_SLICE_BYTES"
    required: true
    manual: false
  - id: "C4"
    text: "Nothing outside the two new test files and this lane's docs/specs file changes; any failing property is kept as a named #[ignore] case listed in the decision file"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-display and cargo clippy -p fe-display --all-targets -- -D warnings pass; each new file runs in under 10 s in a debug build"
    required: true
    manual: false
---

# Property-test the inlay and block layouts in fe-display: positions shift past an inlay or block only, and every shown offset round-trips through hit testing (Mac T499)

Filed on helium from Mac T499 (~/coordination/helium-cards/T499.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-u.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Helium notes (from the Mac lead, note u)

- Mac T484 (the gutter card) is helium T33 and has already landed here (3887127), but the change stays new test files only. Keep any failing property as a named #[ignore] case listed in the decision file, as criterion C4 says, and don't fix layout.rs here.

## Problem

The plan's risk 4 ("Display-map complexity once inlays and blocks are added") is mitigated by "Add one layer at a
time, with round-trip property tests". Wrap, fold, long rows and the display map have proptests (`wrap_map.rs`,
`fold_map.rs`, `long_row.rs`, `display_map/tests.rs`). The two layers added since have only example tests:

- blocks (T254: `BlockMap`, `layout_frame_with_blocks`, `crates/fe-display/src/layout.rs` :555; examples in
  `crates/fe-display/tests/blocks.rs`), which proposal diff blocks (T255) build on;
- the inlay (T389: `layout_frame_with_inlay`, `layout.rs` :580; examples in `layout/inlay_tests.rs`), which now
  carries IME composition (T429) and ghost text (T388), and shifts every position on its row, long-row slices
  included (`layout.rs` :710-740).

`rg proptest crates/fe-display/src/block_map.rs crates/fe-display/tests/blocks.rs
crates/fe-display/src/layout/inlay_tests.rs` finds nothing.

## Change

Test-only: two new integration tests using the public `fe_display` and `fe_display::layout` API, each with its own
copy of a monospace fake measurer like `tests/blocks.rs`'s (`Mono`, `Line`). proptest is already a dev-dependency.
Generated inputs: text of ASCII, multi-byte chars and tabs (a row past `WRAP_SLICE_BYTES` in some cases), soft
wrap on and off, folds in some cases, a vertical and horizontal scroll.

`crates/fe-display/tests/inlay_props.rs`, with one inlay (1 to 20 chars, possibly multi-byte) at a random char
boundary, or none:

1. **Unchanged elsewhere.** Every row but the inlay's display row lays out as without the inlay: same shown range
   and the same `x_for_offset` for each offset.
2. **Shift.** On the inlay's row an offset before the inlay keeps its x; one at or after it moves right by the
   row's `inlay_width`, which equals the inlay text's measured width.
3. **Round trip.** For every offset `o` shown in the frame, `Frame::offset_at` at `o`'s position gives `o` back,
   and a point inside `Frame::inlay` gives the inlay's offset.

`crates/fe-display/tests/block_props.rs`, with 0 to 5 blocks of 1 to 4 rows at random anchors:

4. **Placement.** Each display row's top is its top without blocks plus the rows of the blocks anchored at or
   above it, and with no blocks the frame equals `layout_frame`'s.
5. **Round trip.** For every offset `o` shown, `Frame::offset_at` at `o`'s position gives `o` back; a point in a
   block row clamps as `offset_at`'s doc (`layout.rs` :440-448) says.

Each property runs proptest's default 256 cases (`PROPTEST_CASES` overrides) and each file finishes in a few
seconds in a debug `cargo test`.

If a property fails on main, don't fix `layout.rs` or `block_map.rs` here: keep the minimized case as an
`#[ignore = "<layer> layout bug: <one line>"]` test in the same file, narrow the generator so the rest pass, and
name each ignored case in the decision file so the lead can file the bugfix.

## Implementation notes

- Ignored cases, for the lead to file the bugfix:
  - `inlay_props::point_near_the_bottom_of_a_row_far_down`,
    `#[ignore = "inlay layout bug: offset_at adds scroll.y to y in f32, so a
    point just above the bottom of a row far down hits the next row"]`: with
    the row 600000 px down, a point 0.02 px above the bottom of the inlay's
    rect gives the next row's offset. `Frame::offset_at` computes
    `(y + scroll.y) / line_height` in `f32`, which is a sixteenth of a pixel
    apart there; doing that sum in `f64` (as `view_x` does across) would fix
    it. Not inlay-specific: any point that close to a row's bottom far down
    does it. Found by `offset_at_round_trip` probing at 0.999 of the inlay's
    height (review round 2, F3); the generated probes now stay 1 px inside the
    far edges so the rest pass.
- Otherwise no property failed on main, at 256 cases or in runs of 5000 cases
  per property.
- Both files share a document generator (copied, as the spec asks): 1 to 23
  rows of ASCII, spaces, tabs and 2-, 3- and 4-byte chars (each its own
  grapheme); in 25% of cases one row is replaced by a unit repeated past
  `WRAP_SLICE_BYTES`; soft wrap at 4 to 39 cells or off; 0 to 2 folds; a caret
  anywhere, autoscroll on or off, and a scroll across (up to 1.2 times the
  widest row) and down (in quarter pixels). The fake `Line` keeps each char's
  start, so measuring every offset of an 8 KiB slice stays cheap. The x and
  round-trip checks take every caret boundary shown (review round 1, F1).
- Inlays go anywhere, or within 40 chars of the caret, so that autoscroll
  brings them into a long row's slice. Blocks are anchored anywhere, within 6
  rows of the caret's row, or after the last row.
- Shift: the caret at the inlay's own offset is drawn before the inlay (as
  `inlay_tests.rs` has it), so offsets up to and including the inlay's keep
  their x, and only those after it move by `inlay_width`. An inlay past what's
  shaped of its row (a row too long to shape whole, with soft wrap on and
  nowhere to wrap, such as a long run of spaces) isn't drawn and moves
  nothing, as `layout_frame_with_inlay`'s doc says; the property checks that
  case too.
- Unchanged elsewhere: the inlay widens the row it's on, so it can raise the
  furthest horizontal scroll, or move where autoscroll puts the caret, and a
  long row's slice follows the scroll. So the frame without the inlay is
  compared, in full (shown range, shown text and x of every offset), with the
  inlay laid out at the scroll it settled on, autoscroll off; the inlay can
  only widen the furthest scroll, so that scroll stands (review round 1, F2).
  It's also compared with the inlay's own frame: x everywhere, and shown range
  and text wherever the scroll matches or the row isn't sliced.
- The round trip's points on the inlay stay at least a pixel inside its far
  edges: with a long row wrapped at 4 cells, rows sit about a million pixels
  down, where the ignored case above bites.
- Block round trip: a point on a block in the frame gives its anchor's offset,
  even past the frame's edge; a point on a block row outside the frame gives
  what a point at the same x on the display row below it does.
- Run times (debug, 256 cases, the three properties of a file in parallel):
  `inlay_props` 2 to 5.5 s wall and about 10 s CPU, `block_props` 2.4 to 4.5 s
  wall and about 8 s CPU, measured on a 24-core host at load average 35 to 43
  (other lanes building), where the odd run reached 8 s; earlier, less loaded
  runs took 2 to 3 s. Every caret boundary is checked (review F1); the third
  layout in `unchanged_elsewhere` is only made when the scrolls differ, and
  the generators reach the long row in a quarter of cases. Each property also
  passed 15000 cases.
