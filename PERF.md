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

- **open:** the [startup phases](#startup), one line each, timed from the
  kernel's process start: how long after launch each milestone came, and how
  long since the one before. Then the first presented frame, which is the
  file if its read finished before the window was built and otherwise the
  "Loading <file>…" placeholder (the line says which), and *file shown*, the
  first presented frame painted with the editor. The scroll phases start
  after the file is shown. Before T8, "launch" meant the top of `main`,
  about 6 ms later;
- **smooth scroll:** 600 frames scrolling 40 px (about two rows) each;
- **jump scroll:** 600 frames scrolling about 1000 rows each, so every frame
  shows rows that haven't been shaped yet;
- **prepaint/paint:** the editor element's own time per frame, from
  `EditorView::frame_timings()`; *interval* is the time between painted
  frames, so 8.3 ms is 120 fps and any interval over 12.5 ms missed a vsync;
- **memory:** the process's physical footprint (what Activity Monitor shows),
  which on Apple Silicon includes GPU memory, and its resident set; each with
  its peak over the process's life so far, which covers the background read.
  Printed at file shown, after settling (60 frames later, once a background
  parse has had time to land) and after scrolling;
- **idle:** CPU used over 5 s with nothing happening, starting 2 s after the
  last scroll (GPUI keeps presenting for a second after fast input).

The first launch after a build is slower: macOS spends 240–380 ms before
`main` on a newly written executable (see [startup](#startup)), and one such
run stalled for 3.6 s while scrolling. Repeat runs don't. Numbers below are
from repeat runs.

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
| 1M-line file interactive in < 500 ms | Since T8 the read starts at the top of `main`, and a 33 MB file is on the first frame: presented at 200 ms median from process start. Before, the file was shown at 297–339 ms for 33 MB and 271–310 ms for 100 MB. See [startup](#startup) and [background loading](#background-loading) | ✅ |
| Memory ≤ 1.5× file size | Unhighlighted: 100 MB file costs 1.46–1.47×, 33 MB 1.45–1.48×, over an empty editor at file shown (footprint and resident). The read peaks no higher. **Highlighted: a 16 MB Rust file costs 630 MB more than the same file unhighlighted (~39×)** | ❌ for highlighted files |
| Scrolling holds 120 fps | Smooth scrolling: 120 fps on every file, 0–1 missed vsyncs in 600 frames. Jump scrolling: 119–120 fps, but a highlighted 16 MB file spends 6.5 ms p50 in prepaint and misses 14 vsyncs | ✅, little headroom when highlighted |
| Idle CPU near zero | 0.8–0.95% of a core, no frames painted | ✅ |
| UI never blocks on a parse | A frame tries at most 1 ms of reparse, then moves it to a background thread | ✅ by design; see reparse below |

Every 1M-line file tried is over `MAX_PARSE_BYTES` (16 MB), so the 1M-line
runs are unhighlighted. The largest highlighted file is 16 MB (486k lines of
Rust). The limit has since dropped to 8 MB (see [tree memory](#tree-memory)),
so files like the 16 MB rows below are no longer highlighted.

### Background loading

`fastedit <file>` opens the window at once and reads the file on a
background thread (`crates/fastedit/src/loader.rs`); the first frame shows
"Loading <file>…" and the editor replaces it when the read finishes.
Measured 2026-10-03 on the same machine, release build, three repeat runs
each, with other jobs running (load average 7–14), so the times are noisier
than the m2-bench numbers above. The files are 1M lines of ASCII.

| File | First frame (placeholder) | File shown | Footprint at file shown | Resident at file shown (peak) |
|---|---|---|---|---|
| empty | 201–227 ms | 269–294 ms | 260 MB | 99 MB (99 MB) |
| 1M lines, 33 MB | 217–258 ms | 297–339 ms | 307–308 MB: +47–48 MB, 1.45–1.48× | 146–147 MB (147 MB): +47–48 MB, 1.45–1.48× |
| 1M lines, 100 MB | 208–251 ms | 271–310 ms | 399 MB: +139 MB, 1.46× | 238–239 MB (239 MB): +139–140 MB, 1.46–1.47× |

"First frame" is when the first frame is presented, not drawn: `open_window`
draws a frame at once but presents it at the window's first frame request,
and if the read had finished by then, the editor would be drawn and presented
instead and `--perf` would say so. It didn't happen in these runs, even for
the empty file.

Costs are over the empty file. Memory is now taken at file shown rather than
at the first frame, so it includes the ~240 MB of GPU memory that appears
with the first frames. The lifetime peaks equal the values at file shown: the
streaming read plus the rope never holds more than the finished rope.

The first frame no longer depends on the file's size: before, 100 MB painted
its first frame 43 ms later than an empty file, and a 1 GB file would have
blocked for about 0.45 s. The read itself takes 20–80 ms for 100 MB.

**Superseded by T8** (see [startup](#startup)): the read now starts at the
top of `main`, and a file whose read has finished when the window builds
its view (33 MB has, 100 MB hasn't always) is on the first frame. The rest
of this section describes the T5 behaviour.

The cost is that a file is never on the first frame, even an empty one: it
shows 60–90 ms after the placeholder. The read finishes within a millisecond
for a small file, but the main thread is busy for another 30–80 ms after the
window opens before the read's result is picked up, and the editor's first
frame waits for the next vsync after that. Small files, the common case, are
on screen about 90 ms later than when they were read before the window
opened.

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

About 255 MB appears after the first frames whatever the file. It's GPU
memory, three quarters of it driver memory that becomes purgeable when the
window goes idle: see [fixed memory](#fixed-memory). The 630 MB difference
between the two 16 MB rows is the tree-sitter tree, which lands after the
background parse.

## Startup

`--perf`'s `open:` lines, measured 2026-10-03 (T8) on the same machine,
release builds. Each milestone is timed from the kernel's process start
(`proc_pidinfo`), so exec and dyld count. A sample, the 33 MB file:

```text
open:  7.4 ms to main, +7.4 ms
open: 25.2 ms file read (background thread), +17.9 ms
open: 50.3 ms platform created, +25.1 ms
open: 60.0 ms app callback (AppKit launched), +9.7 ms
open: 142.1 ms window and renderer created, +82.0 ms
open: 142.1 ms file loaded (main thread), +0.0 ms
open: 145.8 ms editor's first prepaint starts, +3.7 ms
open: 151.2 ms editor's first paint done (prepaint 2.5ms, paint 2.9ms), +5.4 ms
open: 151.5 ms window open (first frame drawn), +0.4 ms
open: 174.7 ms first frame requested, +23.2 ms
open: 244.4 ms first frame presented (the file), +69.7 ms
open: 244.4 ms file shown (presented), +0.0 ms
```

Lines are in time order, and each *+* is the time since the line before.
The read runs on its own thread alongside the others, so its *+* and the
next line's overlap.

The milestones: *main*; *platform created* is `gpui_platform::application()`
returning (GPUI's Mac platform exists, AppKit hasn't launched); *app callback*
is `Application::run` calling back once AppKit has finished launching;
*window and renderer created* is `open_window` calling back to build the root
view (the `NSWindow`, its Metal layer and renderer exist, nothing is drawn);
the editor's first prepaint and paint come from its `FrameTiming`; *window
open* is `open_window` returning with the first frame drawn; *first frame
requested* is the start of the window's first frame request, which draws
again if anything changed and then presents; *first frame presented* is that
request returning, timed by a task spawned at its start (GPUI has no hook
after presenting, so this is an upper bound by however long the main thread
takes to run the task). *File shown* is the first presentation after the
editor's first paint. *File read* is the reader thread finishing and *file
loaded* the main thread taking its result. The lines are printed after the
file is shown, so printing them doesn't delay it.

### Warm and cold launches

Five warm launches per file (run interleaved, after one launch of each build
to warm it up), median and range in ms from process start; one cold launch
each, from a fresh copy of the executable. The machine was busy (load average
10–12: two other build lanes and a VM), so the spread is wide. "Before" is
the base commit with only the `--perf` instrumentation added.

| Milestone | Empty, warm | 33 MB, warm | Empty, cold | 33 MB, cold |
|---|---|---|---|---|
| main | 6.4 (5.6–8.7) | 7.2 (5.9–9.2) | 149 | 145 |
| file read | 6.5 (5.7–8.9) | 26.6 (21.6–37.2) | 150 | 164 |
| platform created | 38.9 (32.6–52.6) | 43.8 (38.0–65.7) | 188 | 185 |
| app callback | 46.7 (39.5–62.3) | 52.3 (45.2–77.0) | 201 | 194 |
| window and renderer created, file loaded | 116.0 (95.3–129.7) | 115.6 (99.7–148.6) | 285 | 274 |
| editor's first paint done | 120.8 (100.6–134.5) | 124.1 (107.7–157.6) | 292 | 286 |
| window open | 121.0 (100.9–134.7) | 124.4 (108.0–157.9) | 293 | 286 |
| first frame requested | 136.6 (117.6–159.9) | 139.3 (125.4–174.4) | 311 | 306 |
| **first frame presented, file shown** | **182.3 (151.4–219.0)** | **200.3 (181.9–282.5)** | **328** | **321** |
| *Before:* app callback | 54.0 (52.1–67.7) | 45.2 (43.6–52.0) | 208 | 175 |
| *Before:* window open | 173.9 (170.0–194.2) | 163.4 (161.2–173.2) | 353 | 308 |
| *Before:* first frame requested | 196.7 (191.7–212.4) | 182.2 (178.8–191.2) | 372 | 324 |
| *Before:* first frame presented (placeholder) | 236.3 (212.6–266.6) | 251.1 (201.5–265.5) | 391 | 343 |
| *Before:* file loaded on the main thread | 229.6 (207.2–261.2) | 245.7 (196.0–259.2) | 385 | 337 |
| *Before:* file shown | 270.1 (266.5–295.4) | 260.5 (259.6–283.9) | 444 | 393 |

Ten more warm launches of the current build: first frame presented at a
median of 203 ms (empty, 146–252) and 194 ms (33 MB, 148–245).

Three changes got the file on screen 60–90 ms sooner:

- **`EditorTheme::default_dark` listed every installed font** to pick a
  family: 60–75 ms on the main thread between the app callback and the
  window. It now resolves each preferred family and checks that it didn't
  fall back (`crates/fe-gpui/src/lib.rs`), which takes about 5 ms and loads
  the family the first frame needs anyway.
- **The read starts at the top of `main`** on its own thread, instead of
  after `open_window`. If it has finished when the window builds its view
  (about 115 ms in, enough for 33 MB), the editor is drawn in the window's
  first frame and the placeholder never shows. Before, the read's result
  reached the main thread with or after the first presentation, even for an
  empty file, and the editor took one more frame. A file still being read
  shows the placeholder as before.
- **The editor is focused while the root view is built**, before the
  window's first draw. Focusing it after `open_window` invalidated that draw.
  The first frame request still draws once more (something else in GPUI's
  window setup invalidates it), which costs a few ms.

Where the warm ~190 ms goes now, by phase (medians, both files):

| Phase | Time | Whose |
|---|---|---|
| exec and dyld, to `main` | 6–7 ms | the system |
| `gpui_platform::application()` | 32–37 ms | GPUI (`MacPlatform::new`: keyboard layout, text system, power observers) |
| AppKit launch, to the app callback | 8 ms | AppKit |
| keymap and theme | 5 ms | fastedit |
| `NSWindow`, Metal layer and renderer | 60–65 ms | GPUI and AppKit; in a Time Profiler trace about 25 ms is `NSWindow` init and its title bar, 10–14 ms is GPUI's `tab_bar_visible` creating the window's tab group, 7.5 ms is the Metal device (timed alone), the rest is blocked on the window server and Metal's pipeline setup |
| first draw | 5–10 ms | fastedit's layout and paint (2.5 + 2.9 ms for 33 MB), GPUI's |
| until the first frame request | 15 ms | AppKit: the window server asks the new layer to display |
| the first frame request: draw again, present | 45–60 ms | GPUI and Metal. A trace puts it almost all in `MetalRenderer::draw`, blocked: the process's first command buffer, its first drawable, and presenting with the Core Animation transaction. A process's first Metal command buffer takes 20–31 ms to schedule and adds 217 MB of footprint on its own (a standalone program clearing a 2400×1600 target; the second takes 0.2 ms) |

**First paint isn't under 150 ms: warm medians are 182–203 ms** (best runs
146–152 ms) on this loaded machine. What `crates/fastedit` controls before
the first frame is now about 10–15 ms (the theme and the first draw). The
blocker is GPUI, AppKit and Metal setup: about 100–110 ms between `main` and
a window ready to draw, 15 ms until the window server asks for the first
frame, and 45–60 ms to present it, of which 20–31 ms is Metal's first
command buffer for the process. Known savings are all outside fastedit: in
GPUI, skipping `tab_bar_visible` on window creation (10–14 ms) and whatever
invalidates the first draw (a few ms). Warming Metal (a tiny command buffer
on a background thread at the top of `main`) might take the first command
buffer off the critical path; it would need `metal` as a direct dependency,
which this task doesn't add.

**Cold launches** spend 140–280 ms before `main`: the first exec of a newly
written executable, which macOS checks before running it (a fresh copy of an
unchanged binary pays it too). After that the phases cost about what they
cost warm. Shader compilation isn't part of it while Metal's compiler cache
has GPUI's shaders, see below.

### `runtime_shaders`

The Metal toolchain isn't installed here (`xcrun -sdk macosx metal -v`
fails), so there's no build without `runtime_shaders` to compare. Timed
alone instead: a standalone program that compiles GPUI's stitched shader
source (66 KB, from gpui-pre-apple's build output) with
`new_library_with_source`, as `MetalRenderer::new` does, took **357 ms the
first time and 0.8–1.0 ms on every run after**, because Metal caches
compiled libraries by source. Creating the Metal device took 7.5 ms. So
precompiled shaders would save about 1 ms on a warm launch, and about 350 ms
on the first launch after the shader source changes (a GPUI bump) or the
cache is cleared. That's not the warm blocker; dropping the feature remains
a CI question (the spec's open question), not a startup fix.

## Fixed memory

What the ~255 MB that appears with the first frames is, for the empty file.
Measured with `footprint -v -p <pid>` (per-region physical footprint; its
categories match `vmmap --summary`), 1200×800 window on a 2× display, with
the base build (the renderer and GPUI are unchanged by T8). Instruments'
Game Memory template didn't finish a recording on this machine.

| | At file shown (≈10 frames) | After scrolling | Plain launch, idle 4 s |
|---|---|---|---|
| Footprint | 259 MB | 278 MB | 61 MB |
| Drawables: `CAMetalLayer` IOSurfaces, 2400×1600 BGRA, 15.6 MB each | 30 MB (2) | 45 MB (3) | 30 MB (2) |
| Metal's per-process memory: 8 MB "owned unmapped graphics" regions | 192 MB (24) | 192 MB (24) | 0 (13 regions, 104 MB, all reclaimable) |
| GPUI's Metal buffers and textures ("IOAccelerator (graphics)") | 6.6 MB | 8.6 MB | 4.3 MB |
| … glyph atlas, 1024×1024 A8, 1 MB each | 1–2 MB | 2 MB | 1 MB |
| … instance buffers, 2 MB each | 2 MB | 4 MB | 2 MB |
| Path intermediate texture, 2400×1600 BGRA | 15 MB reserved, none touched | same | same |
| Shader library | about 0.75 MB (timed alone: footprint 4.2 → 4.9 MB across `new_library_with_source`) | | |
| Malloc | 15 MB | 16 MB | 15 MB |

- **Three quarters is Metal's per-process memory, not GPUI's resources.**
  Nothing GPUI allocates is 8 MB, and its own buffers and textures come to
  under 25 MB. The 8 MB regions arrive with the first present (the footprint
  is 18 MB just before it). A standalone program shows the same thing: its
  first Metal command buffer, a clear of a 2400×1600 target, takes the
  footprint from 4 MB to 221 MB, and later ones add nothing. The regions
  stay dirty while frames are drawn every vsync, as in `--perf`. In a window left idle a few seconds they're all reclaimable
  (purgeable), and the footprint drops to 61 MB. So `--perf`'s *after
  scrolling* figure is the busy-rendering cost; an idle editor's is about a
  quarter of it.
- **Drawables** are 15.6 MB each at 1200×800 @2×. GPUI allows three
  (`maximumDrawableCount`); the third appears only under fast scrolling.
- **Precompiled shaders** would save under 1 MB. **A smaller atlas** would
  save under 1 MB per texture: GPUI's minimum is 1024×1024, and an editor
  showing ASCII in one font fills one A8 texture.
- The path intermediate texture is created at the window's size but never
  rendered into (the editor draws no paths), so it costs nothing.

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

**YAML over 32,768 lines** (T15, 2026-10-03). tree-sitter-yaml 0.7.2's
external scanner keeps the current row in an `int16_t`. At line 32,769 it wraps
to −32,768, so `has_nwl = cur_row > row` misses that newline, and the line gets
error nodes. The same happens again every 65,536 lines after that. Any YAML
was affected, not just lists: 32,768 lines of `- item N`, `- a` or `kN: N`
parsed clean, and 32,769 didn't. A 100k-line list got an ERROR root, as did
the largest list under `MAX_PARSE_BYTES` (607k items, 8 MB), so highlighting
stopped past the first screen. The count is lines, not bytes or items: 20k
items separated by blank lines (40k lines) failed too. Upstream:
[tree-sitter-yaml#49](https://github.com/tree-sitter-grammars/tree-sitter-yaml/issues/49),
fixed by the unmerged
[PR #35](https://github.com/tree-sitter-grammars/tree-sitter-yaml/pull/35),
which widens the row fields to `int32_t`. No release after 0.7.2 has the fix.
The row is private scanner state, so nothing in `fe-syntax` could work around
it. We now build 0.7.2 with #35 applied, from `vendor/tree-sitter-yaml/`
through `[patch.crates-io]`; its README says when to drop it. With the patch,
all the cases above parse clean, as does a mapping nested 200 deep, and
`crates/fe-syntax/tests/yaml_long_list.rs` checks a 100k-item list.

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
