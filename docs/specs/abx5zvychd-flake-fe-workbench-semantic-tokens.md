---
task_uid: "01m66x7d6sstqfsyabx5zvychd"
task: "T1249"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The spec notes state what was traced and ruled out as the second request's trigger (forced ask timeouts and Retry, a DELTA request matching the FULL prefix, the other Retry sites), the reading that leaves the setup clock over-advance as the remaining trigger, and that no production duplicate was found; the exact assertion sent.len() == 1 stays and on failure prints each request sent, so a recurrence names the second one"
    required: true
    manual: false
  - id: "C2"
    text: "tests::lsp_diagnostics::semantic_tokens::server_tokens_recolour_and_follow_edits passes 20 runs in a row under load (e.g. alongside the rest of fe-workbench's tests); existing tests pass; clippy and fmt pass for fe-workbench"
    required: true
    manual: false
---

# Flake: fe-workbench semantic_tokens server_tokens_recolour_and_follow_edits sees 2 token requests, not 1, under load (lsp_semantic_tokens.rs:136)

Seen in T1245's finish gate (run 01m66vs7dxya85d8axqq3dh4zr, ~/.local/state/regatta/runs/01m66vs7dxya85d8axqq3dh4zr/test.fastedit.log line ~3019; T1245 touched only fe-vim cmdline.rs and fe-gpui tests). First failure in any stored run log.

crates/fe-workbench/src/tests/lsp_semantic_tokens.rs:136 `assert_eq!(sent.len(), 1)` failed with left 2: after the first tokens paint, two semantic-token requests had been sent instead of one. Likely under load a second full request goes out (a refresh, a debounce timer, or a re-request when the first reply arrives late) before the test reads `asked`.

Find why two requests were sent. If it is a real duplicate request in production (fe-workbench LSP semantic tokens), it becomes a bugfix with a regression test (new file under a tests/ module declared at base, base APIs only). If it is the test reading too early or counting an expected re-request, fix the test and prove it with repeated runs under load.

## Implementation notes

- Production is not at fault. Reading the request path: `sync_semantic_tokens`
  asks once per buffer version (`slot.seen`), each ask replaces the one in
  flight, and an ask made before the server's capabilities are known sends
  nothing (it answers `Retry`). A request goes out only after the capabilities
  are known, and a timed-out ask is deliberately asked again
  (`a_timed_out_ask_is_asked_again…`).
- The test's "first ask" loop advanced the manager's manual clock 20 ms on every
  pass. The ask's 3 s timeout is armed with the ask, so about 150 passes (a slow,
  loaded actor) run it out: the ask answers `Retry`, and the loop's `REST`
  advance fires a second FULL. That is the one path I found by reading that
  gives two FULL requests and no production duplicate.
- I did not observe it. Forcing the clock past the timeout (500 ms and 5 s a
  pass, 40 runs each, under 12 CPU burners) gave a `Retry` from the
  capabilities-unknown path in about a third of runs, never a second request, and
  150 runs of the old loop under 14 burners did not fail. So the second request's
  exact trigger is unconfirmed.
- Ruled out by tracing: the other `Retry` sites (reply timeout, opening
  mismatch, delta-from-another-opening) never fired in any run. A DELTA request
  matching the `FULL` prefix of `asked()` would need a version bump before the
  paint; the only bump logged was the test's own `type_in`.
- Fix: the loop advances the manager's clock only to wake the capability poll
  (a sleeper of 20 ms or less in `ManualClock::sleeping`), so the ask timeout
  cannot elapse. The exact count of 1 stays, and now prints the asks sent, so a
  recurrence names its method and params.
- 150 runs of the semantic-tokens tests under 16 CPU burners: 0 failures.
