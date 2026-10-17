# Performance

How fastedit is measured, and the latest numbers against the plan's targets.
Release builds on an M3 Max MacBook Pro (120 Hz built-in display), measured
2026-10-02 at `m2-bench`.

## Measuring

```zsh
cargo bench -p fe-syntax                          # parse, reparse, highlight (~8 min)
cargo bench -p fe-syntax -- 10k                   # just the 10k-line cases
cargo bench -p fe-syntax --bench flat             # highlighting huge flat files (~2 min)
cargo bench -p fe-buffer                          # load and edit (M1) (~2.5 min)
cargo bench -p fe-display                         # keystroke re-wrap, wrap slices (~2 min)
cargo bench -p fe-project --bench tree            # project tree open, expand, apply (~1 min)
cargo bench -p fe-project --bench finder          # Cmd-P walk and queries over 100k paths (~1 min)
cargo bench -p fe-project --bench search          # project search over 10k files, 40 MB (~1 min)
cargo bench -p fe-gpui --bench keystroke          # keystroke to presented frame, by stage (~20 s)
cargo build --release -p fastedit
target/release/fastedit --perf path/to/file       # open, scroll, memory, idle; then quits
```

Close CPU-heavy jobs first (soak runs, builds): with the machine loaded,
numbers come out several times worse and noisy.

To check a change against a base for regressions, run the **Bench** workflow
(`.github/workflows/bench.yml`, manual only: Actions → Bench → Run workflow,
or `gh workflow run bench.yml -f base=<ref>`). It builds the fe-buffer,
fe-display, fe-syntax (`syntax`, 10k cases) and fe-gpui `keystroke` bench
binaries for the base ref (default `origin/main~1`) and HEAD, runs them
alternately for three rounds, uploads every `sample.json`, and fails if a
case's fastest sample is more than 15% slower than base. The comparison is
`scripts/bench-compare.sh <base-dir> <head-dir> [threshold-percent]`, which
also works locally on criterion output dirs (`CRITERION_HOME`, one per round);
its fixture test is `scripts/test-bench-compare.sh`. Means swing too much on
a shared runner; the fastest sample doesn't.

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
| Memory ≤ 1.5× file size (the text only), plus a syntax tree of ≤ 300 MB | Text: a 100 MB file costs 1.46–1.47×, 33 MB 1.45–1.48×, over an empty editor at file shown (footprint and resident). The read peaks no higher. Tree: each language's parse limit is set so its typical tree stays under 300 MB (see [tree memory](#tree-memory)). Measured before the limits, at the old 16 MB limit: a highlighted 16 MB Rust file cost 630 MB more than the same file unhighlighted (~39×) | ✅ text; tree capped per language, not re-measured |
| Scrolling holds 120 fps | Smooth scrolling: 120 fps on every file, 0–1 missed vsyncs in 600 frames. Jump scrolling: 119–120 fps, but a highlighted 16 MB file spends 6.5 ms p50 in prepaint and misses 14 vsyncs | ✅, little headroom when highlighted |
| Idle CPU near zero | 0.8–0.95% of a core, no frames painted | ✅ |
| UI never blocks on a parse | A frame tries at most 1 ms of reparse, then moves it to a background thread | ✅ by design; see reparse below |

Every 1M-line file tried is over every language's parse limit (a global
16 MB when measured), so the 1M-line runs are unhighlighted. The largest
highlighted file was 16 MB (486k lines of Rust). Each language now has its own
limit, 3.5 to 10 MiB (see [tree memory](#tree-memory)), so files like the
16 MB rows below are no longer highlighted.

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

### Scrolling, headless (`cargo bench -p fe-gpui --bench scroll`)

600 scroll-wheel frames a case through a focused `EditorView` in GPUI's
headless test window with the real macOS text system and the headless Metal
renderer, so it needs no screen. Smooth is 40 px a frame, jump about 1000 rows
(21,000 px) and wraps to the top at the end. The frame runs from dispatching
the event to the scene being committed with `render_to_image`; it leaves out
vsync, the compositor and the GPU's own time, as the keystroke bench's numbers
do. Budget: frame p99 at most 8.33 ms (120 fps).

Apple M3 Max, bench at commit e0b94b24 plus the review fixes, 2026-10-17,
release build. **Not a quiet machine:** the load average was 50-66 on 14 cores
(other lanes' gates), so the p99 and max columns carry scheduler noise; p50
barely moves between runs. Earlier runs the same day varied: frame p99 between
2.5 and 140 ms for smooth, and 4 and 285 ms for the jump cases, with the
quietest at 4-5 ms for `jump/1m-plain` and `jump/7mb-rust`. The table is one
run, not the best.

| Case | Frame p50 / p99 / max | Prepaint p50 / p99 / max | Paint p50 / p99 / max | Over 8.33 ms | p99 <= 8.33 ms |
|---|---|---|---|---|---|
| `smooth/1m-plain` | 0.62 / 2.9 / 6.5 ms | 0.26 / 1.2 / 2.7 ms | 0.20 / 0.55 / 2.4 ms | 0 of 600 | holds |
| `jump/1m-plain` | 3.8 / 97.9 / 168 ms | 3.3 / 92.2 / 167 ms | 0.22 / 3.4 / 17.5 ms | 56 of 600 | misses |
| `jump/7mb-rust` | 3.4 / 108 / 164 ms | 2.9 / 105 / 163 ms | 0.16 / 4.1 / 153 ms | 90 of 600 | misses |
| `jump/1m-plain-wrapped` | 3.4 / 21.6 / 35.0 ms | 3.0 / 17.7 / 27.8 ms | 0.19 / 0.56 / 4.9 ms | 19 of 600 | misses |

The three misses are in prepaint: its p99 is nearly the frame's, while paint
p99 is a few ms at most and present (frame minus prepaint and paint, about
0.2 ms at p50) never carries a tail. Jump prepaint's p50 alone is 2.9-3.3 ms,
and the tail at this load is several times that, so the jump cases can't be
confirmed at 120 fps from this run. Follow-ups: T974 re-runs the bench on a
quiet machine to separate load from the code (T408's screen run is the other
check), and T975 cuts jump-frame prepaint (shaping a thousand-row jump's new
rows, in the plain, highlighted and wrapped cases).

### Memory (physical footprint)

The 16 MB rows were measured at the old 16 MB global parse limit, and not
re-measured since.

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

## Buffer (`cargo bench -p fe-buffer`)

M1's budgets, from the header of `crates/fe-buffer/benches/buffer.rs`: a
100 MB / 1M-line file loads well inside the 500 ms "interactive" budget; a
keystroke edit in it is far below a frame (8 ms p99 end to end); snapshots
are O(1); anchor resolution scales with the edits since the anchor. The
budgets are stated for Apple Silicon. This run is on helium, not the Mac, and
no Mac run of this bench is recorded here yet, so the helium numbers are
compared with the budgets as stated.

- **Machine:** helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24
  threads), release build in a private `CARGO_TARGET_DIR`.
- **Commit:** `2b8248f`. **Date:** 2026-10-12.
- **Load average** (`/proc/loadavg`, 1 minute): 45.93 at the start of the
  run, 27.64 at the end. Other lanes were building, so these numbers are
  pessimistic and noisy.

"median" is criterion's median estimate; "fastest" is the smallest of
times/iters over all the samples in each case's `new/sample.json`. The sample
count is not the same for every case: the two buffer load cases set
`sample_size(10)`.

| Case | helium median | helium fastest | Budget | Verdict |
|---|---|---|---|---|
| `load/100MB` | 49.1 ms | 40.5 ms | well inside 500 ms | meets (10% of it) |
| `load/100MB crlf` | 204 ms | 198 ms | well inside 500 ms | meets, but 41% of it: "well inside" is thinner |
| `edit/keystroke in 100MB` | 1.76 µs | 1.42 µs | far below 8 ms | meets (0.02%) |
| `edit/100 cursors in 100MB` | 122 µs | 106 µs | far below 8 ms | meets (1.5%) |
| `snapshot/clone` | 13.8 ns | 13.5 ns | O(1) | meets: 14 ns, and the case has no size input to scale |
| `convert/offset to LSP point in 100MB` | 699 ns | 528 ns | none stated | n/a |
| `convert/LSP point to offset in 100MB` | 1.60 µs | 1.24 µs | none stated | n/a |
| `anchors/resolve 10k through 100 edits, one by one` | 14.3 ms | 9.89 ms | scales with edits since the anchor | see below |
| `anchors/resolve 10k through 100 edits, batched` | 1.51 ms | 1.42 ms | scales with edits since the anchor | see below |
| `undo/keystroke after 100 later edits` | 16.0 µs | 12.6 µs | far below 8 ms | meets (0.2%) |

- **Nothing misses its budget**, so no follow-up is filed. The 8 ms budget is
  p99 end to end; these are the buffer's share only, in medians, and the
  rest of the keystroke path is in the Typing latency section below.
- **Anchors:** 10k anchors through 100 edits cost about 1.4 µs each one by
  one and 0.15 µs each batched, 9 to 10 times cheaper. The bench uses a
  single edit count, so it shows the cost is small but not that it scales
  with the edits since the anchor; that needs a second edit count, which
  isn't filed.
- **CRLF load** is four times the LF load. It is inside the budget, and the
  budget for it is the same 500 ms, but on a loaded or slower machine it is
  the case to watch.

## Display map (`cargo bench -p fe-display`)

Budgets, from the header of `crates/fe-display/benches/display.rs`, on a
1M-line file (Apple Silicon): a keystroke re-wraps only its row, far inside
the 8 ms p99 keystroke-to-paint budget; a new wrap width wraps one slice of
`WRAP_SLICE_BYTES` around the viewport per frame, which has to stay well
inside a frame (8.3 ms at 120 Hz). As above, the budgets are stated for
Apple Silicon and the helium numbers are compared with them as stated.

- **Machine:** helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24
  threads), release build in a private `CARGO_TARGET_DIR`.
- **Commit:** `2b8248f`. **Date:** 2026-10-12.
- **Load average** (`/proc/loadavg`, 1 minute): 25.10 at the start of the
  run, 21.20 at the end, from other lanes building.

| Case | helium median | helium fastest | Budget | Verdict |
|---|---|---|---|---|
| `wrap/set width + one slice, 1M lines` | 571 µs | 479 µs | well inside 8.3 ms (T10: under 2 ms) | meets (7% of a frame) |
| `wrap/one slice, 1M lines` | 1.06 ms | 553 µs | well inside 8.3 ms | meets (13% of a frame) |
| `wrap/all pending rows in slices, 1M lines at 80 columns` | 827 ms | 620 ms | none: the whole file, spread over frames | n/a |
| `sync/keystroke, 1M lines unwrapped` | 3.73 µs | 3.47 µs | far inside 8 ms | meets (0.05%) |
| `sync/keystroke, 1M lines wrapped` | 13.4 µs | 12.8 µs | far inside 8 ms | meets (0.2%) |

- **Nothing misses its budget**, so no follow-up is filed.
- `one slice` is slower than `set width + one slice` in the median, and its
  fastest sample (553 µs) is half its median, so it is noisy under this
  load. Both are far inside a frame.
- `all pending rows` is the whole 1M-line file wrapped in slices, about 0.8 s
  in total, spread over frames; it has no budget and is here for the record.

### Many folds (T159)

A keystroke also brings every fold's hidden rows up to date. T159 resolves
them in one forward cursor pass instead of two seeks from the root per fold.
The cases fold 4 rows every 100 rows (10k folds) or every 5 (200k, as Fold
All gives on such a file) and type on headers or between folds. The bench
lifts the undo history limit: trimming a full history costs more per edit
than the display work (see the decision file).

- **Machine:** helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24
  threads), release builds in private `CARGO_TARGET_DIR`s.
- **Commit:** base `4a3c74c` with the new bench file over its sources;
  candidate `1b12d1b`. **Date:** 2026-10-17.
- **Load average** (`/proc/loadavg`, 1 minute): 2.64 at the start, 11.20 at
  the end, from other lanes.
- Base and candidate run alternately for 3 rounds (`--warm-up-time 1
  --measurement-time 3 'sync/|fold/'`). "Fastest" is over all 3 rounds;
  "median" is the lowest of the 3 criterion medians. The 200k cases set
  `sample_size(10)`.

| Case | Base median | Base fastest | Candidate median | Candidate fastest | Budget | Verdict |
|---|---|---|---|---|---|---|
| `sync/keystroke, 1M lines unwrapped` | 3.83 µs | 3.57 µs | 3.52 µs | 3.40 µs | not over 10% slower than base | meets |
| `sync/keystroke, 1M lines wrapped` | 14.3 µs | 13.9 µs | 15.3 µs | 14.2 µs | not over 10% slower than base | meets (2%) |
| `sync/keystroke, 1M lines unwrapped, 10k folds` | 73.4 µs | 72.8 µs | 85.9 µs | 73.4 µs | not over 10% slower than base | meets (1%) |
| `sync/keystroke, 1M lines wrapped, 10k folds` | 1.51 ms | 1.42 ms | 897 µs | 690 µs | at least 1.8x faster than base | meets (2.05x) |
| `sync/keystroke, 1M lines wrapped, 200k folds` | 21.2 ms | 20.8 ms | 11.1 ms | 11.0 ms | at least 1.8x faster than base | meets (1.90x) |
| `fold/one more fold among 200k, wrapped` | 21.1 ms | 20.9 ms | 13.2 ms | 12.7 ms | not over 10% slower than base | meets (1.6x faster) |

- The pass is memory-bound: the same rebuild timed alone is about 3x (10k)
  and 6x (200k) faster than base, and the 200k case on a freshly built tree
  ran at 5.1 ms. In the bench its tree comes after the earlier cases'
  keystrokes, scattered in memory, and other lanes share the cache.
- The budgets are the card's revised ones (rev 2, 2026-10-17). The original
  budgets are not met: `wrapped, 10k folds` at most 0.5 ms (690 µs, 190 µs
  over) and `wrapped, 200k folds` at least 4x faster than base (1.90x;
  11.0 ms against 5.2 ms, and over the 8 ms keystroke budget).
- **Follow-up**, with those budgets: keep folds' wrap rows across edits,
  shifted by each edit's wrap-row delta. Details in
  `docs/decisions/ez74dvmwrv-fold-rows-one-cursor-pass.md`.

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
16 MB parse limit the same cost is the 640 ms above. The decision (accept it for
now, upstream issue drafted, fork assessed) is in
[docs/decisions/markdown-reparse.md](docs/decisions/markdown-reparse.md).

Reparse time grows with file size, not with the size of the edit. Plain
tree-sitter shows the same cost (25–30 ms per keystroke on the 16 MB Rust
file), so it isn't our wrapper. Above roughly 0.5 MB of Rust, a reparse never
fits the view's 1 ms synchronous budget: each keystroke spends that 1 ms and
then reparses in the background, and colours lag the text by the reparse
time.

## Flat files (`cargo bench -p fe-syntax --bench flat`)

Files where one node has about a million children: a JSON array of numbers,
the same with single digits (the most elements that fit), and Markdown
paragraphs. Each was filled to just under the old 8 MB global parse limit; the
bench now fills each to its language's limit, so Markdown is ~3.5 MiB (the
numbers below are unchanged). The cost is one
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
the largest list under the old 8 MB parse limit (607k items, 8 MB), so highlighting
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
number of bytes. The tree gets an absolute budget of 300 MB, and each
language's parse limit is `floor(300 MiB / ratio)`, with the largest typical
ratio above for the language. The constants in
`crates/fe-syntax/src/parse_limit.rs` come from this section's ratios; they
aren't measured at runtime.

| Language | Ratio (row above) | Parse limit |
|---|---|---|
| Rust | 40× (generated items) | 7,864,320 B (7.5 MiB) |
| JSON | 30× (pretty-printed objects) | 10,485,760 B (10 MiB) |
| Markdown | 85× (short paragraphs) | 3,700,856 B (~3.5 MiB) |
| every other language | 40× (not measured; the Rust ratio) | 7,864,320 B |

The limits bound the tree of a typical file, not the worst: a JSON array of
single digits (96×) still reaches ~960 MB at 10 MiB. Above its limit, a file
has no tree and a 32 KB window around the viewport is highlighted. The limit
was a global 16 MB, then 8 MB, before this; at 16 MB trees reached 0.6–1.5 GB.

## Project tree (`cargo bench -p fe-project --bench tree`)

The fixture is `fe_project::fixture::generate` with 10 directories of 10,000
empty files (100,000 files), plus a `.gitignore` and the directory it
ignores. One run, measured 2026-10-05 (load average about 8 from other
lanes). Medians:

| Case | Time |
|---|---|
| `open` of the root (lists 11 entries) | 0.19 ms |
| `expand` of a 10,000-file directory | 13 ms |
| `apply` of a 1,000-path batch into a loaded, expanded directory | 4.5 ms |

`expand` is the listing plus storing it. Most of it is the listing
(`Listing::run`). It reads the directory through a handle (about as long as
`std::fs::read_dir`), matches each entry against the ignore rules in scope
(git's global excludes included), and sorts the entries case-insensitively.
Each of the three took roughly a third, in a timing run under load. Interleaved
in one process, a listing took 10 to 20% less than the `ignore` crate's walker
plus the same sort, which it replaced. Each listing opens `/` and every
directory down to the one it lists, checking the root and those below it
against the tree's identities, before reading and again after. It reads the
ignore files of each directory through its handle. For the fixture's
temporary directory that is about 30 opens, under 0.1 ms. `open` costs a
little over half what it did with the walker, mostly because git's global
excludes are now read once per tree instead of by every listing. The UI is
meant to run the listing on a background thread and only `fill` on its own.

The `apply` batch is 1,000 files that exist and are already in the tree, as
a build touching files would send. The directories on the way are opened once
per batch, a component at a time from `/` without following symlinks, and
checked to still be the directories the tree has, spelling included. The
ignore rules in force are read through them. Then each path costs one
`getattrlistat` (the entry's type, device, inode and on-disk name, without
following symlinks) and an ignore check, about 4.5 µs, and nothing changes.
Paths under directories that aren't loaded or are ignored cost no IO
(`changes_outside_loaded_directories_do_no_io`).

## Finder (`cargo bench -p fe-project --bench finder`)

The plan's target is first Cmd-P results in under 50 ms on 100k files. The
walk is `FinderIndex::walk` over `fe_project::fixture::generate` with 100
directories of 1,000 empty files (100,000 files), plus a `.gitignore` and
the directory it ignores, into a new index. The queries run over 100,000
in-memory paths shaped like `crates/fe-editor-0/src/view/motion_buffer_42.rs`
(`FinderIndex::from_paths`), with limit 50, and each returns a full page of
50. One run, measured 2026-10-06 on the 14-core M3 Max with a load average
of 5 to 10 from other lanes. Medians:

| Case | Time | Budget |
|---|---|---|
| walk of 100,000 files, into the index | 73 ms | none (background) |
| query, 1 char (`e`) | 2.0 ms | 50 ms |
| query, 3 chars (`edv`) | 1.3 ms | 50 ms |
| query, 8 chars (`editview`) | 1.4 ms | 50 ms |
| query, 20 chars (`crates/editor/viewrs`) | 2.3 ms | 50 ms |

Nothing is asserted. The first run, 2026-10-05, gave 86 ms for the walk and
3.0 to 6.1 ms for the queries, with a load average of 31 to 35.

Since T91 the walk goes through directory handles, as `FileTree` lists: each
directory is opened through its parent's handle without following a
symlink, and its ignore files are read through it once, on top of its
parent's rules. It replaced the `ignore` crate's `WalkBuilder`, which lists
directories by path. Measured back to back on 2026-10-06, the handle walk
took 73 and 78 ms where the `WalkBuilder` walk took 95 and 96 ms. Both use up
to 12 threads. The walk keeps at most 32 directories open for subdirectories
still to read. Past that (only in a tree more than 32 levels deep, with
directories left to read at each level) a directory is opened again from
where the walk started, a level at a time. That costs one `openat` per level
of depth: about 1.3 s more over three walks of a tree 100 levels deep with
40 directories at each level (`a_deep_tree_is_walked_without_a_handle_per_level`).

The walk runs on a background thread, and each walk thread sends its paths
every 1,024 files. Cmd-P therefore has results long before the walk ends,
over the paths indexed so far.

A query first matches every file name with frizbee's `match_list`. A file
name match outranks any full-path match, so the full paths are matched only
when fewer than 50 file names match. A needle with a `/` can't match a file
name and goes straight to the full paths. The best 50 candidates are picked
without sorting the rest, and only they get match indices
(`match_one_indices`). Single-threaded matching takes about a tenth of the
budget, so `match_list_parallel` wasn't needed and isn't benched.

## Project search (`cargo bench -p fe-project --bench search`)

The plan gives no number for search. The provisional target is the first
batch of results in under 50 ms, the finder's budget. The corpus is
generated by the bench: 100 directories of 100 Rust-like files of about
4 KB each (10,000 files, about 40 MB), plus a `.gitignore` and the
directory it ignores, which isn't searched. Every search is
case-insensitive with the default options: at most 10,000 matches, and the
walker's default number of threads. One run, measured 2026-10-06 on the
14-core M3 Max with a load average of about 15 from other lanes. Medians,
and what the bench printed for each search (with several threads, which
files a capped search reaches varies a little from run to run):

| Case | What it finds | Time | Target |
|---|---|---|---|
| rare literal (`rare_needle_token`) | 5 matches; every file searched | 158 ms | none |
| common literal (`let`) | capped at 10,000 matches in 385 files, 397 searched | 10.8 ms | none |
| regex (`fn \w+_\d+\(`) | capped, 385 files, 395 searched | 13.7 ms | none |
| whole word `Buffer`, case-insensitive | capped, 1,553 files, 1,565 searched | 19.8 ms | none |
| rare regex with a text anchor (`rare_needle_token = 1;\z`), every line checked (T96) | 5 matches; every file searched | 317 ms (rare literal 144 ms in the same run) | none |
| time to first batch (common literal, cancelled at the first batch) | | 3.1 ms | 50 ms |

Nothing is asserted. A search that finds little reads every file, so the
rare literal is the cost of the whole corpus: mostly opening and reading
10,000 files. `rg -c` on a corpus of the same shape took 110 to 130 ms
(wall clock, process start included) on the same machine the same
evening, at a load average of about 10, so the per-file cost is ripgrep's
own. The capped searches stop once
10,000 matches are found, after a few hundred files. The first batch comes
when no more results are waiting, so it arrives within a few files of the
first match, long before the walk ends.

A regex with a text anchor (`\A`, `\z`, or `^` or `$` with multi-line
mode off) or that turns off dot-all or CRLF mode (`(?-s)`, `(?-R)`) can't
be searched over a buffer of lines and agree with each line searched
without its terminator, so every line of such a search is a candidate,
checked with the regex on its own (T96). That is the "rare regex with a
text anchor" row, measured 2026-10-06 with a load average of about 30 from
other lanes: its 95% interval was 239 to 396 ms, about twice the rare
literal's. Other searches pick candidate lines over the whole buffer as
before; a rerun at a load average of about 50 put both rare cases near
450 ms, so treat the ratio as rough.

## Navigation (`cargo bench -p fe-nav --bench nav`)

The plan's target is go-to-def in under 100 ms before the language server is
ready. The corpus is generated by the bench, the same shape and size as the
project search bench's: 100 directories of 100 Rust-like files of about 4 KB
(10,000 files, 41.3 MB), plus a `.gitignore` and the directory it ignores.
Every file defines a struct, an enum, an impl, `handle` (10,000
definitions), one of 200 `shared_N` names (50 definitions each) and about
25 functions of its own. One run, measured 2026-10-07 on the 14-core M3 Max
with other lanes running, so the build rows in particular are noisy.
Medians; nothing is asserted.

**Index after the build:** 10,000 files, 330,001 symbols, 260,241 distinct
names, `approx_bytes` 63.5 MB (about 6.3 KB a file, 190 bytes a symbol).
The process's peak RSS grew from 13 MB to 87 MB (+74 MB) over the first
build, which includes the build's own transient buffers.

| Case | Time | Target |
|---|---|---|
| build, default threads (7 workers) | 2.08 s (4.8k files/s) | none |
| build, `threads: 1` | 8.96 s (1.1k files/s; 95% interval 5.8 to 13.0 s) | none |
| go-to-def, index complete: rare name (1 definition) | 0.38 ms | 100 ms |
| go-to-def, index complete: name in 50 files | 0.41 ms | 100 ms |
| go-to-def, index complete: same-file hit (50 items, unambiguous) | 0.42 ms | 100 ms |
| go-to-def, ripgrep fallback: rare name | 95 ms, **0 items, partial** | 100 ms |
| go-to-def, ripgrep fallback: common name (`handle`) | 62 ms, 50 items, partial | 100 ms |
| references, rare name (6 files; 10,001 searched, 6 verified, not truncated): first batch / complete | 0.95 ms / 703 ms | none |
| references, common name (`handle`): capped at 10,000 hits in 5,000 files (5,011 searched, 1,001 verified, truncated): first batch / capped request completion | 0.50 ms / 703 ms | none |
| workspace symbols, 1 char (`s`) | 1.07 ms | none |
| workspace symbols, 3 chars (`han`) | 1.02 ms | none |
| workspace symbols, 6 chars (`shared`) | 4.45 ms | none |

No go-to-def case is over 100 ms, but the fallback for a rare name is
close to it and wrong: it stops at the request's 80 ms budget (the rest is
parsing the request's own file and thread hand-offs) having found none of
the one definition, so it answers with nothing and `partial` set. The cause
is that the fallback text-searches the project, which for a word with one
hit reads most of the 10,000 files, and a search over this corpus takes
about 150 ms (see Project search above), so the budget runs out before the
hit's file is reached. The common name fills its 50 items from the first
files and stops early. Follow-up to file: make the fallback find a rare
name's file within the budget (for example search files in the order of
likelier definers, or give the text search more threads), or tell the user
it timed out rather than showing an empty answer. Recorded; no change here.

With a complete index go-to-def is three orders of magnitude inside the
target, so the build is what the fallback covers: the first 2 s of a
10,000-file project, and about 9 s on one thread. References have no
target. The reference rows come from a second, later run, at a higher load
(the first run read 0.33 ms and 460 ms for the rare name, 0.33 ms and
659 ms for the common one). The common name's request is capped, not run to
the end of the corpus: the 10,000-hit cap stops it after about 5,000 files,
and only the default 1,000 files are parsed to drop hits in comments and
strings, so its completion time is that of a capped request. Their first
batch is immediate and completion is dominated by reading the project's
files, as project search is. The 6-char symbol query
is four times the shorter ones because fuzzy matching a longer needle
scores more candidates.

### Language server answers: locations in unopened files

`cargo test --release -p fe-lsp decode_cost -- --ignored --nocapture`. The
fake server holds its reply on its own clock; each run's timer starts as the
clock releases it and stops when the answer reaches the caller. The interval
is the reply's serialisation, transport and parsing, then the decoding on the
calling thread (a read of each file and the offset conversions); decoding
dominates at these sizes. The references cases also wait on the companion
definitions request, answered empty. Fastest and median of 10 runs, all files
unopened in a temp dir. Measured on neon (Linux 7.0, Intel Xeon E-2386G
@ 3.50 GHz), at commit `e8bf10c` plus the test; the other Navigation numbers
are from the Mac, so compare orders of magnitude, not digits.

| Case | Fastest | Median | Target |
|---|---|---|---|
| definitions: 50 locations in 50 files of 4 KB | 1.74 ms | 2.97 ms | 100 ms (go-to-def) |
| references: 1,000 locations in 500 files of 4 KB | 23.9 ms | 37.0 ms | 703 ms (search references, complete) |
| references: 1,000 locations in one 10 MB file | 21.4 ms | 31.1 ms | 703 ms |
| references: 1,000 locations in one 10 MB non-ASCII file (UTF-16) | 24.9 ms | 40.5 ms | 703 ms |

No case is over target: definitions are over 30 times inside 100 ms, and
the references cases are over 15 times inside search's completion. No
follow-up to file.

## M3 Browse sign-off

The plan's M3 done-when is "Tree handles a 100k-file repo; finder and
search budgets met". One run of `cargo bench -p fe-project --bench tree`,
`--bench finder` and `--bench search` at commit `b16f69b`, on 2026-10-07
on the 14-core M3 Max. The machine was badly loaded, with a load average of
about 90 (it had been 130 to 160 in the previous 15 minutes), so every
number is several times worse than the quiet-machine runs in the sections
below, and they stay the better guide. Medians. Nothing is asserted, and
benches aren't part of the gate. The end-to-end behaviour on 100,000 files
(counts, no timing) is `tests::m3_signoff::a_100k_file_project_end_to_end`.

| Target | Measured | Met? | Detail |
|---|---|---|---|
| Finder: first results under 50 ms on 100k files | 1.4 to 2.5 ms for a query (1, 3, 8 and 20 chars); the walk, which runs in the background, 135 ms | met | [Finder](#finder-cargo-bench--p-fe-project---bench-finder) |
| Search (provisional, T65): first batch under 50 ms | 0.93 ms | met | [Project search](#project-search-cargo-bench--p-fe-project---bench-search) |
| Tree: `open` of the root of 100k files | 0.25 ms | no target | [Project tree](#project-tree-cargo-bench--p-fe-project---bench-tree) |
| Tree: `expand` of a 10,000-file directory | 14 ms | no target | same |
| Tree: `apply` of a 1,000-path batch | 5.2 ms | no target | same |

The search bench's other cases have no target; under this load they were
602 ms for a rare literal (every file searched), 21.5 ms for a capped common
literal, 29 ms for a regex, 563 ms for a rare regex with a text anchor and
75 ms for a case-insensitive whole word.

## Typing latency (`cargo bench -p fe-gpui --bench keystroke`)

The plan's target is keystroke to paint under 4 ms at p50 and 8 ms at p99.
The bench dispatches a keystroke through the real keymap and input handling
into a focused `EditorView`, in a 1200×800 headless GPUI window with the real
macOS text system and GPUI's headless Metal renderer, and times it until the
frame showing it has been drawn and presented. No window is put on screen, so
a locked screen doesn't matter. The harness is
`crates/fe-gpui/benches/keystroke/harness.rs`; between keystrokes it undoes
the keystroke and settles background work, untimed.

| Stage | From | To | What it covers |
|---|---|---|---|
| `dispatch` | before the keystroke | the view's first notification | keymap matching, the action or text-input handler, the model edit |
| `pre_layout` | notification | the editor's prepaint starts | the view's render, which brings the syntax tree up to date (the in-frame reparse, up to its 1 ms budget), and GPUI's layout of the element tree |
| `prepaint` | | | the editor's layout: scrolling, highlighting, shaping the visible rows |
| `paint` | | | the editor's paint |
| `draw_rest` | | the window's draw returns | the rest of GPUI's draw, including anything between the editor's prepaint and its paint |
| `present` | | the scene is committed | encoding the scene and committing it to the GPU, as presenting to a window does, without waiting for the GPU |

Cases: `type/small` types `x` inside a word in 60 lines of highlighted Rust;
`type/10k-rust` the same in 10,002 lines, around line 5,000; `type/1m-plain`
types `x` in 1M lines of plain text (33 MB, no language), mid-file;
`move/10k-rust` presses `right` in the 10k-line Rust file, the path with no
edit; `type/during-parse` types `x` into about 5 MB of Rust (30,000
functions), too big to reparse inside the frame's 1 ms budget, while the
previous keystroke's reparse is still pending in the background (the harness
asserts `needs_parse()` before each keystroke, and settles fully every 50
samples); `move/long-row` presses `right` in the middle of one 10 MB row of
plain ASCII.

Two runs, side by side, p50 / p99 in milliseconds over the measured samples
of each case, each built into a private `CARGO_TARGET_DIR`:

- **Mac** (baseline): an M3 Max MacBook Pro (36 GB, macOS 27.0), 2026-10-05
  at `9c08650`, before the keymap engine. Other lanes were building: the
  load average was 21 to 27.
- **Mac, 2026-10-17** (current): the same M3 Max, a release run of every
  case at `6193bea3`, load average 8.2 before and 7.6 after (waited for under
  10, no other lane building). Its columns are "Mac 10-17".
- **helium** (Linux, 24 cores, headless GPUI test window): 2026-10-11 at
  `7570bb2` (current main plus the two new cases), load average 13 to 15
  from other lanes. Not comparable with the Mac rows stage by stage: it is a
  different machine, text system and renderer, and `present` in particular
  is the Linux headless renderer's, not Metal's.

**`type/small`**, 5,100 samples on the Mac (10-05 and 10-17), 600 on helium:

| Stage | Mac 10-17 p50 | Mac 10-17 p99 | Mac 10-05 p50 | Mac 10-05 p99 | helium p50 | helium p99 |
|---|---|---|---|---|---|---|
| dispatch | 0.006 | 0.014 | 0.004 | 0.010 | 0.019 | 0.038 |
| pre_layout | 0.011 | 0.018 | 0.010 | 0.017 | 0.038 | 0.058 |
| prepaint | 0.176 | 0.222 | 0.161 | 0.204 | 0.310 | 0.538 |
| paint | 0.076 | 0.092 | 0.079 | 0.097 | 0.092 | 0.110 |
| draw_rest | 0.040 | 0.051 | 0.036 | 0.049 | 0.022 | 0.050 |
| present | 0.024 | 0.065 | 0.022 | 0.058 | 4.159 | 5.193 |
| **total** | **0.336** | **0.463** | **0.315** | **0.419** | **4.655** | **5.603** |

**`type/10k-rust`**, 2,550 samples on the Mac 10-05, 1,275 on 10-17, 550 on helium:

| Stage | Mac 10-17 p50 | Mac 10-17 p99 | Mac 10-05 p50 | Mac 10-05 p99 | helium p50 | helium p99 |
|---|---|---|---|---|---|---|
| dispatch | 0.007 | 0.016 | 0.004 | 0.018 | 0.021 | 0.030 |
| pre_layout | 0.581 | 0.667 | 0.584 | 0.939 | 0.016 | 1.092 |
| prepaint | 0.482 | 0.600 | 0.171 | 0.255 | 0.343 | 0.386 |
| paint | 0.083 | 0.100 | 0.084 | 0.106 | 0.100 | 0.113 |
| draw_rest | 0.044 | 0.055 | 0.041 | 0.066 | 0.020 | 0.029 |
| present | 0.033 | 0.081 | 0.028 | 0.103 | 0.292 | 2.590 |
| **total** | **1.235** | **1.466** | **0.915** | **1.413** | **0.814** | **4.044** |

**`type/1m-plain`**, 7,650 samples on the Mac (both), 600 on helium:

| Stage | Mac 10-17 p50 | Mac 10-17 p99 | Mac 10-05 p50 | Mac 10-05 p99 | helium p50 | helium p99 |
|---|---|---|---|---|---|---|
| dispatch | 0.008 | 0.032 | 0.004 | 0.009 | 0.020 | 0.051 |
| pre_layout | 0.004 | 0.013 | 0.002 | 0.003 | 0.013 | 0.025 |
| prepaint | 0.067 | 0.130 | 0.048 | 0.062 | 0.066 | 0.104 |
| paint | 0.098 | 0.123 | 0.087 | 0.104 | 0.111 | 0.130 |
| draw_rest | 0.047 | 0.078 | 0.043 | 0.054 | 0.019 | 0.047 |
| present | 0.029 | 0.146 | 0.021 | 0.048 | 4.602 | 5.298 |
| **total** | **0.256** | **0.503** | **0.209** | **0.272** | **4.835** | **5.526** |

**`move/10k-rust`**, 7,650 samples on the Mac 10-05, 3,825 on 10-17, 600 on helium:

| Stage | Mac 10-17 p50 | Mac 10-17 p99 | Mac 10-05 p50 | Mac 10-05 p99 | helium p50 | helium p99 |
|---|---|---|---|---|---|---|
| dispatch | 0.006 | 0.036 | 0.003 | 0.006 | 0.013 | 0.024 |
| pre_layout | 0.006 | 0.030 | 0.002 | 0.005 | 0.013 | 0.026 |
| prepaint | 0.531 | 2.774 | 0.048 | 0.068 | 0.119 | 0.170 |
| paint | 0.122 | 0.531 | 0.084 | 0.105 | 0.097 | 0.122 |
| draw_rest | 0.066 | 0.261 | 0.039 | 0.054 | 0.019 | 0.047 |
| present | 0.042 | 0.378 | 0.022 | 0.051 | 4.552 | 5.261 |
| **total** | **0.780** | **4.358** | **0.201** | **0.266** | **4.815** | **5.527** |

**`type/during-parse`**, 1,275 samples on the Mac 10-17, 400 on helium (new case since 10-05, so no Mac 10-05 numbers):

| Stage | Mac 10-17 p50 | Mac 10-17 p99 | Mac 10-05 p50 | Mac 10-05 p99 | helium p50 | helium p99 |
|---|---|---|---|---|---|---|
| dispatch | 0.007 | 0.017 | — | — | 0.026 | 0.043 |
| pre_layout | 0.005 | 0.010 | — | — | 0.017 | 0.033 |
| prepaint | 0.513 | 0.640 | — | — | 0.347 | 0.497 |
| paint | 0.087 | 0.103 | — | — | 0.101 | 0.123 |
| draw_rest | 0.046 | 0.059 | — | — | 0.032 | 0.066 |
| present | 0.023 | 0.081 | — | — | 3.998 | 5.106 |
| **total** | **0.682** | **0.891** | **—** | **—** | **4.529** | **5.481** |

**`move/long-row`**: the Mac 10-17 run (30,600 samples, after T20) is in the
table below the helium before/after one. The helium before/after: the row copy
(`EditorModel::row_at`) was removed in T20 (Mac T407): grapheme moves read a
1 KiB window either side of the caret, word moves scan chunks outwards, and
`LineStart` reads only the leading whitespace. "Before" is `4d37cc6` (50
samples per run), "after" is the T20 commit (600 samples per run). Each
binary was built into its own `CARGO_TARGET_DIR`, copied out, and the two
were run alternately three times under `xvfb-run` with
`--bench move/long-row`; each side is its fastest run (lowest p50). All
numbers are helium (Linux), 2026-10-12, other lanes building.

| Stage | before p50 | before p99 | after p50 | after p99 |
|---|---|---|---|---|
| dispatch | 72.331 | 73.889 | 0.011 | 0.032 |
| **total** | **72.541** | **74.115** | **5.083** | **6.073** |

**`move/long-row`, Mac 2026-10-17** (after T20, 30,600 samples):

| Stage | Mac 10-17 p50 | Mac 10-17 p99 |
|---|---|---|
| dispatch | 0.009 | 0.010 |
| pre_layout | 0.003 | 0.005 |
| prepaint | 0.007 | 0.009 |
| paint | 0.009 | 0.014 |
| draw_rest | 0.007 | 0.014 |
| present | 0.017 | 0.038 |
| **total** | **0.053** | **0.077** |

The helium after `total` is dominated by `present` (not tabulated per stage here),
the Linux headless renderer's 4 to 5 ms as in the other caret-move cases, not
the editor; the before runs' `present` was 0.18 ms in the earlier 50-sample
run, so `total` is not comparable stage by stage, only `dispatch` is. The run before T20 recorded `dispatch` 72.464 / 74.917 ms and
`total` 72.684 / 75.144 ms (p50 / p99).

On the Mac (10-17), every case fits the budget: the p50 totals are 0.05 to
1.24 ms (1% to 31% of 4 ms) and the p99 totals 0.08 to 4.36 ms (1% to 54% of
8 ms), so no case is over. The 10-05 numbers fit too (p50 5% to 23%, p99 3% to
18%). Two cases are slower than on 10-05, both in `prepaint`: `type/10k-rust`
(0.48 ms against 0.17 ms at p50, total 1.24 ms against 0.92 ms) and
`move/10k-rust` (0.53 ms against 0.05 ms, total 0.78 ms against 0.20 ms).
Both are highlighted Rust, and a lot landed after 10-05 (the keymap engine,
vim, diagnostics, find highlights); which change it is was not bisected. The
4.36 ms p99 of `move/10k-rust` is `prepaint` (2.8 ms), `paint`, `present` and
`draw_rest` all rising together at the tail; it is one run at a load of 7.6 to
8.2 and was not re-run, so it may be a tail event rather than a stage. It is
the largest Mac p99 recorded here. `type/during-parse` (0.68 ms p50, 0.89 ms
p99) costs what `type/small` does plus the `prepaint` of its larger file, as
on helium. `type/1m-plain` and `type/small` are unchanged from 10-05 within
0.13 ms. The largest stage other than `prepaint` is `pre_layout` in
`type/10k-rust`: the reparse of the edited Rust file, inside its 1 ms in-frame
reparse budget (0.58 ms at p50, as on 10-05). On 10-05 an edit to highlighted
text cost about 0.12 ms more `prepaint` than a caret move in the same file or
an edit to plain text, from highlighting the visible rows again. The first keystroke after the
editor opens is slower (2.1 ms for `type/small` in a `--test` run, mostly
`dispatch` and `present`); criterion's warm-up keeps it out of the numbers.

The 2026-10-11 helium run is after the keymap engine, vim, LSP sync,
diagnostics, find highlights and kiln landed. Leaving out `move/long-row`,
which is reported separately below, no case's p99 is near 8 ms (the highest is
5.6 ms, and they all stay under 6 ms), but on
helium that is mostly `present` (about 4 to 4.6 ms at p50 in the cases that
are unchanged on screen apart from the caret, 0.3 ms in `type/10k-rust`),
which is the Linux renderer and says nothing about Metal; the stages the
editor owns (`dispatch` through `draw_rest`) total 0.2 to 0.8 ms at p50 in
every case but `move/long-row`. `type/during-parse` types with a background
parse queued under the deterministic test executor: the harness's
`TestDispatcher` runs queued tasks only when it is driven, so the parse stays
pending but does not run concurrently with the timed keystroke. Typing with
it queued costs the same as typing with none (`type/during-parse` against
`type/small`: `prepaint` 0.35 ms against 0.31 ms, `pre_layout` 0.02 ms against
0.04 ms). That shows the keystroke path does not wait on a pending parse; it
does not show M2's "UI never blocks on a parse" while a parse is actually
running, which needs a measurement with real concurrent execution. The 4.044 ms p99 of
`type/10k-rust` comes from `pre_layout` (1.1 ms, the in-frame reparse at its
budget) and `present`.

`move/long-row` failed the budget before T20: its p99 was 75.144 ms against
8 ms, and `dispatch` was 72 ms at p50, 18 times the 4 ms target, from
`EditorModel::row_at` copying the whole 10 MB row on every arrow key;
everything after `dispatch` was under 0.2 ms. After T20 (table above) its
`dispatch` is 0.011 ms at p50 on helium and 0.009 ms on the Mac (2026-10-17).
The numbers leave out OS event delivery, waiting for vsync, the compositor,
scan-out and the GPU's own time, which need a real screen.

### Diff base and 100 carets (T160)

Two cases for per-keystroke costs the six above leave out. `type/diff-base`
is the `type/10k-rust` corpus with a git base in which one body row of every
10th function differs (167 changed rows), typing `x` at the same caret: each
new version is row-diffed when the change bar paints (`row_changes`,
imara-diff Histogram). `type/100-carets` is the same corpus with 100 carets
in the body rows of 100 functions 16 apart, the first at the usual caret,
typing `x` at every caret. The harness asserts non-empty `row_changes()` and
100 selections respectively.

- **Machine:** helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24
  threads), release build in a private `CARGO_TARGET_DIR`, one run of all 8
  cases under `xvfb-run -a cargo bench -p fe-gpui --bench keystroke`.
- **Commit:** `ac2d19e` (the cases, plus the bench's `editor` row).
  **Date:** 2026-10-17.
- **Load average** (`/proc/loadavg`, 1 minute): 34.97 at the start, 33.31 at
  the end, from other lanes building: heavily loaded. A first run of the same
  build at load 12 to 37 had `present` p99 of 11 to 32 ms in every case and
  is not used; this is the second. In this run `type/10k-rust` has a `present`
  p99 of 18.8 ms against a p50 of 1.2 ms, so p99s here are load-sensitive.
- `editor` is total minus `present`, computed per sample and then
  percentiled; it is not the difference of the two p99s.

**`type/diff-base`**, 250 samples:

| Stage | p50 | p99 |
|---|---|---|
| dispatch | 0.021 | 0.034 |
| pre_layout | 0.983 | 1.206 |
| prepaint | 2.818 | 3.640 |
| paint | 0.118 | 0.157 |
| draw_rest | 0.065 | 0.087 |
| present | 0.144 | 0.200 |
| **total** | **4.141** | **4.460** |
| editor (total - present) | 4.000 | 4.334 |

**`type/100-carets`**, 350 samples:

| Stage | p50 | p99 |
|---|---|---|
| dispatch | 0.338 | 0.467 |
| pre_layout | 0.107 | 0.209 |
| prepaint | 0.532 | 0.657 |
| paint | 0.119 | 0.156 |
| draw_rest | 0.063 | 0.083 |
| present | 0.134 | 0.193 |
| **total** | **1.295** | **1.638** |
| editor (total - present) | 1.162 | 1.461 |

Against the budgets (total p99 under 8 ms; the editor's own stages, total
minus `present`, at most 2 ms at p99):

- **`type/100-carets` meets both**: total p99 1.638 ms, editor p99 1.461 ms.
  100 carets cost `dispatch` (0.34 ms at p50, against 0.08 ms in
  `type/10k-rust`) and `prepaint` is no larger than one caret's.
- **`type/diff-base` meets the total** (4.460 ms) **and misses the editor's
  2 ms**: editor p99 4.334 ms (4.0 ms at p50). It is `prepaint`, 2.818 ms at
  p50 and 3.640 ms at p99 against 1.18 ms and 2.66 ms in `type/10k-rust` in
  this run, plus `pre_layout`, 0.98 ms at p50 against 0.03 ms. The change bar's
  `row_changes` diffs each new version synchronously. Follow-up: Mac T812 (the
  row-diff Histogram-to-Myers bound), lent to argon. The editor is not changed
  here.
- The earlier run at load 12 to 37 gave `type/diff-base` an editor p99 of
  13.9 ms and `type/100-carets` 6.4 ms, so the `100-carets` verdict is
  sensitive to load; the budgets should be re-checked on a quiet host.

## Large-file view (`cargo run --release -p fe-fileview --example first_screen` and `--example line_index`)

The target, from `docs/decisions/mmap-read-only.md`, is the first screen of
a very large file without loading it into a rope: open plus the first 60
rows under 100 ms, with RSS growth under 100 MB. The example writes a 1 GiB
file of 52-byte lines (ASCII plus `…`, `é` and `😀`) to `$TMPDIR`, so the
page cache is warm, then times `FileView::open` plus `rows_from(0, 60)`,
which reads one 256 KiB chunk with `pread`. RSS growth is the change in
`getrusage` peak RSS (3.1 MB before). The decision's acceptance size is
4 GB; 1 GiB leaves disk headroom (T158), and nothing in the path scales with
the file's length. Three runs at `b225c97` on 2026-10-09, on the 14-core
M3 Max with a load average of about 12 from other lanes.

The line index (`--example line_index`, T269) is timed on the same
generated file: `LineIndex::build` until its state is `Complete`, polled
every 200 µs. The thread reads the file 256 KiB at a time with `pread`,
bypassing the chunk cache (0 KiB cached after), and keeps the start of every
64th row: 322,639 checkpoints for 20,648,882 rows, 2.6 MB. RSS growth is the
change in peak RSS across the build, so it counts the scan buffer, the
checkpoints and the thread's stack. For comparison, the decision's eager
scan through a mapping took 7.8 s and 4.3 GB resident on 4 GB. Three runs at `496b691` (plus the example) on
2026-10-09, same machine, load average about 24.

| File | Measured | Time | RSS growth | Target |
|---|---|---|---|---|
| 1 GiB, warm | open + first 60 rows | 0.18 to 0.32 ms | 0.02 to 0.31 MB | 100 ms, 100 MB |
| 1 GiB, warm | full line index | 0.24 to 0.27 s | 3.3 to 4.3 MB | 2 s, 50 MB |

## Config reload (M5) (`cargo test --release -p fastedit --test config_reload_latency -- --ignored --nocapture --test-threads=1`)

M5's exit criterion is "TOML edits apply < 200 ms". `crates/fastedit/tests/config_reload_latency.rs`
times it: kiln's config service is set up as `main.rs`'s `config()` does it
(fastedit's `SettingsStore`, the default keymap with a user `keymap.toml` and
its `on_keymap_reload` keymap report, the user theme with fastedit's tables), on a real `ConfigWatcher`. For each of
`settings.toml`, `keymap.toml` and the user theme, 20 saves are made as editors
make them (write a temp file, rename it over) and each is timed from the rename
to the change being visible in the `App`: `editor.font_size`, a new binding for
`editor.select_all`, the theme's accent colour. The test polls every 1 ms
(`sleep` plus `run_until_parked`), so times have about 1 ms of resolution and a
floor of about 1.1 ms. The three `#[ignore]`d tests run on their own, 100 ms
apart between saves.

- **Machine:** helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24
  threads), release build. The watcher is notify's inotify here, not FSEvents.
- **Commit:** `b5014fc` plus the keymap-report callback. **Date:** 2026-10-12.
- **Load average** (`/proc/loadavg`, 1 minute): 13.18 before the first run,
  10.00 after the last. Three runs, all within the ranges below.
- **Mac:** the M3 Max, release build, the FSEvents path, 2026-10-17 at
  `6193bea3`, three runs; load average (1 minute) 7.9 before the first run,
  7.9 after the last. Per-run p50 and max are over the 20 saves of each run.

| File | Mac p50 (3 runs) | Mac max (3 runs) | helium p50 | helium max | Target |
|---|---|---|---|---|---|
| `settings.toml` | 13.4, 15.9, 14.9 ms | 24.4, 19.1, 18.4 ms | 1.1 ms | 1.2 ms | 200 ms |
| `keymap.toml` | 17.7, 21.6, 15.3 ms | 28.4, 27.6, 20.2 ms | 2.5 ms | 2.6 to 3.1 ms | 200 ms |
| user theme | 14.7, 9.3, 12.9 ms | 19.4, 20.0, 18.0 ms | 1.1 ms | 1.2 ms | 200 ms |

- **The 200 ms target holds on FSEvents too**: the slowest of the 60 Mac
  saves was 28.4 ms (keymap), 14% of the target. FSEvents is 10 to 20 times
  slower than inotify (p50 9 to 22 ms against 1 to 3 ms), from its
  coalescing latency, but no file approaches 200 ms and no follow-up is filed.
  The shortest saves (1.4 ms) are the test's polling floor.

- **The 200 ms target holds on helium's inotify path**, by two orders of
  magnitude. The keymap is slower because it rebuilds and checks the keymap
  and menus. No file is over 200 ms, so no follow-up is filed.
- **Worst case: 3 s.** kiln's `ConfigWatcher` re-stats every watched file every
  `RECONCILE` (3 s, `kiln/crates/kiln-core/src/watch.rs`) to recover saves the
  platform watcher silently drops (FSEvents on the Mac). A save whose event is
  lost applies up to 3 s late, well over 200 ms. That applies on both
  platforms; these runs (helium and Mac) lost no events, so they don't exercise it.

### Tree memory by language (`cargo bench -p fe-syntax --bench tree_ratio`)

This continues "Tree memory", under "Flat files" above, for every language
other than Rust, JSON and Markdown, and **supersedes its "every other
language" row** (40×, not measured). Each language's source is its golden
sample, `crates/fe-syntax/tests/golden/<name>.<ext>`, repeated whole to about
4 MB. A few need help to parse without errors: later copies leave out Lua's
shebang, Kotlin's package and imports and XML's prolog, and XML's copies sit
in one root element. PHP is 50 copies (0.1 MB), because tree-sitter-php stops
recognising heredocs after about 200 in a file; other languages measured at
50 copies come within 0.2× of their 4 MB ratios. The tree's memory is the
growth in malloc'd bytes across one parse, median of three, each in a fresh
`Syntax`: `uordblks + hblkhd` from glibc's `mallinfo2` on Linux.

- **Machine:** helium (Linux x86_64, glibc 2.43, AMD Ryzen 9 5900X, 12 cores /
  24 threads), release build.
- **Commit:** `f940dbb`. **Date:** 2026-10-12.
- **Load average** (1 minute): 28.38 at the start of the run, 25.27 at the
  end. Memory doesn't depend on load; an earlier run gave the same numbers.
- **macOS:** the M3 Max, release build, 2026-10-17 at `6193bea3`, with
  `malloc_zone_statistics` (bytes in use) for the tree's memory. Load average
  (1 minute) 7.95 at the start; it rose to 30 by the end, from other lanes
  starting builds. Memory doesn't depend on load.
- **No language's macOS ratio is above its `RATIOS` entry**, so no follow-up
  is filed. macOS is 1 to 8% below the helium ratio in every language
  (PHP 42.6× against 43.1×, the smallest gap; Python 40.5× against 43.1×).

The parse limit is `floor(300 MiB / ratio)`, with the ratio the measured one
rounded up, and never below 20 (HTML measured 18.5×), so that no limit passes
15 MiB. These are `RATIOS` in `crates/fe-syntax/src/parse_limit.rs`.

| Language | Source | Tree | Tree / source (helium) | Tree / source (macOS) | Ratio used | Parse limit |
|---|---|---|---|---|---|---|
| Python | 4.0 MB | 173 MB | 43.1× | 40.5× | 44× | 7,149,381 B (~6.8 MiB) |
| JavaScript | 4.0 MB | 141 MB | 35.2× | 33.8× | 36× | 8,738,133 B (~8.3 MiB) |
| TypeScript | 4.0 MB | 156 MB | 39.1× | 37.3× | 40× | 7,864,320 B (7.5 MiB) |
| TSX | 4.0 MB | 137 MB | 34.3× | 33.0× | 35× | 8,987,794 B (~8.6 MiB) |
| TOML | 4.0 MB | 141 MB | 35.3× | 31.8× | 36× | 8,738,133 B (~8.3 MiB) |
| Go | 4.0 MB | 154 MB | 38.5× | 36.1× | 39× | 8,065,969 B (~7.7 MiB) |
| C | 4.0 MB | 112 MB | 28.1× | 26.9× | 29× | 10,847,337 B (~10.3 MiB) |
| C++ | 4.0 MB | 130 MB | 32.6× | 30.9× | 33× | 9,532,509 B (~9.1 MiB) |
| Bash | 4.0 MB | 180 MB | 44.9× | 40.4× | 45× | 6,990,506 B (~6.7 MiB) |
| HTML | 4.0 MB | 74 MB | 18.5× | 17.1× | 20× (floor) | 15,728,640 B (15 MiB) |
| CSS | 4.0 MB | 101 MB | 25.1× | 23.3× | 26× | 12,098,953 B (~11.5 MiB) |
| YAML | 4.0 MB | 215 MB | 53.7× | 48.5× | 54× | 5,825,422 B (~5.6 MiB) |
| Java | 4.0 MB | 106 MB | 26.6× | 25.6× | 27× | 11,650,844 B (~11.1 MiB) |
| Ruby | 4.0 MB | 272 MB | 68.1× | 65.3× | 69× | 4,559,026 B (~4.3 MiB) |
| Lua | 4.0 MB | 161 MB | 40.3× | 39.1× | 41× | 7,672,507 B (~7.3 MiB) |
| PHP | 0.1 MB | 4 MB | 43.1× | 42.6× | 44× | 7,149,381 B (~6.8 MiB) |
| C# | 4.0 MB | 177 MB | 44.3× | 43.2× | 45× | 6,990,506 B (~6.7 MiB) |
| Scala | 4.0 MB | 215 MB | 53.8× | 51.4× | 54× | 5,825,422 B (~5.6 MiB) |
| XML | 4.0 MB | 184 MB | 45.9× | 40.8× | 46× | 6,838,539 B (~6.5 MiB) |
| Swift | 4.0 MB | 192 MB | 48.1× | 45.7× | 49× | 6,419,853 B (~6.1 MiB) |
| Kotlin | 4.0 MB | 218 MB | 54.5× | 52.3× | 55× | 5,719,505 B (~5.5 MiB) |
| SQL | 4.0 MB | 168 MB | 42.0× | 39.0× | 42× | 7,489,828 B (~7.1 MiB) |
| Dockerfile | 4.0 MB | 94 MB | 23.5× | 20.8× | 24× | 13,107,200 B (12.5 MiB) |
| Make | 4.0 MB | 129 MB | 32.3× | 29.3× | 33× | 9,532,509 B (~9.1 MiB) |

- **Twelve languages now have a lower limit than the old 40× default**
  (Python, Bash, YAML, Ruby, Lua, PHP, C#, Scala, XML, Swift, Kotlin, SQL:
  their trees would have passed 300 MB below 7.5 MiB); Ruby's is the lowest,
  about 4.3 MiB. TypeScript's is unchanged, and the other eleven get a higher
  one, up to HTML's 15 MiB.
- **One sample per language.** Like Rust's and JSON's rows above, a file
  denser in nodes than its sample costs more; the limits bound a typical
  file's tree, not the worst.
- **XML repeated whole, prolog and all, measured 72×** against 46× clean:
  error recovery inflates a tree, so the bench measures only corpora that
  parse without errors and reports any that don't.
