---
task_uid: "01m6t5mkzk10xbn6ad78x56vkh"
task: "T496"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "plugin_host::plugin_host_tests::an_event_handler_edits_its_buffer_after_a_slow_process passes: the handler's process outlasts the call deadline, then its buffer read and edit succeed"
    required: true
    manual: false
---

# Plugin host: test that an event handler can read and edit its buffer after running a process longer than the call deadline (T1347 F1) (Mac T1491)

Follow-up to Mac T1347 (neon T278, review 50z6r1 round 2, F1 medium, carried):

> crates/fe-workbench/src/plugin_host.rs:2408 — The target's process deadline allowance does not reach the editor bridge's CallDeadline, which remains fixed at call entry. An event handler that runs a granted process for 300 ms remains within its extended deadline, but subsequent buffer reads, edits or proposals immediately fail against the original 100 ms deadline, even when the buffer remains open and unchanged.
> Fix: make the bridge use the same process-adjusted deadline as the runner. Add a regression test whose event handler runs a process longer than the call deadline and then successfully reads or edits its buffer.

It may already be fixed on main: the landed code builds the bridge deadline with `CallDeadline::new(kiln.program_runs.clone())` (plugin_host.rs ~1690), and plugin_bridge.rs documents that the call's deadline does not count the time a process runs. Nobody has tested it.

Do: add a test to crates/fe-workbench/src/plugin_host_tests.rs (follow its existing WAT/fixture plugin style) in which a plugin's event handler (e.g. on buffer saved or changed) runs a granted process that takes longer than the call deadline (sleep ~300 ms), then reads its buffer and makes an edit; assert both succeed and the edit is applied.

If the test passes on main, that is the whole task. If it fails, block the task with the failure output and ask the lead before changing production code (the fix would go in plugin_host.rs / plugin_bridge.rs and the card becomes a bugfix).
