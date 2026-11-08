---
task_uid: "01m6xjxhpzyhan3v3ap27dnxxr"
task: "T529"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "an_event_handler_edits_its_buffer_after_a_slow_process uses start_with_call_deadline with a deadline of at least 1 s (not CALL_DEADLINE) and a nap script longer than that deadline (e.g. sleep 1.5), so it still proves the deadline doesn't count the program's time while two main-thread hand-offs fit in what is left"
    required: true
    manual: false
  - id: "C2"
    text: "its timeout assertion also prints the plugin states, so a deadline stop shows as Stopped(Timeout)"
    required: true
    manual: false
  - id: "C3"
    text: "the test passes 20 runs in a row under host load 30+, with the load recorded in the finish notes; only crates/fe-workbench/src/plugin_host_tests.rs changes; clippy on fe-workbench all targets passes (vet stage)"
    required: true
    manual: false
---

# Flake: fe-workbench plugin_host_tests an_event_handler_edits_its_buffer_after_a_slow_process runs out of its 100 ms call deadline on the read+edit hand-offs under load; give it a call deadline with headroom and a longer process

Load flake in helium batch gate run 01m6xgrn8xsgg5akcg35yrfygq (commit c2097027, 2026-11-08): plugin_host_tests.rs:1545 "the handler didn't finish" after 30 s, with only "log acme.napper on plugin acme.napper: alpha" logged. First failure in ~400 runs.

Cause: the test runs at the real 100 ms CALL_DEADLINE. The program's 0.3 s is moved off the deadline (ProgramBudget::extend), but the buffer read and edit each hand off to the main thread and wait only until the call's deadline (plugin_bridge.rs Handoff::wait). The test only serves main-thread jobs inside draw()'s run_until_parked, between window draws and 5 ms sleeps, so under heavy load the edit's hand-off times out. The guest then enters its wasm $failed function, and the epoch callback (plugin_host.rs start) interrupts it before it can log, which is why neither "edited" nor "failed" appears. Production behaves as designed (the plugin_bridge.rs module doc says buffer waits stop at the call's deadline). Same family as T488.

Fix (test-only): use start_with_call_deadline(cx, Duration::from_secs(1)) (or 2 s) and a nap longer than that (sleep 1.5 / 2.5), and update the doc comment. Don't touch plugin_host.rs, plugin_bridge.rs or the other tests. This can't fail at base on demand; prove it with repeated runs under load.
