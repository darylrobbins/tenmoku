---
task_uid: "01m6wt0wzwnv954k8fqdf0aygb"
task: "T330"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The decision file names the cause of the 30 s timeout of an_event_handler_edits_its_buffer_after_a_slow_process under load, with the evidence (a reproduction or the code path)"
    required: true
    manual: false
  - id: "C2"
    text: "If the cause is in the test, the fix is in crates/fe-workbench/src/plugin_host_tests.rs only, and cargo nextest run -p fe-workbench --lib plugin_host::plugin_host_tests::an_event_handler_edits_its_buffer_after_a_slow_process --retries 0 passes 30 times in a row while another cargo build runs, with the command and result in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "If the cause is in production code, nothing outside the decision file changes and the task is blocked for a respec as a bugfix"
    required: true
    manual: false
---

# Flake: fe-workbench plugin_host_tests an_event_handler_edits_its_buffer_after_a_slow_process times out after 30 s under load (a 0.3 s handler) (Mac T1500)

`fe-workbench plugin_host::plugin_host_tests::an_event_handler_edits_its_buffer_after_a_slow_process` (crates/fe-workbench/src/plugin_host_tests.rs:1522, added by Mac T1491 / helium T496, commit 74639676) failed in T1462's finish gate on the Mac (run 01m6tt7ae204hf86kn72y9e3ak, 2026-11-07 06:16 local, load ~70-145): it timed out after ~31 s with "the handler didn't finish: ...".

The handler's process only sleeps 0.3 s (`script(..., "nap", "sleep 0.3")`) and the test polls for 30 s with `std::thread::sleep(5 ms)`, so plain machine slowness is an unlikely cause: a 100x slowdown would be needed. Treat it as real until shown otherwise. Candidates: the poll loop sleeps the test thread without driving the gpui executor (no `cx.run_until_parked()` / `cx.background_executor.run_until_parked()` per iteration), so the handler's continuation after the process exits never gets polled unless some other wakeup happens; or a wakeup from the process-exit path is lost under contention; or the call deadline (CALL_DEADLINE) and the process allowance interact under load (T1347 F1 / T1491 territory).

Do: reproduce under load (run the test 30+ times while another cargo build runs: `cargo nextest run -p fe-workbench --lib plugin_host::plugin_host_tests::an_event_handler_edits_its_buffer_after_a_slow_process --retries 0`), and find the cause. If it is in the test (how it waits or drives the executor), fix it in crates/fe-workbench/src/plugin_host_tests.rs. If the cause is in production code (crates/fe-workbench/src/plugin_host*.rs or the plugin runtime), stop, write what you found in the decision file and block the task asking the lead to retype it as a bugfix with a regression test: that change is out of this chore's scope.
