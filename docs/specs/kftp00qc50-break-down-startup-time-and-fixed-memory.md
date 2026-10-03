---
task_uid: "01m417z3t75e67wjkftp00qc50"
task: "T8"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "fastedit --perf prints startup phases from process start (to main, file loaded, app callback, window open, first prepaint, first paint); manual: sample output in the decision file, build and vet stages pass"
    required: true
    manual: false
  - id: "C2"
    text: "Manual: PERF.md has a Startup section with the phase table from 5 warm launches and 1 cold launch, for an empty file and the 33 MB 1M-line file, plus with and without runtime_shaders if the Metal toolchain is installed"
    required: true
    manual: false
  - id: "C3"
    text: "Manual: PERF.md has a Fixed memory section splitting the ~255 MB growth of an empty editor into drawables, glyph atlas, shaders and other, naming the tool used"
    required: true
    manual: false
  - id: "C4"
    text: "Manual: warm first paint is under 150 ms, or PERF.md names the blocker with its measured cost"
    required: true
    manual: false
---

# Break down startup time and fixed memory, and get first paint under 150 ms or name the blocker

Paddock T-29 (P3, draft: needs the user's approval before `rgt task ready`).

## Problem

From `fastedit --perf` (PERF.md, 2026-10-02, release build, M3 Max):

- **First paint.** Warm launches paint their first frame 180–200 ms after launch whatever the file size
  (loading a 100 MB file is only 43 ms of that). The plan's target is under 150 ms. The first launch after a
  build took 490 ms. We don't know where the time goes. `--perf` measures from the `Instant` taken at the
  top of `main` (`crates/fastedit/src/main.rs:126`), so it also misses exec and dyld time before `main`.
- **Fixed memory.** An empty editor's physical footprint is 21 MB at the first frame and 276 MB after a
  few hundred frames. About 255 MB appears whatever the file, and is mostly GPU memory: drawables, glyph
  atlas, compiled shaders. We don't know the split.

One known suspect: the `runtime_shaders` feature of `gpui_platform` (root `Cargo.toml:26-29`) compiles the
Metal shaders at every launch, so builds don't need Xcode's separately downloaded Metal toolchain.

## Change

1. **Startup breakdown in `--perf`.** In `crates/fastedit/src/main.rs` and `crates/fastedit/src/perf.rs`,
   record instants at: process start (from the kernel's process start time via `libc`, which fastedit
   already depends on), entering `main`, file loaded, the `Application::run` callback starting,
   `open_window` returning, and the first prepaint and first paint (from `FrameTiming`). Print each phase's
   duration on the `open:` lines.
2. **Measure.** Run `--perf` on an empty file and on the 33 MB 1M-line file: five warm launches each, plus
   one cold launch after a build. If the Metal toolchain is installed (`xcrun -sdk macosx metal -v`),
   repeat the warm runs with a local, uncommitted build without `runtime_shaders` and record the
   difference.
3. **Fixed memory.** Break down the ~255 MB growth for the empty file with macOS's tools (`footprint
   <pid>`, `vmmap --summary`, or Instruments' Metal resource allocations): drawables (count × size at
   1200×800 @2x), glyph atlas, shader libraries, and the rest. Note what precompiled shaders or a smaller
   atlas would save, without changing GPUI.
4. **Write it up** in PERF.md: replace "Cold start to first paint" under *Not measured yet* with a
   *Startup* section (the phase table, warm and cold) and a *Fixed memory* section. If first paint can get
   under 150 ms with a change inside `crates/fastedit`, make it. Otherwise name the blocker and its
   measured cost.

Don't change the root `Cargo.toml`, `Cargo.lock` or CI here. Dropping `runtime_shaders` needs the Metal
toolchain in CI and is the user's call (see below).

## Open questions

- If `runtime_shaders` turns out to be the blocker: drop it for release builds and install the Metal
  toolchain in CI (`xcodebuild -downloadComponent MetalToolchain`)? That touches `Cargo.toml` and
  `.github/workflows/**`, which are human paths. It would be a follow-up card once the numbers are in.

## Implementation notes

- Files outside the listed scope changed, all minimally:
  - `crates/fe-gpui/src/lib.rs`: `EditorTheme::default_dark` called `all_font_names()`, which costs
    60–75 ms on the main thread before the window opens and was the largest phase fastedit controls. It
    now resolves each preferred family with `resolve_font` and checks with `get_font_for_id` that the
    family didn't fall back. GPUI's `font_id` is private. The chosen font is unchanged (JetBrains Mono
    here).
  - `crates/fe-gpui/src/{timing,element,tests}.rs`: `FrameTiming` records when the editor's prepaint
    started (review finding F2). Reconstructing it from durations counted other elements' prepaint as the
    editor's.
  - `crates/fastedit/src/loader.rs`: `start` runs the existing `read` on its own thread from the top of
    `main`, so a small file is ready when the window builds its view and is drawn in the first frame.
- `--perf` also prints memory "after settling" (60 frames after file shown).
- Review finding F1: the first frame was timed when the first frame request started, which is before it
  draws and presents. Now a task spawned at each frame request's start records when the request returned.
  "First frame requested" is its own milestone, and the startup lines print only after the file is shown.
  The re-measured first paint is 182–200 ms (median), not under 150 ms; PERF.md names the blocker.
- Measuring needs an unlocked screen. While locked, the window gets no frame requests after the first, so
  `--perf` waits.
- Metal toolchain not installed, so there are no measurements without `runtime_shaders`. Shader compile
  cost was timed in a standalone program instead (PERF.md, *Startup*).
- Measurement hazard: with the shared cargo target dir, another lane's `fe-gpui` (same metadata hash)
  replaced this lane's, and cargo saw it as fresh, so a release build silently lacked the font fix. The
  numbers in PERF.md come from builds in a private target dir.
- Findings and follow-ups: `docs/decisions/kftp00qc50-startup-blockers.md`.
