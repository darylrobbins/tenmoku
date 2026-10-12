---
task_uid: "01m4rfwh24e1wpzyg7xw0b4m2t"
task: "T38"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fastedit/tests/config_reload_latency.rs builds in the gate; its #[ignore]d tests time 20 rename-over saves of each of settings.toml, keymap.toml and the user theme to the change visible in the App, through a real ConfigWatcher"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md has a 'Config reload (M5)' section with p50 and max per file, the 3 s RECONCILE worst case under silent FSEvents loss, and whether the 200 ms target holds"
    required: true
    manual: false
  - id: "C3"
    text: "No non-test source changes; Cargo.lock changes only in fastedit's dependency list and gains no [[package]]"
    required: true
    manual: false
---

# Measure how long a saved settings, keymap or theme TOML takes to apply, against M5's 200 ms (Mac T413)

Filed on helium from Mac T413 (~/coordination/helium-cards/T413.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-s.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Helium notes (from the Mac lead)

- The Mac's dependency T246 (kiln theme) is already on helium's main as 801e1af, so no depends-on.
- kiln's ConfigWatcher uses notify, so on Linux this times inotify, not FSEvents. Read every "FSEvents" below as "the platform watcher (inotify here)".
- PERF.md: head the measured columns "helium" and give the machine line, as PERF.md's "Buffer" section (T32, Mac T404) does: machine, commit, date, load average. Leave the 3 s RECONCILE worst case as the spec states it (it applies on both platforms). Say plainly that no Mac numbers were taken.
- Cargo.lock may change only in fastedit's own dependency list (criterion C3); the lead calls it out when this lands.

## Problem

M5's exit criterion is "TOML edits apply < 200 ms". It was never measured. Saving `settings.toml`,
`keymap.toml` or the user theme file reaches the app through kiln's config service. kiln-core's
`ConfigWatcher` (FSEvents) sees the save, the service reloads the store on the main thread, and fastedit
applies the result. T334 added a re-stat of every watched file every `RECONCILE` (3 s,
`kiln/crates/kiln-core/src/watch.rs:52`). That recovers saves FSEvents silently drops, but the save then
applies up to 3 s late, well over 200 ms.

T246 (which this depends on) brings the theme mode and user theme file through kiln's config. After it,
all three files take the same path.

## Change

Measurement only.

- `crates/fastedit/tests/config_reload_latency.rs` holds `#[ignore]`d `#[gpui::test]`s on a real watcher.
  They follow `kiln/crates/kiln-platform/tests/config.rs::the_watcher_reloads_settings_saved_on_disk`:
  `cx.executor().allow_parking()`, and saves made the way editors make them (write a temp file, rename it
  over).
  - Configure kiln's config service the way `crates/fastedit/src/main.rs` `config()` does, with fastedit's
    `SettingsStore`, the default keymap with a user keymap file, and the user theme file.
  - For each of the three files, save 20 changes and time each one from the rename to the change being
    visible in the `App`: the setting's new value, the new binding resolving, the new theme colour. Poll
    every 1 ms.
  - Print p50 and max per file.
  - Add `gpui` (feature `test-support`) and `tempfile` as dev-dependencies in `crates/fastedit/Cargo.toml`.
    Both are already in the lock.
- PERF.md gains `## Config reload (M5)`, with the run command and the table. It states the 3 s `RECONCILE`
  worst case when FSEvents drops events, and says whether the M5 target holds on the FSEvents path. If p50
  is over 200 ms for a file, the section names the slow stage (watcher delivery or reload) and the follow-up
  to file.
