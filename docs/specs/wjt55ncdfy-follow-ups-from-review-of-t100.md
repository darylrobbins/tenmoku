---
task_uid: "01m494xsjzcfdbzwwjt55ncdfy"
task: "T130"
rev: 2
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F2: docs/decisions/lsp-client.md §4.1 rejects unversioned publishDiagnostics unless a named, verified server-specific guarantee fixes their snapshot (including across reopen); the UI validates Diagnostics/DiagnosticsCleared against the current active (session, incarnation) set by lifecycle changes, and close/session end retire it; the §1 summary, the §7 event shape (with session and incarnation) and §10's diagnostics-versioning entry agree; §4.1's fake-server test list names the three tests: delayed unversioned publication across reopen dropped, old queued event before any new diagnostic rejected, Diagnostics after DiagnosticsCleared for the same identity rejected. §9 is unchanged (git diff shows no hunk under '## 9.')."
    required: true
    manual: false
---

# Follow-ups from review of T100

Follow-up from the review of T100 (docs/decisions/lsp-client.md). Doc-only: no code.

Do NOT edit §9 (Questions for the user) or decide any of Q1–Q7; those are with the user. Keep the edits to the finding below.

## F2 (medium): diagnostics lack proven snapshot and lifecycle identity

Still true on main (§1 summary ~l.120, §4.1 rules 4–5 and "Delivery to the UI" ~l.989–1040, §7 table ~l.1500, §10 ~l.1674):

1. After close/reopen, an unversioned publication computed during the earlier incarnation can arrive before the new incarnation's first didChange; §4.1 rule 4 accepts it against the new text. LSP 3.17 publishDiagnostics has neither a required version nor an incarnation id.
2. At UI delivery, comparing only against previously accepted `(session, incarnation)` accepts an old queued event before any newer identity has been accepted.
3. `DiagnosticsCleared` does not retire its identity, so equal-identity `Diagnostics` stay eligible after it.
4. §7's table still shows `LspEvent::Diagnostics { buffer, version, server, items }` without session/incarnation.

Fix:
- Reject unversioned publications unless a verified server-specific guarantee establishes their snapshot, including across reopen.
- The UI validates events against the current active session/incarnation established by lifecycle changes (open/close/restart), independently of queued diagnostic delivery; close and session end retire the identity, and later events for it are rejected.
- Update the §1 summary, the §7 event shape and the §10 "Which servers version their diagnostics" entry to match.
- Add to §4.1's fake-server test list: a delayed unversioned publication across reopen (dropped); an old queued event before any new diagnostic (rejected); a `Diagnostics` delivered after `DiagnosticsCleared` for the same identity (rejected).
