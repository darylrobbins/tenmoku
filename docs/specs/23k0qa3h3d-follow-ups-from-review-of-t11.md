---
task_uid: "01m41hne8mz9yrhj23k0qa3h3d"
task: "T35"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T11

Findings of the review of T11 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/examples/screenshots/main.rs:169: Two draws do not guarantee a completed frame. On a cold render, the first drain loads the language; the second draw can start a background parse or discover missing injected languages. The subsequent drain completes that work, but capture_screenshot reads the previously drawn scene without repainting. Large source files and Markdown containing fenced code can therefore produce screenshots missing highlights while reporting success. The shared capture function also makes golden comparisons dependent on whether parsing fits the synchronous budget.
  fix: Drive drawing and task completion until parsing and visible injected-language loading have settled, with bounded completion detection, then capture a frame painted after those results landed. Add coverage for a cold injected-language file and a forced background parse, keeping rendering outside plain cargo test.
  instances: crates/fe-gpui/examples/screenshots/main.rs:169

## Implementation notes

- F1's mechanism didn't reproduce. `VisualTestAppContext` runs the app in
  GPUI's test mode, where `App::flush_effects` redraws every dirty window, so
  each notify from a landed language, parse or injected language already
  redrew the window inside `run_until_parked`. With the old loop cut to one
  explicit draw, a cold Markdown file with a Python fence and 400 KB of cold
  C still matched fully highlighted goldens. The capture was correct only
  because of that undocumented behaviour.
- `capture` now observes the `EditorView` and draws (also delivering
  next-frame callbacks) until a frame and the work it starts end with no
  notification. Only then does it capture. It fails after `MAX_FRAMES` (10).
  It doesn't rely on the auto-redraw, and the sync parse budget can't
  change what's captured.
- New scenes `markdown_fence` (markdown, markdown_inline and python cold)
  and `large_c` (C cold, about 400 KB, so the first parse goes to the
  background). `Scene::cold` lists languages a scene must load first, and
  the run fails if one is already loaded or the view notified fewer than
  twice (the language, then a parse or injected language). Shrinking
  `large_c` to 5 functions makes it fail with "no background work landed
  after the language loaded". Frame counts can't stand in for this:
  because of the auto-redraw, every scene settles within two explicit
  draws.
- Rendering stays under `cargo run` only. Regenerating leaves all eight
  goldens byte-identical.
