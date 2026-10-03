# Performance

How fastedit is measured, and the latest numbers against the plan's targets.
Release builds on an M3 Max MacBook Pro (120 Hz built-in display), measured
2026-10-02 at `m2-bench`.

## Measuring

```zsh
cargo bench -p fe-syntax                          # parse, reparse, highlight (~8 min)
cargo bench -p fe-syntax -- 10k                   # just the 10k-line cases
cargo bench -p fe-syntax --bench flat             # highlighting huge flat files (~2 min)
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
Rust). The limit has since dropped to 8 MB (see [tree memory](#tree-memory)),
so files like the 16 MB rows below are no longer highlighted.

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
meet it with room to spare; Markdown doesn't, and stays a documented limit.

**Why Markdown is slow (T4).** It isn't our wrapper: plain tree-sitter driving
`tree_sitter_md::LANGUAGE` over the same 187 KB corpus and the same random
in-word keystrokes averages 7.5 ms per reparse against the bench's 6.1 ms. The
cost tracks the distance to the end of the file, not the edit: 6.5 ms at 1% of
the file, 5.8 ms at 25%, 4.2 ms at 50%, 2.9 ms at 90%, with no changed ranges
(the tree comes out the same shape). A profile of 20,000 reparses puts under
0.4% in the block scanner and the lexer combined (calls included). The time is in tree-sitter's
parser itself: about a quarter in `ts_parser_parse` (reuse checks), a fifth in
`ts_subtree_summarize_children` and 3% in `ts_subtree_compress` (rebuilding the
`document` node's 666 children and the ancestors of the edit), and the rest in
stack pushes, `reusable_node_advance` and freeing subtrees. Rust and JSON trees
reuse their unchanged tail whole; the Markdown block grammar's tail is walked
and reduced node by node. The fix would be in the grammar's structure (an
upstream tree-sitter-md change or a fork, which is a dependency change and
needs a licence audit), so it is left as a limit: follow-up card T33. At the
16 MB parse limit the same cost is the 640 ms above.

Reparse time grows with file size, not with the size of the edit. Plain
tree-sitter shows the same cost (25–30 ms per keystroke on the 16 MB Rust
file), so it isn't our wrapper. Above roughly 0.5 MB of Rust, a reparse never
fits the view's 1 ms synchronous budget: each keystroke spends that 1 ms and
then reparses in the background, and colours lag the text by the reparse
time.

## Flat files (`cargo bench -p fe-syntax --bench flat`)

Files where one node has about a million children: a JSON array of numbers,
the same with single digits (the most elements that fit), and Markdown
paragraphs. Each is filled to just under `MAX_PARSE_BYTES`. The cost is one
60-row highlight, scrolled a row at a time, measured 2026-10-03.

| File | Start | Middle | End | Unscrolled |
|---|---|---|---|---|
| JSON, 850k numbers (8 MB) | 24 µs | 20 µs | 15 µs | 62 ns |
| JSON, 2.8M zeros (8 MB) | 47 µs | 31 µs | 16 µs | 62 ns |
| Markdown, 450k paragraphs (8 MB) | 22 µs | 18 µs | 13 µs | 33 ns |

Before this, highlighting cost depended on position. On a 1M-element JSON
array (9.4 MB), a screen cost 0.27 ms at the top, 0.15 ms in the middle and
0.03 ms at the end, and 0.45 ms at the top of a 1.5M-element array. Markdown
with 500k paragraphs cost 0.18 ms at the top. Tree-sitter stores a node's
repeated children as a partly balanced tree of hidden nodes, and early
children sit about a thousand levels deep. A query cursor walks up through
those levels at every child it visits. `Syntax::highlights` now queries a node
with more than 1024 children only for patterns rooted at the node itself,
then queries each child in view separately.

A frame that doesn't scroll or edit reuses the last highlights, so it doesn't
query at all: that's the Unscrolled column. Logs and CSV have no grammar and
are never highlighted.

### Tree memory

Each bench prints its trees' memory, measured as the growth in malloc'd bytes
across a parse:

| Source | Tree / source |
|---|---|
| Rust, generated items (4 and 15 MB) | 40× |
| Rust, this repo's sources concatenated (4 MB) | 34× |
| JSON, pretty-printed objects (4 and 15 MB) | 30× |
| JSON, an array of numbers (8 MB) | 29× |
| JSON, an array of single digits (8 MB) | 96× |
| Markdown, mixed blocks (4 and 15 MB) | 71× |
| Markdown, short paragraphs (8 MB) | 85× |

Memory grows linearly with size, and with the number of nodes more than the
number of bytes. `MAX_PARSE_BYTES` is now 8 MB, down from 16 MB. That keeps a
code file's tree near 300 MB, and the worst case above under 800 MB; at
16 MB, trees reached 0.6–1.5 GB.

## Not measured yet

- Typing latency (keystroke to paint; target p50 < 4 ms, p99 < 8 ms).
- Cold start to first paint (target < 150 ms). Warm launches paint their
  first frame at 180–200 ms.
