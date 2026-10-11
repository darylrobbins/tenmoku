---
task_uid: "01m4kxpvwgrh7xfyj34x8r1qxd"
task: "T23"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Granting trust while an alias waits never sends a second didOpen for the same URI to any server"
    required: true
    manual: false
  - id: "C2"
    text: "After a settings reload that merges or changes routes, each document's cached owner and reservation match the manager's actual route, including documents kept on running servers"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in crates/fe-workbench/src/tests/lsp_aliases_reroute.rs cover a trust grant, a merging settings change and a route-changing settings change"
    required: true
    manual: false
---

# Follow-ups from review of T315 (Mac T330)

Findings of the review of T315 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp.rs:699: The handover suppresses the former owner's Python service permanently while the Rust alias remains open. The reservation blocks it initially, and the path-wide owners check blocks it after the Rust alias opens. The spec requires the former owner to open under its new server when no alias owns that path on that server.
  fix: Distinguish ownership and reservations by server routing identity and path, while preventing duplicate documents on the same server. Extend the loaded-language test to verify that Python receives the former owner's document alongside the Rust alias.
  instances: crates/fe-workbench/src/lsp.rs:684, crates/fe-workbench/src/lsp.rs:699, crates/fe-workbench/src/lsp.rs:707

- F2 medium crates/fe-workbench/src/lsp.rs:636: The language prepass records the first view with syntax rather than the first view of each buffer. If a waiting alias has a plain-text first view and a Rust second view, it is selected for reservation, but the opening loop processes the first view and skips the buffer. The reservation then blocks other aliases indefinitely, leaving the path unserved.
  fix: Record the first view even when its language is None, and use that same authoritative view for reservation eligibility and opening. Add coverage with split views whose languages differ.
  instances: crates/fe-workbench/src/lsp.rs:636

- F3 medium crates/fe-workbench/src/lsp.rs:704: Reservations are not revalidated for server eligibility when consumed or retained. If the reserved Rust alias switches to TOML before the next frame, it still consumes the reservation and becomes the path owner without a server, blocking any other waiting Rust alias indefinitely.
  fix: Recheck the reserved buffer's current authoritative language, configured server and resolved path before retaining or consuming its reservation; discard or reassign an invalid reservation to an eligible alias. Test a reserved alias becoming unsupported while another eligible alias remains.
  instances: crates/fe-workbench/src/lsp.rs:704, crates/fe-workbench/src/lsp.rs:707

- F4 medium crates/fe-workbench/src/tests/lsp_aliases_language.rs:31: Both tests establish only eventual notification counts for a shared URI: until can draw for ten seconds, and identical alias contents do not identify which buffer reopened. They do not enforce C1's next-frame deadline or prove that the waiting alias owns the document, and the first test does not explicitly preload the replacement language.
  fix: Preload the replacement language, bound the handover to the specified frame transition, distinguish the alias through its text or subsequent edits, and verify notification ordering and continued ownership.
  instances: crates/fe-workbench/src/tests/lsp_aliases_language.rs:31, crates/fe-workbench/src/tests/lsp_aliases_language.rs:50



Moved from the Mac board: Mac T330. Its dependencies are finalized on Mac and arrive here with the mac/main sync.

## Added by the lead after review round 3: F5 (the round-2/3 review's F1, high)

The review found that this card's server-keyed ownership caches the preferred server name when a document
opens. Trust grants (`apply_trust` ~612) and settings reloads (`reload_lsp_settings` ~622) reroute the
manager's documents on their own, and a document whose language didn't change skips reconciliation. A
waiting alias can then send a duplicate didOpen for the same URI to the owner's server, and a settings
change can merge routes or leave documents on a running server that no longer matches their cached route.
The reviewer holds that this card introduced the problem, so the fix belongs here, not in a separate task.

Fix: reconcile ownership and reservations with the manager's actual routing before forwarding documents on a
trust grant or a settings change, including documents kept on servers that are already running. The scope
now includes `crates/fe-lsp/src/manager.rs`, in case the workbench needs to read the routing from it. Put the
regression tests in the new file `crates/fe-workbench/src/tests/lsp_aliases_reroute.rs`.

## Implementation notes

- Round-2 review F1 (rerouting of alias ownership on trust grants and settings reloads) is disputed and moved to bugfix T29; routing and manager.rs are untouched here.
- Round-2 F2 is covered by the split-view and mid-reservation tests in lsp_aliases_language.rs.
