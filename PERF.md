# Performance

How fastedit is measured, and the latest numbers against the plan's targets.
Release builds on an M3 Max MacBook Pro (120 Hz built-in display), measured
2026-10-02 at `m2-bench`.

## Measuring

```zsh
cargo bench -p fe-syntax                          # parse, reparse, highlight (~8 min)
cargo bench -p fe-syntax -- 10k                   # just the 10k-line cases
cargo bench -p fe-buffer                          # load and edit (M1)
cargo build --release -p fastedit
target/release/fastedit --perf path/to/file       # open, scroll, memory, idle; then quits
```

Close CPU-heavy jobs first (soak runs, builds): with the machine loaded,
numbers come out several times worse and noisy.

`--perf` opens a real window and prints:

- **open:** time from launch to the first painted frame;
- **smooth scroll:** 600 frames scrolling 40 px (about two rows) each;
- **jump scroll:** 600 frames scrolling about 1000 rows each, so every frame
  shows rows that haven't been shaped yet;
- **prepaint/paint:** the editor element's own time per frame, from
  `EditorView::frame_timings()`; *interval* is the time between painted
  frames, so 8.3 ms is 120 fps and any interval over 12.5 ms missed a vsync;
- **memory:** the process's physical footprint (what Activity Monitor shows),
  which on Apple Silicon includes GPU memory;
- **idle:** CPU used over 5 s with nothing happening, starting 2 s after the
  last scroll (GPUI keeps presenting for a second after fast input).

The first launch after a build is slower (cold caches): one run took 490 ms
to the first frame and stalled for 3.6 s while scrolling. Repeat runs don't.
Numbers below are from repeat runs.

The tables were measured before soft wrap and folding landed. Rechecked on
top of them, the 100 MB and highlighted 16 MB files give the same open time,
memory, missed vsyncs and jump-scroll costs within noise. Smooth-scroll
prepaint on the 16 MB file is 0.3 ms higher (1.15 ms p50), from keeping the
wrap map up to date each frame.

GPUI's own `profiler` feature (gpui-pre) adds window-level histograms (draw
time, dirty-to-present, present interval, input latency) and a debug frame
overlay. It isn't enabled. `--perf` measures the editor element directly,
which is enough for these targets.

## M2 large-file targets

| Target | Result | |
|---|---|---|
| 1M-line file interactive in < 500 ms | 33 MB: 189 ms to first frame (load 17 ms). 100 MB: 201 ms (load 43 ms) | ✅ |
| Memory ≤ 1.5× file size | Unhighlighted: 100 MB file costs 1.46×, 33 MB 1.48×, over a 21 MB empty editor at open. **Highlighted: a 16 MB Rust file costs 630 MB more than the same file unhighlighted (~39×)** | ❌ for highlighted files |
| Scrolling holds 120 fps | Smooth scrolling: 120 fps on every file, 0–1 missed vsyncs in 600 frames. Jump scrolling: 119–120 fps, but a highlighted 16 MB file spends 6.5 ms p50 in prepaint and misses 14 vsyncs | ✅, little headroom when highlighted |
| Idle CPU near zero | 0.8–0.95% of a core, no frames painted | ✅ |
| UI never blocks on a parse | A frame tries at most 1 ms of reparse, then moves it to a background thread | ✅ by design; see reparse below |

Every 1M-line file tried is over `MAX_PARSE_BYTES` (16 MB), so the 1M-line
runs are unhighlighted. The largest highlighted file is 16 MB (486k lines of
Rust).

### Scrolling, per frame (p50 / p99)

| File | Smooth: prepaint | Smooth: paint | Jump: prepaint | Jump: paint | Missed vsyncs (smooth, jump) |
|---|---|---|---|---|---|
| empty | 0.03 / 0.08 ms | 0.01 / 0.02 ms | 0.03 / 0.07 ms | 0.01 / 0.02 ms | 2, 3 |
| 10k lines Rust, highlighted | 0.85 / 2.9 ms | 0.22 / 0.64 ms | — ¹ | — ¹ | 1, 4 |
| 16 MB Rust, highlighted | 0.85 / 3.5 ms | 0.24 / 0.59 ms | 6.5 / 11.6 ms | 0.11 / 0.57 ms | 1, 14 |
| 16 MB, plain text | 0.36 / 0.97 ms | — | 2.2 / 3.6 ms | — | —, 7 |
| 1M lines (33 MB), plain | 0.32 / 0.97 ms | 0.21 / 0.71 ms | 2.4 / 3.8 ms | 0.15 / 0.57 ms | 0, 4 |
| 1M lines (100 MB), plain | 0.29 / 0.48 ms | 0.62 / 1.2 ms | 1.2 / 1.8 ms | 0.55 / 0.78 ms | 0, 0 |

¹ Jumps reach the end of a 10k-line file within ten frames, after which
nothing changes.

Highlighted rows are slow to shape. Highlighting 60 rows takes about 0.12 ms
(see below), but a jump frame in the highlighted 16 MB file takes 4.3 ms more
than the same file as plain text. The difference is shaping each row in many
coloured runs instead of one.

### Memory (physical footprint)

| File | At first frame | After scrolling |
|---|---|---|
| empty | 21 MB | 276 MB |
| 10k lines Rust (0.3 MB) | 23 MB | 301 MB |
| 16 MB Rust, highlighted | 45 MB | 937 MB |
| 16 MB, plain text | 45 MB | 307 MB |
| 1M lines, 33 MB | 70 MB | 333 MB |
| 1M lines, 100 MB | 160 MB | 424 MB |

About 255 MB appears after the first frames whatever the file, and is mostly
GPU memory (drawables, glyph atlas, compiled shaders). The 630 MB difference
between the two 16 MB rows is the tree-sitter tree, which lands after the
background parse.

## Syntax (`cargo bench -p fe-syntax`)

Corpora are generated: Rust items, pretty-printed JSON objects, and Markdown
sections with lists, quotes and fenced code. A keystroke is one letter typed
inside a word, deleted again untimed before the next. The highlight is 60
rows starting at a random row.

| | Rust | JSON | Markdown |
|---|---|---|---|
| Full parse, 10k lines | 21.8 ms | 11.7 ms | 29.4 ms |
| Reparse after a keystroke, 10k lines | 0.87 ms | 0.25 ms | **6.1 ms** |
| Highlight 60 rows, 10k lines | 0.12 ms | 0.06 ms | 0.05 ms |
| Full parse, 16 MB | 1.21 s | 0.96 s | 2.67 s |
| Reparse after a keystroke, 16 MB | 48–182 ms | 32 ms | **640 ms** |
| Highlight 60 rows, 16 MB | 0.12 ms | 0.10 ms | 0.05 ms |
| Full parse, 1M lines (31 / 22 / 19 MB) | 2.26 s | 1.23 s | 3.01 s |

The plan's syntax target is a reparse under 5 ms on 10k lines. Rust and JSON
meet it with room to spare; Markdown doesn't.

Reparse time grows with file size, not with the size of the edit. Plain
tree-sitter shows the same cost (25–30 ms per keystroke on the 16 MB Rust
file), so it isn't our wrapper. Above roughly 0.5 MB of Rust, a reparse never
fits the view's 1 ms synchronous budget: each keystroke spends that 1 ms and
then reparses in the background, and colours lag the text by the reparse
time.

## Not measured yet

- Typing latency (keystroke to paint; target p50 < 4 ms, p99 < 8 ms).
- Cold start to first paint (target < 150 ms). Warm launches paint their
  first frame at 180–200 ms.
