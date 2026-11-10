---
task_uid: "01m7202a44g8v9f0wxjhdak3sj"
task: "T552"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "FASTEDIT_A11Y_DUMP=\u003cpath\u003e writes Window::debug_a11y_tree_json() to \u003cpath\u003e after each frame where a11y is active, throttled to once a second (section 9 shape 3); with the variable unset nothing is written and no work is done. \"After each frame\" means: triggered by fe_workbench::AfterFrame, which Workbench::render defers past the draw, so every drawn frame (initial, child-only, platform-forced) triggers it and the tree read is that frame's, never from on_next_frame; a frame inside the throttle second is written when the second ends (the latest tree then), not dropped; the once-a-second budget is one per app, shared by all windows, and claimed (checked and set in one call) immediately before serializing; an idle window requests no frames and serializes nothing; unset, no AfterFrame is installed"
    required: true
    manual: false
  - id: "C2"
    text: "driver.sh gains an a11y-tree \u003cout.json\u003e command that walks the running app's AT-SPI tree as section 2's Spike 2 did (Xvfb, dbus-run-session, at-spi-bus-launcher, IsEnabled = true, busctl), documented in SKILL.md; its output from a running Tenmoku window is quoted in the finish notes"
    required: true
    manual: false
  - id: "C3"
    text: "Section 1 Status of docs/decisions/qenw9xxdzk-accessibility-design.md says the user approved sections 3 and 10 as written on 2026-11-09, and nothing else in the design changes"
    required: true
    manual: false
  - id: "C4"
    text: "Lifecycle tests pass: fe-workbench tests::after_frame::runs_once_after_each_drawn_frame (one call per draw for a child-only editor notify and for window.refresh, after the frame: the editor's viewport_rows already show the edit) and tests::after_frame::an_idle_window_runs_none; fastedit main.rs tests::the_a11y_dump_budget_is_claimed_once_across_windows, tests::a_redraw_inside_the_second_is_written_when_it_ends, tests::a_retry_that_loses_the_budget_waits_again and tests::an_isolated_child_redraw_is_dumped_after_its_frame (review F1-F3)"
    required: true
    manual: false
---

# A11Y-1 Dump hook: FASTEDIT_A11Y_DUMP writes the running app's a11y tree, and the Linux driver gains an AT-SPI a11y-tree probe (T294 design) (Mac T1667)

# A11Y-1 Dump hook

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-1 Dump hook** row of section 11 "Proposed card breakdown", and section 9 shapes 3 and 4, and section 2 "Spike 2".
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Review-cap respec (2026-11-10)

Review `03f6wx` (candidate `zyf5vs`, round 3) left C1 unmet on F1–F3 (medium). F1, on frame timing, has now failed twice. C2 and C3 are met; their text is unchanged. C1 is reworded so that "after each frame" says how the trigger must work. C4 is new, for the lifecycle tests. The scope widens to the four fe-workbench files named below: a hook in the window's root render is the only place that, through gpui-pre 0.3.9's public API, sees every drawn frame.

### Why the current trigger can't be fixed from `main.rs` alone
- **F1.** gpui runs `on_next_frame` callbacks in `on_request_frame` *before* it decides whether to draw and before `window.draw()` (gpui-pre 0.3.9 `src/window.rs` ~1866–1904). The a11y tree is captured in `draw` → `a11y.end_frame` (`window.rs` ~3788; `A11yDebug::capture` bumps `frame_number`). So a callback scheduled by the triggering notify reads the frame *before* the change, and nothing reads again until the next notify.
- **F3.** `cx.observe(&workbench)` only sees `Workbench`'s own `cx.notify()`. A child entity's notify (the setup wizard, an editor) or a platform refresh draws the window without telling the Workbench's observers. gpui-pre exposes no public "frame drawn" hook on `Window` (the profiler counters are behind the `profiler` feature, which fastedit doesn't enable).
- **What does see every frame.** The window's root view renders on every `draw`: it is not `.cached()`, and `draw_roots` renders it each time. Tenmoku's root is `Workbench` (`crates/fe-workbench/src/workbench.rs`, `impl Render for Workbench` ~1057). Work deferred from that render with `window.defer` / `cx.defer_in` runs when the update holding `draw` + `present` ends, so after the frame. `Workbench::render` already does this for spelling and TODOs ("Once the frame says what's in view", ~1062–1069).

### Required fix
1. **fe-workbench: an after-frame hook.** In `workbench.rs`, add and re-export from `lib.rs`:
   `#[derive(Clone)] pub struct AfterFrame(pub Rc<dyn Fn(&mut Window, &mut App)>); impl Global for AfterFrame {}`.
   At the top of `Workbench::render`: `if let Some(hook) = cx.try_global::<AfterFrame>().cloned() { window.defer(cx, move |window, cx| (hook.0)(window, cx)); }`. That is one global lookup per frame when nothing is installed. Doc comment: it runs once after each frame the window draws, initial, child-only and platform-forced frames alike, and never before the draw.
2. **main.rs: no observer, no `on_next_frame`.** Delete `cx.observe(&workbench, …)` and the `on_next_frame` callback. When `a11y_dump_path()` is `Some`, `main` installs `AfterFrame` once, through a testable `install_a11y_dump(path, source: A11ySource, cx)`, where `struct A11ySource { active: Rc<dyn Fn(&Window) -> bool>, read: Rc<dyn Fn(&mut Window, &mut App) -> Option<String>> }`. The production source is `Window::is_a11y_active` / `Window::debug_a11y_tree_json`. When unset, nothing is installed (C1's "no work"). `watch_window` no longer calls a dump function.
3. **Throttle (F2), claimed atomically, per app.** Replace the `static A11Y_DUMP_LAST_WRITE` with a gpui `Global`, `A11yDumpBudget { last: Option<Instant> }`, shared by every window of the app. Give it `fn claim(&mut self, now: Instant) -> Result<(), Duration>`: `Ok` and sets `last = now` when at least `A11Y_DUMP_EVERY` has passed (or never written); else `Err(remaining)`. Checking and setting happen in that one call, immediately before serializing. Nothing serializes without a successful `claim`. Take `now` from `cx.background_executor().now()`, not `Instant::now()`, so test time (`advance_clock`) and timers agree.
4. **Per-window state.** Keep a map from `WindowId` to `DumpGate { drawn: bool, retry: bool }` (drop the `frame_number` parse, or keep it only as an extra dedupe). `DumpGate::frame_drawn(&mut self, budget, now) -> Next` and `DumpGate::retry_due(&mut self, budget, now) -> Next`, where `enum Next { Write, RetryIn(Duration), Nothing }`:
   - `frame_drawn`: set `drawn`. If `retry` is pending → `Nothing`. Else `claim`: `Ok` → `drawn = false`, `Write`. `Err(w)` → `retry = true`, `RetryIn(w)`.
   - `retry_due`: `retry = false`. If `!drawn` → `Nothing`. Else `claim` as above. Losing the budget to another window that wrote in the meantime gives `RetryIn` again; the dump is never dropped.

   The hook: if `!(source.active)(window)` (cheap), return before touching the gate (activation forces a redraw, which calls the hook again). `Write` → `(source.read)(window, cx)` and `write_a11y_dump` if `Some`. `RetryIn(w)` → `window.spawn(cx, async move |cx| { timer(w).await; cx.update(|window, cx| …retry_due… ) })`. The retry runs between frames, so it reads the latest completed frame. A closed window drops its task; remove its map entry in `cx.on_window_closed` or when an update fails. An idle window draws no frames, so the hook never runs and nothing is serialized. Nothing here requests a frame.

### Required tests (C4)
- **fe-workbench**, new `crates/fe-workbench/src/tests/after_frame.rs` (declared in `tests/mod.rs`), with a workbench window opened as `tests/minimap_setting.rs::open` does and a counting `AfterFrame` installed:
  - `tests::after_frame::runs_once_after_each_drawn_frame`: (a) a child-only notify, an edit through an open file's `EditorView` (`view.model_mut().insert(…)`, `cx.notify()` on the view, not the Workbench), then one draw → exactly one call; inside that call the editor's `viewport_rows()` already shows the inserted rows (proof that it ran after the frame); (b) `window.refresh()` then one draw → exactly one more call.
  - `tests::after_frame::an_idle_window_runs_none`: after the initial draw, `run_until_parked` and `advance_clock(5 s)` → no further calls.
- **fastedit `main.rs`** `mod tests`:
  - `tests::the_a11y_dump_budget_is_claimed_once_across_windows`: two `DumpGate`s on one `A11yDumpBudget`, both `frame_drawn` at the same `now` → one `Write`, one `RetryIn(1 s)`. At `now + 1 s` the second's `retry_due` → `Write`.
  - `tests::a_redraw_inside_the_second_is_written_when_it_ends`: `frame_drawn` at t0 → `Write`; t0+200 ms → `RetryIn(800 ms)`; t0+500 ms → `Nothing`; `retry_due` at t0+1 s → `Write`; a later `retry_due` with no frame → `Nothing`.
  - `tests::a_retry_that_loses_the_budget_waits_again`: another gate claims at the retry's instant → the retry gets `RetryIn` again, not `Nothing`.
  - `tests::an_isolated_child_redraw_is_dumped_after_its_frame`: a Workbench window (as the existing `a_window_on_another_root_waits_for_its_settings` makes one) with `install_a11y_dump(tempfile, source)`. The test `source` says active and reads JSON of the active editor's `viewport_rows()`, counting its reads. Open a 3-row file, draw, `advance_clock(1.1 s)`; insert 10 rows through the editor only (child notify), draw once. The file now holds the post-edit rows. Then `advance_clock(5 s)` with no draw → the reader's call count doesn't change.
- Keep `the_a11y_dump_replaces_the_file_whole` and `the_a11y_dump_is_off_without_its_variable`. Replace `the_a11y_dump_gate_schedules_one_dump_per_second_and_frame` with the tests above.

Headless gpui never activates a11y (design section 9), which is why `A11ySource` is injected. Say so in the Implementation notes, with the gpui line numbers above for F1/F3. C2's AT-SPI evidence from round 1 still stands: re-quote it in the finish notes; there's no need to re-run it unless the probe changed. Finish once.

## Implementation notes

- The trigger is `fe_workbench::AfterFrame` (a `Global`), deferred from the top of `Workbench::render` with `window.defer`, so it runs after the draw and present. `main.rs` installs it through `install_a11y_dump(path, A11ySource, cx)` only when `FASTEDIT_A11Y_DUMP` is set. No `cx.observe`, no `on_next_frame`, no timer except the one-shot retry.
- Why not `main.rs` alone (earlier rounds, F1/F3): gpui-pre 0.3.9 runs `on_next_frame` callbacks in `on_request_frame` (`src/window.rs` ~1866-1904) before it decides to draw and before `window.draw()`; the a11y tree is captured in `draw` → `a11y.end_frame` (~3788, `A11yDebug::capture`). A callback scheduled by a notify therefore reads the frame before the change. `cx.observe(&workbench)` sees only the Workbench's own notifies, not a child's (setup wizard, editor) or a platform refresh.
- The once-a-second budget is `A11yDumpBudget`, a `Global`: `claim(now)` checks and sets in one call, just before serializing. `DumpGate` (per window) turns a drawn frame inside the second into a one-shot retry that writes the latest tree when the second ends, and a retry that loses the budget to another window waits again.
- Headless gpui never activates a11y (design section 9), so `A11ySource { active, read }` is injected: the app's is `Window::is_a11y_active` / `debug_a11y_tree_json`; `an_isolated_child_redraw_is_dumped_after_its_frame` reads an editor's `viewport_rows()` instead.
- "After the frame" is proved in `tests::after_frame::runs_once_after_each_drawn_frame` two ways: the hook count equals the editor's drawn-frame count (`FrameTimings`), and in the last call the editor has already scrolled to a caret that only its own frame reveals.
