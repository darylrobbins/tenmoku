# Startup: what's left between launch and the first frame

Status: **measured**. Fixes are made in fastedit and fe-gpui. The 150 ms
target isn't met: the rest of the time is GPUI, AppKit and Metal, and
`runtime_shaders` isn't a warm-launch cost. Decided 2026-10-03. Source card T8 (Paddock T-29).

## Context

Warm launches presented their first frame 180–200 ms after `main`, against
a 150 ms target, and we didn't know where the time went. `runtime_shaders`
was the suspect: it compiles GPUI's Metal shaders at every launch. The
numbers, method and tables are in PERF.md under *Startup* and
*Fixed memory*.

## What `--perf` prints now

Each startup milestone, timed from the kernel's process start time (a
33 MB file; see PERF.md for what each milestone means):

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
open: first frame 244ms after launch (the file), window active: true
open: file shown 244ms after launch
```

## Findings

- **Fonts: 60–75 ms, fixed.** `EditorTheme::default_dark` listed every
  installed font to choose a family. It now resolves the preferred
  families one at a time, which takes about 5 ms.
- **Late read: the file appeared a frame after the placeholder. Fixed.** The
  read now starts at the top of `main`. If it has finished when the window
  builds its view, the file is on the first frame. That covers an empty
  file and a 33 MB one. The editor is also focused before the window's first
  draw, so focusing doesn't invalidate it.
- **Result:** the file is shown 182–200 ms after process start (median), down
  from 260–270 ms. That's five warm runs per file on a loaded machine; ten
  more runs gave medians of 194–203 ms. The best runs took 146–152 ms.
  **The 150 ms target isn't met.**
- **First presentation is a large, hidden cost.** The window's first frame
  request takes 45–60 ms, almost all of it blocked in GPUI's
  `MetalRenderer::draw`. A process's first Metal command buffer takes
  20–31 ms to schedule and adds 217 MB of footprint, even in a standalone
  program. The first milestone, which timed the request's start, missed
  this; review finding F1 caught it.
- **What's left is GPUI, AppKit and Metal.** Platform creation takes
  32–37 ms. The `NSWindow`, Metal layer and renderer take 60–65 ms; GPUI's
  `tab_bar_visible` alone makes AppKit build a tab group, costing
  10–14 ms. The window server takes 15 ms to request the first frame, and
  presenting it takes 45–60 ms. fastedit's own share is about 10–15 ms.
- **`runtime_shaders` isn't the blocker.** Compiling the shader source
  takes 0.8–1.0 ms once Metal's compiler cache has it, and 357 ms on a cache
  miss: the first launch after the shaders change. The Metal toolchain isn't
  installed here, so there's no build without the feature to compare.
- **Cold launches** lose 140–280 ms before `main`. macOS checks a newly
  written executable on its first exec.
- **Fixed memory:** about 192 MB of the ~255 MB is Metal's per-process
  memory from the first command buffer. It becomes purgeable when the window
  idles, and an idle editor's footprint is 61 MB. Drawables are 30–45 MB.
  GPUI's atlas, buffers and shaders come to under 10 MB.

## Follow-ups (not made here)

- **GPUI: skip `tab_bar_visible` at window creation when tabbing is off**
  (saves 10–14 ms). This needs a GPUI patch, either upstream or as a
  `[patch.crates-io]` fork, which is a dependency decision for the user.
- **Dropping `runtime_shaders`** would save about 350 ms only on the first
  launch after a GPUI bump. It needs the Metal toolchain in CI and touches
  `Cargo.toml` and the CI workflows. It's worth doing for shipped builds,
  but not for this target.
- **Warm Metal off the critical path:** submit an empty command buffer on a
  background thread at the top of `main`, so the 20–31 ms first-submission
  cost overlaps platform and window setup. It needs `metal` as a direct
  dependency of `fastedit` (it's already in `Cargo.lock` through GPUI), and
  it's untested whether the saving carries over to GPUI's command queue.
- **Find what invalidates the first draw** in GPUI's window setup, which
  makes the first frame request draw again (a few ms).
- **Measure startup on a quiet machine.** Load averages of 10–12 from other
  lanes and a VM spread the warm runs by ±30 ms.
