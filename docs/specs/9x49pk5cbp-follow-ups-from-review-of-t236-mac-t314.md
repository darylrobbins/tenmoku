---
task_uid: "01m4kxpvsj1nxmdx9x49pk5cbp"
task: "T20"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T236 (Mac T314)

Findings of the review of T236 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp.rs:632: Changing the owner's language to another loaded language releases ownership but immediately reopens the same buffer and reclaims the path. The waiting alias cannot acquire it, regardless of iteration order, and handed_over is false. This misses the specified handoff on language changes; changing the owner to a language without a configured server also leaves the remaining Rust alias without LSP service.
  fix: When a language change releases an owner and another eligible alias is shown, give that alias ownership at the next frame rather than immediately reacquiring the path for the former owner. Add a regression test covering a change to another loaded language.
  instances: crates/fe-workbench/src/lsp.rs:632



Moved from the Mac board: Mac T314. Its dependencies are finalized on Mac and arrive here with the mac/main sync.

## Implementation notes

F1 was already fixed on the base by T315 (1718200): `sync_lsp` reserves a released path for a waiting alias a server handles, and `tests/lsp_aliases_language.rs` covers a change to another loaded language and to one without a server. Both pass; no code change was needed.
