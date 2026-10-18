---
task_uid: "01m55b79pfmp5zqydt4tzt5xyh"
task: "T819"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "PERF.md's Typing latency section has Mac p50/p99 per stage for every case the keystroke bench has, from one release run on the Mac, with date, commit and load, the 2026-10-05 Mac numbers kept beside them"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md's Config reload (M5) section has Mac p50 and max for settings.toml, keymap.toml and the user theme from three runs, and says whether 200 ms holds on FSEvents"
    required: true
    manual: false
  - id: "C3"
    text: "PERF.md's Tree memory by language has a macOS tree/source column; any language whose macOS ratio exceeds its RATIOS entry in parse_limit.rs is listed with a follow-up named"
    required: true
    manual: false
  - id: "C4"
    text: "Only PERF.md changes"
    required: true
    manual: false
---

# Take the Mac runs PERF.md is missing: keystroke bench cases, config reload on FSEvents, and tree memory ratios on macOS

## Problem

Three PERF.md measurements were taken only on Linux and say the Mac run is still to do:

- "Typing latency": `type/during-parse` and `move/long-row` have helium numbers only ("A Mac run of the two
  new cases is still to do"). The Mac is the platform the 4 ms p50 / 8 ms p99 target is for, and the only
  one with Metal's `present`.
- "Config reload (M5)": "No Mac numbers were taken. The Mac path is FSEvents and is still to run." M5's
  done-when is "TOML edits apply in under 200 ms", and FSEvents (with its coalescing latency) is the path
  users have.
- "Tree memory by language": "No macOS numbers were taken" (`malloc_zone_statistics`).

## Change

Measurement and docs only; no code changes.

- `cargo bench -p fe-gpui --bench keystroke` (every case, so the Mac rows are from one build), release,
  private `CARGO_TARGET_DIR`, load average noted. Fill the Mac columns of `type/during-parse` and
  `move/long-row` in PERF.md "Typing latency", and refresh the other cases' Mac columns (marked with the new
  date and commit; the 2026-10-05 numbers stay beside them).
- `cargo test --release -p fastedit --test config_reload_latency -- --ignored --nocapture
  --test-threads=1`, three runs. Add Mac p50 and max for each file to PERF.md "Config reload (M5)" next to
  helium's, and say whether 200 ms holds on FSEvents.
- `cargo bench -p fe-syntax --bench tree_ratio`. Add a macOS column of tree / source ratios to "Tree memory
  by language"; if any language's macOS ratio is above its `RATIOS` entry in
  `crates/fe-syntax/src/parse_limit.rs`, list it and name a follow-up; don't change the constants here.

Before measuring, check that no lane is building (`rgt lane list`) and wait for a load average under 10
(`sysctl -n vm.loadavg`) in a background loop. Each run is short (keystroke ~20 s, config reload ~10 s,
tree_ratio ~2 min).
