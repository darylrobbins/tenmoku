---
task_uid: "01m4evvbktbrjrck1a4mppqb8c"
task: "T212"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T211

Findings of the review of T211 that don't block the merge (below merge.block_severity):

- F1 medium docs/specs/jdtt72nj42-flaky-under-load-fe-config-tests-watch.md:49: The recorded root cause contradicts the actual registration sequence. ConfigWatcher invokes the hook after watcher.watch() returns, and the locked notify 8.2.0 backend waits until FSEventStreamStart before returning. The hook writes while registering c, the final directory, with no subsequent registration to restart the stream. Thus the original write already occurs with a live stream; the added write provides another stimulus but does not eliminate the claimed registration race. The repeated passes do not establish this diagnosis.
  fix: Investigate and record a cause consistent with the actual startup sequence, explain how the additional write addresses it, and correct the helper comment. If investigation identifies a production watcher defect, follow the spec's stop-and-respec instruction.
  instances: docs/specs/jdtt72nj42-flaky-under-load-fe-config-tests-watch.md:49, crates/fe-config/src/tests/watch.rs:283
