---
task_uid: "01m4kxpvpnzt7rnwe1j0nhesxv"
task: "T17"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T256 (Mac T284)

Findings of the review of T256 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-lsp/src/tests/registry.rs:686: The assertion still confuses receiving an `initialize` request with completing initialization. The fake records the request before answering it; if the next settings update supersedes that launch before the client consumes its reply, Actor::initialize returns Started::Stopped and kills it without sending `exit`. Both adjacent launches can therefore record `initialize` while the predecessor legitimately has no `exit`, leaving another load-dependent failure. The helper documentation and implementation notes repeat this incorrect assumption.
  fix: Require the predecessor's completed handshake, using its `initialized` notification, before demanding its `exit`; retain the successor's `initialize` as the ordering endpoint. Add deterministic coverage for superseding a launch after its initialize request is recorded but before its reply completes, and correct the documentation and notes.
  instances: crates/fe-lsp/src/tests/registry.rs:674, crates/fe-lsp/src/tests/registry.rs:686, docs/specs/x65mvdr0dw-fe-lsp-flake-tests-registry.md:43



Moved from the Mac board: Mac T284. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
