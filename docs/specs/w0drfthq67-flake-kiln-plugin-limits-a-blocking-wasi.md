---
task_uid: "01m6xn6hsc5ey65sw0drfthq67"
task: "T530"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "the elapsed-time asserts of a_blocking_wasi_import_times_out_in_init, _in_a_call and _in_shutdown (limits.rs:187, :245, :275) allow enough headroom over the 250 ms call deadline to pass under host load 50+ (e.g. 5 s), and bounded()'s hang guard (limits.rs:52) is raised above that bound, so a plugin that blocks for ever still fails the test"
    required: true
    manual: false
  - id: "C2"
    text: "each of the three tests still asserts Reason::Timeout, and each elapsed assert's message names the deadline it is measured against"
    required: true
    manual: false
  - id: "C3"
    text: "the three tests pass 20 runs in a row under host load 30+, with the load recorded in the finish notes; only kiln/crates/kiln-plugin/tests/limits.rs changes; clippy on kiln-plugin all targets passes (vet stage); no comment names an app crate (no_gpui boundary)"
    required: true
    manual: false
---

# Flake: kiln-plugin limits a_blocking_wasi_import_times_out_in_init overruns its 1 s elapsed bound under load (1.04 s, limits.rs:245); give the blocking-WASI timeout tests headroom

Load flake in helium batch 137 gate rerun 01m6xjrkpcpsqygatymj2t3j89 (commit c2097027, 2026-11-08, load ~50): kiln-plugin::limits a_blocking_wasi_import_times_out_in_init panicked at kiln/crates/kiln-plugin/tests/limits.rs:245 with "1.039334175s". The load did end in Reason::Timeout, as the first assert checks; only the wall-clock bound `elapsed < 1 s` overran. Its siblings _in_a_call (1.18 s test time) and _in_shutdown (1.06 s) carry the same 1 s bound (limits.rs:187, :275) and sit close to it.

The bound is meant to prove the 250 ms call deadline stops a guest blocked in a WASI import (see T261 / kiln-plugin deadline handling: call_async raced against the deadline), not to time the scheduler. bounded() (limits.rs:48-52) already catches a guest that blocks for ever with a 2 s recv_timeout.

Fix (test-only): widen the three elapsed bounds to a few seconds and raise bounded()'s guard above them. Same family as T120's fix (8f6d6c3f), which gave a_call_that_returns_past_its_deadline_times_out retries on the load. Don't change kiln-plugin src. This can't fail at base on demand; prove it with repeated runs under load. kiln/ is under the no_gpui boundary: comments must not name fe-* crates or fastedit.
