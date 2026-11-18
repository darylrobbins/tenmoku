---
task_uid: "01m7r7tnmedsth6x20atqeqmmm"
task: "T386"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "the_preview_setting_applies_live passes 20 of 20 consecutive runs while the machine is loaded (load average above 25, e.g. a parallel cargo build or 30 busy loops, bounded by timeout), and the plugins test binary's other tests pass in the same runs"
    required: true
    manual: false
---

# Flake: fastedit::plugins the_preview_setting_applies_live times out waiting for the plugin to load under load (Mac T1961)

# Flake: the_preview_setting_applies_live times out waiting for the plugin to load

Seen in a Mac full gate under load about 25: fastedit::plugins the_preview_setting_applies_live (crates/fastedit/tests/plugins.rs:216) failed with "timed out waiting for the plugin to load". The `until` helper (plugins.rs ~line 142) draws and polls for 60 s before it panics. The test passed 3 of 3 in under 1 s on rerun.

The deadline is already 60 s against a sub-second normal run, so a plain slow compile is an unlikely cause: the editor.wasm guest is small and the engine is shared. More likely the wait never sees the load finish: save_settings sends one watcher batch, the host decides on a background thread (plugin_host.rs `decide`, ~1061), then loads on another background task (~1496) and reports back through cx.spawn, while `draw` uses run_until_parked on a test executor with real threads. A missed or reordered wake-up, or the load racing the batch, would hang regardless of the deadline. First find out which: on timeout, print plugin_states, the host's diagnostics and cached(state) in the panic message, and reproduce under load with a loop (for example 30 busy loops under timeout, 50 runs). Then fix the cause in the test, for example have `until` resend the settings batch or wait on an event the host emits, rather than raising 60 s. Only if the evidence shows a genuine slow stage may the deadline grow, with the evidence in the commit.

Test-only: no production change unless the repro shows a lost wake-up in plugin_host.rs, in which case name the file in a respec. No regression test.

## Implementation notes

- Repro attempt: 100 runs of the plugins binary under 30-40 busy loops (load average 40-70) never failed, before or after the change, so no stage was shown to be slow and no lost wake-up in plugin_host.rs was observed. Deadline kept at 60 s.
- The timeout panic now prints plugin_states and the diagnostics, so a recurrence names its stage.
- `save_settings` now waits for its outcome and resends the watcher batch each second until it holds, so a batch taken before the host was ready cannot hang the test.
