---
task_uid: "01m5f7gvnrjg1et527ythswzz0"
task: "T1034"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Row runs are cached per display row, spliced on row inserts/deletes, dropped only for edited or re-highlighted rows, never for an unchanged row (an unchanged empty row keeps its tile across different chunk splits and frames stop), and bounded (minimap_cache_tests.rs, minimap_tile_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Typing a char rebuilds only its row's tile; inserting a row rebuilds its tile and shown tiles below it only; scrolling by a row rebuilds none; a theme change rebuilds all; dropped tiles are released with drop_image and live tiles stay bounded after scrolling a 1M-row file end to end (minimap_tile_tests.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "A frame stops building tiles at its budget and requests another frame; unbuilt tiles show the background"
    required: true
    manual: false
  - id: "C4"
    text: "cargo bench -p fe-gpui --bench minimap runs smooth/1m-plain, jump/7mb-rust, jump/1m-plain-wrapped and 7mb-rust typing with the minimap off and on, reporting per-frame draw cost (the between-frame sweep is not included, a limit PERF.md documents and the follow-up card tracks); PERF.md gets a Minimap section with both, A/B by fastest sample; minimap-on frame p99 \u003c= 8.33 ms on smooth and typing, and jump p50 within 0.5 ms of minimap-off"
    required: true
    manual: false
  - id: "C5"
    text: "The minimap golden from card 2 is unchanged"
    required: true
    manual: false
---

# Draw the minimap from cached row runs in tile images rebuilt only where rows changed, within a per-frame budget, with a minimap bench in PERF.md

# Minimap 3/5: cache the minimap's rows and draw them from tile images rebuilt only where rows changed, with a bench

Card 2 builds the minimap's runs from highlight spans and paints one quad per run every frame. For a
big file that is ~10x the rows the text lays out (500 minimap rows on a 1,000 px editor vs ~50 text rows),
redone on every scroll frame, and card 5's character rendering would be tens of thousands of cells.
This card makes it incremental and off the frame budget. Mac-only (GPUI images, the bench).

## Design

- Row runs cache (fe-display, beside card 1's `row_runs`, GPUI-free so it unit-tests headless):
  runs per display row, valid for a buffer version, spliced on edits (rows inserted/removed shift the
  entries after them; only edited rows are dropped) and invalidated for row ranges whose highlights
  changed (a reparse or injected language landing, a theme or bracket-colour change drops all). Bounded:
  keep at most a few thousand rows around what the minimap last showed.
- Tiles (crates/fe-gpui/src/element/minimap.rs): the minimap is drawn from `RenderImage` tiles of a fixed
  number of display rows (e.g. 64) at device pixels (`window.scale_factor()`), with a transparent
  background so card 4's marks can go underneath, painted with `window.paint_image`. A tile is rebuilt
  only when one of its rows' runs changed, the colours, scale or max column changed, or the scale factor
  did; scrolling reuses tiles and only moves them. Tiles leaving a small LRU are released with
  `window.drop_image` (no atlas leak: a test counts live tiles after scrolling a 1M-row file end to end).
- Budget: building runs and tiles for a frame stops after a fixed time (e.g. 1.5 ms); tiles not yet
  built draw nothing (the background shows) and the element asks for another frame
  (`request_animation_frame`), as soft wrap catches up today. A jump scroll therefore never pays for
  the whole minimap in one frame. If highlight queries dominate, building runs on a background
  executor from a snapshot is acceptable instead; say which in the PR.

## Bench

New `crates/fe-gpui/benches/minimap.rs` (`[[bench]] name = "minimap"`, harness = false, in
crates/fe-gpui/Cargo.toml), on the scroll bench's harness (`#[path = "keystroke/harness.rs"]`; add only
what's needed to turn the minimap on): `smooth/1m-plain`, `jump/7mb-rust`, `jump/1m-plain-wrapped` and a
typing case in `7mb-rust`, each with the minimap off and on, reporting frame p50/p99 and
prepaint/paint as scroll.rs does. Compare A/B by fastest sample, alternating runs (PERF.md
"Measuring"). Record the numbers in a new PERF.md section "Minimap (`cargo bench -p fe-gpui --bench
minimap`)".

## Tests (new files)

- crates/fe-display/src/minimap_cache_tests.rs: splice on insert/delete rows keeps the other rows'
  runs; an edit drops only its rows; invalidating a range; the bound.
- crates/fe-gpui/src/element/minimap_tile_tests.rs: typing a char rebuilds only the tile of its row;
  inserting a row rebuilds its tile and the shown tiles below it, none above; scrolling by a row
  rebuilds nothing; a theme change rebuilds all; live tiles stay bounded and dropped ones are released;
  a frame over budget leaves tiles for the next frame and asks for it.
- Card 2's `minimap` golden is unchanged (tiles look the same as quads).

## Review round fixes

### F7 (required): empty rows get a chunk-independent decoration stamp

`Builder::extras` (crates/fe-gpui/src/element/minimap.rs, the stamp loop after the chunked decoration
query) clips each chunk's foreground spans to the chunk, so a span that crosses an empty row between two
chunks is cut short of it. The empty row is stamped 0 when only its own chunk is built (a stale-row
rebuild) and nonzero when a whole tile is swept, so unchanged empty rows are invalidated again and again
and frames never stop (C1).

Fix: a row with no text to paint gets the same stamp however its rows were chunked. Preferred: skip
stamping rows whose drawn range is empty (they paint nothing, so no decoration or fade over them can
change what is drawn); such a row stays at stamp 0 in every chunk split. Equivalent alternatives are
acceptable if the stamp is provably chunk-independent. Do not change the stamps of rows that have text.

Regression test, in crates/fe-gpui/src/element/minimap_tile_tests.rs (beside
`a_decoration_over_a_gap_between_chunks_stamps_its_rows_the_same_each_time`, reusing `Everything`,
`settle` and `built`): `an_unchanged_empty_row_keeps_its_tile_across_different_chunk_splits`.
It draws a file with an interior blank row at a build-chunk boundary (e.g. row 15 of a tile, and a second
blank row after a long row so a gap splits the chunks) under the `Everything` provider, settles, then
draws the same rows again through a different chunk split (a zero-budget build of rows 0..=15 followed by
a sweep of the whole tile, then a stale-row-only rebuild), and asserts every frame run stops
(`settle(..) < limit`) and `built(..)` is empty: no tile is rebuilt. It must fail on the current
branch (the blank row's stamp flips 0 / nonzero, frames never stop) and pass with the fix.

### C4 narrowed (documented limit; F8 moves to a follow-up card)

C4 is met when `cargo bench -p fe-gpui --bench minimap` runs the four cases with the minimap off and on,
reports per-frame draw cost (frame p50/p99, prepaint/paint), and PERF.md's Minimap section has the
numbers A/B by fastest sample with the thresholds below. The benchmark does not yet deliver GPUI's
scheduled frame callbacks (`window.on_next_frame`, so `sweep_step` and the build continuations do not run
in the timed cases) and its settle helpers do not drain them; that is a known limit. PERF.md's Minimap
section must say so in one or two sentences: "The numbers are per-frame draw cost; the between-frame
sweep's scheduled work is not in them (tracked by the follow-up card, 'Include the minimap's between-frame
sweep in the minimap benchmark')". Do not change the bench to drive callbacks in this card. The thresholds
(minimap-on frame p99 <= 8.33 ms on smooth and typing, jump p50 within 0.5 ms of minimap-off) apply to
the per-frame draw cost as measured.

### Criteria for this revision

C1 and C2, C3, C5 as before. C4 is reworded (see the `--criteria` list in the respec command).
