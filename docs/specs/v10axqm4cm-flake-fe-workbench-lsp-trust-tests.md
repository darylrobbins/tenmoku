---
task_uid: "01m5r39sg73asb07v10axqm4cm"
task: "T1170"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Every assertion in server_lenses_show_above_their_rows is kept (including exactly one codeLens request and one resolve request after the lenses first show), and no skip-under-load guard or retry is added; the fix makes the test wait for (or the lens code not send before) the buffer's active identity."
    required: true
    manual: false
  - id: "C2"
    text: "Proof: 20 consecutive runs of fe-workbench lsp::trust_tests::lenses::server_lenses_show_above_their_rows (cargo nextest, --retries 0) pass while 3x-ncpu 'yes' burners keep load above 30."
    required: true
    manual: false
---

# Flake: fe-workbench lsp::trust_tests::lenses::server_lenses_show_above_their_rows fails under load (a second codeLens request: the first was sent before the document's identity was active)

`lsp::trust_tests::lenses::server_lenses_show_above_their_rows` (crates/fe-workbench/src/lsp_lenses_tests.rs:168) asserts exactly one `textDocument/codeLens` request after the lenses first show. The daemon's fast check of main f92d54fb failed it under load ("left: 2, right: 1", run 01m5r03bhqwtyspcmq8ka7dzzq); the same commit passed 5367/5367 in a full gate.

Reproduced under 42 `yes` burners (load 36-45) at roughly 1 run in 25-40, with temporary eprintln tracing in a scratch worktree. A failing run shows: `send_lenses Version(0) identity=None key=Some(Version(0))`, then `identity None -> Some((SessionId(1), IncarnationId(1)))`, then a second `send_lenses` at the same version. The first `Workbench::sync_lenses` after the first draw sees `server_of(buffer)` already set but `active_identity` still None, so the first request is sent with no identity; `lenses_landed` and `lens_request_current` discard an answer with no identity. When the identity turns active, `sync_lenses` resets `slot.key` and asks again. That is by design (a reopening retires the old lenses), harmless in production (one redundant codeLens request while the document is still opening, never a wrong or stuck row, no timeout or refresh involved; unrelated to the T1120 semantic-tokens retry), so this is a test-timing artefact: the test's `setup` assumes the document's identity is active before the first draw, and under load it is not.

Fix the test, not the assertions. Either make `setup` (or the test) wait until the buffer's identity is active (e.g. poll `active_identity` through the manager before the first `s.draw()`/`until_ticking`), or, if that is not reachable from the test, make `sync_lenses` not send while `active_identity` is None (leave `slot.key` unset so the ask is made once the identity is active) and keep the one-request assertion. Prefer the test-side wait; touch production only if the wait is not expressible.

## Implementation notes

A test-side wait could not work: `open` and `until` draw before any check, and a draw runs `sync_lenses`. So `sync_lenses` now does not ask while the buffer has no active identity (it leaves `slot.key` unset, and asks once the identity turns active). The test is unchanged.
