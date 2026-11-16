---
task_uid: "01m7hpdtybz3yjtwk4hr1q9r30"
task: "T667"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "debug_session.rs exposes a per-file send generation, and which generation a Breakpoints answer is for; debug_gutter.rs clears a file's pending lines only on the answer to the send they wait on (or a later one)"
    required: true
    manual: false
  - id: "C2"
    text: "Under an id-preserving fake adapter with the replacement's answer delayed, a breakpoint event and the earlier answer for a reused id leave the row pending, and the replacement's answer clears it (test in src/tests/debug_gutter/kept_ids.rs, a submodule of src/tests/debug_gutter.rs, base APIs only)"
    required: true
    manual: false
  - id: "C3"
    text: "When a pending line's earlier verification had no adapter id, an unrelated breakpoint event leaves it pending, for both a delayed and a failed replacement (test in src/tests/debug_gutter/kept_ids.rs, a submodule of src/tests/debug_gutter.rs, base APIs only)"
    required: true
    manual: false
---

# Debug gutter pending verification waits on the awaited setBreakpoints by send generation, not breakpoint ids, for adapters that keep ids across a replacement (T1881 F1) (Mac T1884)

Review follow-up of Mac T1881 (helium T665), from T1849 F12.

**F1 (medium)** crates/fe-workbench/src/debug_gutter.rs: pending verification can't tell the awaited `setBreakpoints` replacement from an earlier answer when the adapter keeps breakpoint ids across the replacement (the protocol allows it). An intervening `breakpoint` event or the stale answer for an old id that the replacement reuses clears pending early, so the row shows verified before the adapter has answered the new set.

Fix:
- crates/fe-workbench/src/debug_session.rs already numbers each send (`FileSend::generation`, `Running::next_send`). Expose it: the generation of a file's last send, and the generation that `DebuggerEvent::Breakpoints` answers (on the event or through a `Debugger` getter). A `breakpoint` event is not an answer to any send.
- crates/fe-workbench/src/debug_gutter.rs records, for each file's pending lines, the generation of the send they wait on, and clears them only when that generation's answer arrives (or a later one replaces it). A failed send keeps them pending, as T1881 has it.
- crates/fe-dap/src/fake.rs: a `Script` switch (for example `keep_breakpoint_ids`) so `setBreakpoints` hands a line's earlier id back when the line is sent again, instead of always a fresh id. Default off, so other tests are unchanged.

Regression test: crates/fe-workbench/src/tests/debug_gutter/kept_ids.rs, declared from src/tests/debug_gutter.rs (base has it; a new module declared from src/tests/mod.rs gave 0 tests at base, helium respec), written against base APIs only (the gutter's drawn marks and the T1881 test helpers, never the new generation getter). Under an id-preserving fake with the replacement's answer delayed, a `breakpoint` event and the earlier answer for the reused id leave the row pending; the replacement's answer clears it. crates/fe-dap/src/fake.rs, src/tests/debug_gutter.rs and src/tests/debug_gutter/** are test-support paths so the switch and the module exist at base.

**F5 (from T1881 review round 3, C2):** a pending line whose earlier verification had no adapter id can't be told apart from the awaited answer either, so an unrelated event clears it. The same send generation covers it. Test a delayed and a failed replacement where the pending line's prior verification has no id.
