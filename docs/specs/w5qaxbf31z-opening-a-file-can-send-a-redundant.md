---
task_uid: "01m69aetdbgyvmzfw5qaxbf31z"
task: "T384"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "sync_colors sends no documentColor request for a buffer whose document is not yet activated (identity None)"
    required: true
    manual: false
  - id: "C2"
    text: "a regression test holds activation, draws several times, releases it and asserts exactly one documentColor request; it fails at the base commit"
    required: true
    manual: false
  - id: "C3"
    text: "colors_become_swatches and edit_asks_again_and_drops_stale_answers pass repeatedly under load"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench lsp::colors passes"
    required: true
    manual: false
---

# Opening a file can send a redundant documentColor request when a draw runs before the server activates the document (lsp colors tests see 2 requests under load) (Mac T1210)

Opening a file can send a redundant textDocument/documentColor request. Two tests in crates/fe-workbench/src/lsp_colors_tests.rs fail under load because of it, both seeing 2 requests where 1 is expected:
- colors_become_swatches at :156 (T1204 gate, run 01m60egbtbzg5q6jb9x3athxqv);
- edit_asks_again_and_drops_stale_answers at :203 (T1204 gate, run 01m60brqht3x83wjyvvr6zqzrk).

Cause (diagnosed and reproduced 1 in 120 runs under 24 CPU hogs at 85026bb30): `Workbench::sync_colors` (crates/fe-workbench/src/lsp_colors.rs:94-151) asks as soon as `server_of(buffer)` is Some, but `active_identity` stays None until the server thread runs `Document::activate` (crates/fe-lsp/src/document.rs:580, identity.rs:123). A draw before activation sends ask #1 (first, zero delay) with identity None. When the identity becomes Some, lsp_colors.rs:110-116 resets `slot.key = None` (moved is false because the old identity was None), so the next draw sends a second `first` ask. Ask #1's reply is dropped as `identity.is_none()` (lsp_colors.rs:238); ask #2 fills the swatches.

Fix: in `sync_colors`, skip a buffer whose identity is None (`let Some(identity) = identity else { continue; }`) before the key/ask logic, so the first ask waits for activation and the draw-time sync re-runs once the document activates. Alternatively, don't set `slot.key`/`asked` while the identity is None.

Regression test (new, in lsp_colors_tests.rs or a new test file next to it): hold the document's activation with fe-lsp's pause hook (identity.rs `pauses()`, `Point::Activate`, document.rs:511; it is `#[cfg(test)] pub(crate)` in fe-lsp, so it may need a test-support re-export), draw several times, release it, and assert exactly one documentColor request after the swatches appear. It must fail at base deterministically. Leave T1196's exhausted_retries fix alone.

## Implementation notes

- sync_colors skips a buffer whose active identity is None. The regression test holds activation with new test-support Manager::hold_next_activation / wait_activation_held / release_activation (over identity.rs pauses, now behind the test-support feature); it fails at base with 2 requests.
- C3 evidence: the whole `lsp::colors::tests::` set (5 tests, incl. colors_become_swatches and edit_asks_again_and_drops_stale_answers) run 60 times in a row with the fe-workbench lib test binary under 24 busy-loop CPU hogs: 60 passed, 0 failed.
