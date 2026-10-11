---
task_uid: "01m4kxpvjwxev6etz4apchnxy2"
task: "T13"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F4 is fixed in docs/decisions/app-shell.md: restored window entries get exclusive ownership (a cross-process claim held for the window's lifetime, or a fresh ID when an entry already has a live owner), applied to folder launches, new windows, writes and deletion, and S11 requires a two-process same-root restore/write/close test"
    required: true
    manual: false
  - id: "C2"
    text: "F5 is fixed in docs/decisions/app-shell.md: a close transaction either prevents relevant mutations while confirmation is pending or revalidates membership and close-relevant state before committing, overlapping close requests are serialized, and S8 requires tests for editing an already-approved window and for opening an item while another window's Ask is pending"
    required: true
    manual: false
  - id: "C3"
    text: "Only docs/decisions/app-shell.md changes, and its lines 1-8 (the status banner) are unchanged"
    required: true
    manual: false
---

# Follow-ups from review of T242 (Mac T275)

Findings of the review of T242 that don't block the merge (below merge.block_severity):

- F4 medium docs/decisions/app-shell.md:928: Restored window IDs can have multiple live owners across processes. Launching the same folder in two processes restores the same entries with the same IDs; subsequent writes replace the same entry, and closing one owner can delete the other's entry. Locking read-modify-write prevents interleaving but does not prevent this overwrite, contradicting the cross-process preservation guarantee.
  fix: Specify exclusive ownership of restored entries, using a cross-process claim held for the window's lifetime, or assign a fresh ID when an entry already has a live owner. Apply that policy to folder launches, new windows, writes and deletion, and require a two-process same-root restoration/write/close test in S11.
  instances: docs/decisions/app-shell.md:928, docs/decisions/app-shell.md:932, docs/decisions/app-shell.md:938, docs/decisions/app-shell.md:941, docs/decisions/app-shell.md:1008, docs/decisions/app-shell.md:1009, docs/decisions/app-shell.md:1089

- F5 medium docs/decisions/app-shell.md:697: The asynchronous confirmation contract commits earlier approvals without handling intervening changes. During quit, window A can answer Close while clean, then become dirty while window B's prompt is pending; accepting B leads directly to cx.quit() without asking about A's new changes. GPUI prompts are window-modal, so other windows remain usable. New items and overlapping close requests have the same stale-confirmation problem.
  fix: Define a close transaction that either prevents relevant mutations while confirmation is pending or revalidates item membership and close-relevant state before committing, restarting confirmation when necessary. Serialize overlapping close requests and add S8 tests for editing an already-approved window and opening an item while another window's Ask is pending.
  instances: docs/decisions/app-shell.md:697, docs/decisions/app-shell.md:719, docs/decisions/app-shell.md:720, docs/decisions/app-shell.md:729, docs/decisions/app-shell.md:1006



Moved from the Mac board: Mac T275. Its dependencies are finalized on Mac and arrive here with the mac/main sync.

Cleared by kiln (2026-10-10, ~/coordination/kiln-to-neon-lead.md): go ahead. Leave lines 1-8 (the status banner) untouched, since kiln's batch 2 rewrites it to mark the doc superseded by app-framework.md. Write the fixes in the doc's own terms (fe-shell, S-cards). F4's two-process restore/write/close test is required of S11 in the text, and kiln carries it as a criterion of its K5.10.
