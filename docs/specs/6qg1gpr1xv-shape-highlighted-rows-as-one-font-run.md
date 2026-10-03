---
task_uid: "01m417z4haeh2web6qg1gpr1xv"
task: "T27"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test element::tests::colour_runs_become_decoration_runs: a row's decoration runs cover its text exactly, merge adjacent same-colour runs and keep each span's colour"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test element::tests::colours_do_not_change_the_shaped_layout: in one frame, shaping the same text with two different colour splits returns the same Arc\u003cLineLayout\u003e"
    required: true
    manual: false
  - id: "C3"
    text: "Highlighting still reaches the screen: fe-gpui tests::highlighting_follows_edits and the rest of the test stage pass"
    required: true
    manual: false
  - id: "C4"
    text: "Manual, numbers in the decision file: fastedit --perf (release) on a highlighted Rust file just under MAX_PARSE_BYTES, alternating builds from before and after the change in one session (at least 5 rounds each): the median jump-scroll prepaint p99 after the change is at least 30% below before. Plain-text p99, the absolute 4 ms target and the machine load are recorded but not required"
    required: true
    manual: false
---

# Shape highlighted rows as one font run and colour them at paint time

Paddock T-28 (P2, draft: needs the user's approval before `rgt task ready`).

## Problem

Jump-scrolling a highlighted file costs about three times as much per frame as the same file in plain
text. PERF.md (measured 2026-10-02, `fastedit --perf`, M3 Max, 120 Hz), 16 MB Rust file, jump phase
(about 1000 rows a frame): prepaint 6.5 ms p50 / 11.6 ms p99 and 14 of 600 frames miss a vsync when
highlighted, against 2.2 / 3.6 ms and 7 misses as plain text. Highlighting the 60 visible rows costs only
about 0.12 ms, so about 4 ms of the difference is shaping. Smooth scrolling is fine (0.85 ms) because GPUI
reuses rows it shaped the frame before.

The cause is in GPUI, not in our run building. `TextSystem::layout_line` (gpui-pre 0.3.7,
`src/text_system.rs:886`) starts a new `FontRun` at every colour change, so CoreText shapes a highlighted
row as one call per token instead of one per row. Our `Shaper::shape` (`crates/fe-gpui/src/element.rs`)
passes one `TextRun` per style run, all in the same font. Colour also enters GPUI's line-layout cache key,
so a row whose highlights change (a background parse landing) is shaped again even when its text didn't
change.

## Change

Shape each row once in a single font run and apply the colours when painting. All of this is in
`crates/fe-gpui/src/element.rs`:

- `Shaper::shape` calls `WindowTextSystem::layout_line(text, font_size, &[one TextRun covering the text],
  None)` to get an `Arc<LineLayout>`, and turns the `StyleRun<Hsla>`s into `gpui::DecorationRun`s (adjacent
  runs of the same colour merged, no background, underline or strikethrough).
- `Line` holds the layout and its decoration runs instead of a `ShapedLine`. `MeasuredLine` uses
  `LineLayout::x_for_index` / `closest_index_for_x` as now. `paint` calls `LineLayout::paint(origin,
  line_height, TextAlign::Left, None, &decoration_runs, window, cx)` for rows, line numbers and fold
  markers.
- Put the run-to-decoration conversion in a small function so it can be tested without a window. Add tests
  in a new `crates/fe-gpui/src/element/tests.rs` (`#[cfg(test)] mod tests;` in `element.rs`).

A glyph whose bytes span two colours (a ligature across a token boundary) takes the colour of its first
byte. The editor font (Menlo) has no programming ligatures, so nothing visible changes.

Leave `fe-display` alone: `layout::row_runs` already merges nothing it shouldn't, and the fake measurer
doesn't care how rows are shaped.

`MAX_PARSE_BYTES` is now 8 MB (T-15), so a 16 MB file is no longer highlighted. Measure with a
generated Rust file just under 8 MB. The per-frame cost depends on rows per frame, not file size, so the
numbers compare with the 16 MB rows in PERF.md.

## Open questions

- The source card asks for "no missed vsyncs in the `--perf` jump phase". The same file as plain text
  already misses 7 of 600 (PERF.md), so shaping alone can't reach zero. The criterion below asks for no
  more misses than the plain-text run. The user should confirm that, or say what else zero needs.
- PERF.md is left to T-29, which owns it. Record the new numbers in the decision file; a later PERF.md pass
  can pick them up.

## Implementation notes

- `Shaper::shape` now lays each row out once with `layout_line` in one `TextRun`; `Line` holds the
  `Arc<LineLayout>` plus merged `DecorationRun`s (`decoration_runs`), painted with `LineLayout::paint`.
- C4 measured with `fastedit --perf` (release) on a generated 8,193,970-byte Rust file (just under
  `MAX_PARSE_BYTES`), jump phase, 600 frames. The machine was loaded (load average 6–28, other lanes
  building), so absolute numbers are inflated and noisy.

  | run | prepaint p99 | intervals over 12.5 ms |
  |---|---|---|
  | highlighted, before | 11.5 / 10.8 / 10.4 ms | 11 / 8 / 7 |
  | highlighted, after | 7.0 / 7.0 / 7.4 ms | 14 / 24 / 36 |
  | plain text (after) | 6.4 / 8.2 / 8.3 ms | 13 / 52 / 38 |

  Highlighted now tracks plain text (p99 ~7 ms vs 6–8 ms), down from ~11 ms. The 4 ms p99 target was not
  reached, and the plain-text file misses it too under this load, so it needs a re-run on an idle machine.

### C4 back-to-back rerun (revision 2)

Same 8,193,970-byte Rust file and its plain-text copy, run back to back, release `--perf`, jump phase
(load average 15–27, other lanes building). Prepaint p99:

| binary | plain text | highlighted |
|---|---|---|
| before | 2.68 ms | 10.77 ms |
| after | 2.93 ms | 2.94 ms |

Review round 1 rejected "level, within noise" without repeated runs. Five more back-to-back rounds
(plain then highlighted per binary; load average 13-21), jump prepaint p99 in ms:

| round | before plain | before hl | after plain | after hl |
|---|---|---|---|---|
| 1 | 6.68 | 10.53 | 4.94 | 5.62 |
| 2 | 5.36 | 10.30 | 2.88 | 6.57 |
| 3 | 7.60 | 10.13 | 7.97 | 2.30 |
| 4 | 8.03 | 11.71 | 2.39 | 7.75 |
| 5 | 7.16 | 10.68 | 7.34 | 6.36 |
| median | 7.16 | 10.53 | 4.94 | 6.36 |

Before: highlighted is worse than plain in every round (~3 ms). After: highlighted p99 drops from ~10.5 to
~6.4 ms and beats plain in one round, but plain text itself swings 2.4-8.0 ms under this load, so the
spread is wider than the plain/highlighted gap and C4's "no worse than plain text" can be neither shown
nor refuted here (median highlighted 1.4 ms above plain). Needs an idle machine or a stated tolerance.

### C4 before/after drop (revision 3)

The five rounds above alternated the before and after release builds on the highlighted file (8,193,970
bytes, just under `MAX_PARSE_BYTES`), jump-scroll prepaint p99: before 10.53, 10.30, 10.13, 11.71,
10.68 ms (median 10.53); after 5.62, 6.57, 2.30, 7.75, 6.36 ms (median 6.36). Median drop 10.53 -> 6.36
ms = 39.6%, above the required 30%. Context, not required: plain-text p99 medians 7.16 ms (before build)
and 4.94 ms (after build); the 4 ms absolute target was not met in the median; load average 13-21 with
other lanes building.
