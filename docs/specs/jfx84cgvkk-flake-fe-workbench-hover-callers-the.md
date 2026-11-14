---
task_uid: "01m7ch5rag9z8wy2jfx84cgvkk"
task: "T382"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "the_footer_fills_late_without_moving_the_card no longer misses the incomingCalls request under load; the cause is named in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "20 runs of the fe-workbench hover_callers tests alongside a parallel cargo build all pass, recorded in the decision file"
    required: true
    manual: false
---

# Flake: fe-workbench hover_callers the_footer_fills_late_without_moving_the_card waits 60 s for incomingCalls under machine load (Mac T1869)

`fe-workbench tests::hover_callers::the_footer_fills_late_without_moving_the_card` (landed with T1602) failed in T1583's Mac finish gate on 2026-11-14: it sat 61 s in `fake.wait_for("the incoming calls", …)` until the fake language server's 60 s timeout, with the Mac's load average around 33 (a stack gate's test stage running alongside). The other 5028 tests passed; on the lane, all 8 hover_callers tests passed three runs in a row at about 2 s each.

Find out why the `callHierarchy/incomingCalls` request can fail to arrive under load. If it is the test (a wait that depends on a frame or timer being driven while the test blocks on the fake), fix the test so it drives whatever sends the request. If it is a real race in the caller-count request path (crates/fe-workbench/src/lsp.rs, crates/fe-lsp/src/hierarchy.rs: e.g. the request only goes out on a frame that never comes, or a prepare reply racing the hover), fix it there and add a regression test that fails at base.

Prove it with 20 runs of the hover_callers tests alongside a parallel `cargo build` of another crate, recorded in the decision file. The other hover_callers tests use the same wait; fix them all if they share the cause.
