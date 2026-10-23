---
task_uid: "01m5myrdvfpw318bwynknwy5yt"
task: "T1088"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The minimap bench delivers scheduled frame callbacks (simulate_next_frame or equivalent) before each sampled draw and adds their time to the frame total; settle drains continuations until none is scheduled"
    required: true
    manual: false
  - id: "C2"
    text: "The sweep is reported separately per case (callback time p50/p99 and continuation frame count) beside frame p50/p99 and prepaint/paint"
    required: true
    manual: false
  - id: "C3"
    text: "All four cases rerun with the minimap off and on, A/B by fastest sample; PERF.md Minimap section updated, T1034's not-included note replaced, and any threshold miss (on p99 \u003c= 8.33 ms smooth/typing, jump p50 within 0.5 ms) reported"
    required: true
    manual: false
  - id: "C4"
    text: "scroll.rs and the keystroke harness users still build and their numbers are unchanged; no minimap behaviour change"
    required: true
    manual: false
---

# Include the minimap's between-frame sweep in the minimap benchmark

# Include the minimap's between-frame sweep in the minimap benchmark

T1034's benchmark (crates/fe-gpui/benches/minimap.rs) measures per-frame draw cost only. GPUI's test
platform records scheduled frame requests (`window.on_next_frame`, `request_animation_frame`) but nothing
in the bench or its harness delivers them, so `sweep_step` (crates/fe-gpui/src/element/minimap.rs) and
the build continuations never run in the timed cases, and the settle helpers cannot tell that the
minimap's continuations have finished. Production runs that work before drawing a frame. Review finding
F8 of T1034 (benches/minimap.rs lines ~178, 255, 282; keystroke/harness.rs ~544; minimap.rs ~753).

## Do

- In the minimap bench, deliver scheduled frame callbacks with `simulate_next_frame` (or an equivalent
  platform-frame pump) before each sampled draw, and add their time to that frame's sampled total.
- Report the sweep separately as well: per case, the sweep's callback time per frame (p50/p99) and the
  number of continuation frames it needed, next to the existing frame p50/p99 and prepaint/paint.
- Make `settle` drain continuations: pump frame callbacks until none is scheduled (bounded, and assert it
  stops) so a timed run starts with the minimap's tiles and sweep complete.
- Rerun all four cases, minimap off and on, A/B by fastest sample, alternating runs (PERF.md "Measuring"),
  and update the Minimap section of PERF.md: replace T1034's "sweep not included" note with the new
  numbers. If minimap-on frame p99 with the sweep exceeds 8.33 ms on smooth or typing, or jump p50 is
  more than 0.5 ms over minimap-off, report it in PERF.md and the PR; do not change the minimap here.
- Bench-only: do not change minimap behaviour. A harness change is allowed only to expose the frame pump.

## Scope

crates/fe-gpui/benches/minimap.rs, crates/fe-gpui/benches/keystroke/harness.rs, crates/fe-gpui/benches/scroll.rs, PERF.md.
(scroll.rs includes `keystroke/harness.rs`: it must still compile and its numbers must not change. Edit it only as a harness change forces, minimally, and name it in the PR.)
