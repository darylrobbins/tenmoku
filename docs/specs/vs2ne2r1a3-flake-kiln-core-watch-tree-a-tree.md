---
task_uid: "01m5k9ma0qrwnpyfvs2ne2r1a3"
task: "T1086"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "reported() waits on events/batches with a generous failure-only bound (e.g. 60 s) instead of the fixed 5 s WITHIN, so the 3 s re-stat fallback plus FSEvents latency under load cannot trip it; the passing path is still fast."
    required: true
    manual: false
  - id: "C2"
    text: "The assertion messages report what was or was not seen, not just elapsed time; no timing-based skip is added."
    required: true
    manual: false
  - id: "C3"
    text: "Proof: 50 consecutive runs of the test (cargo nextest, --retries 0) pass while a parallel full workspace build or other CPU load keeps load above 30 (no scripts/load.sh exists; use CPU burners)."
    required: true
    manual: false
---

# Flake: kiln-core watch_tree a_tree_watcher_reports_a_new_extension_dir fails under load (late root not reported within fixed 5 s window)

Seen once in a macOS stack gate (gate-stack-52 run1, load 30-84), 10.4 s run:
kiln/crates/kiln-core/tests/watch_tree.rs:67
"the late root, not reported after 5.157033375s" (the second stage: creating later/acme.late, expecting a batch naming the late root). Passed twice in isolation and on a full rerun.

Suspected cause: reported() uses a fixed wall-clock window WITHIN = 5 s, sized as the 3 s re-stat fallback plus 2 s margin. Under load FSEvents delivery and the re-stat thread are delayed past that margin (the late root also has to be registered after appearing), so the single 5 s window expires.

Fix direction (test-only): make the wait a failure-only upper bound far above the normal latency (return as soon as the batch arrives), or retry/poll until the event is observed, asserting on the batch contents rather than on elapsed time. Do not shrink the check or add a skip-if-loaded guard. Chore: test-only, no regression test.
