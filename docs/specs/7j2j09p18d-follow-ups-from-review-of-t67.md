---
task_uid: "01m4q7997fvjxr777j2j09p18d"
task: "T69"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T67

Findings of the review of T67 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp.rs:150: user_layer() locks the shared user layer on the UI thread when starting a manager. Background reads hold this mutex throughout filesystem access and validation. Opening another window or replacing a root during a stalled user-file read therefore freezes the UI, reintroducing the blocking problem that T67 is intended to eliminate.
  fix: Store the immutable user path alongside the shared layer so the UI can compare paths without locking read state. Add a regression check for manager creation while a user-layer read is blocked.
  instances: crates/fe-workbench/src/lsp.rs:150

- F2 medium crates/fe-workbench/src/lsp.rs:856: The retained user layer is read only for trusted roots. While every window is untrusted, valid user-file loads and reloads never populate or update it. For example, load lsp.enabled=false while untrusted, corrupt that file, then trust the root: the new layer rejects the broken file and applies defaults, enabling servers despite the previously validated disabling setting. A previously populated layer can likewise retain an obsolete enabling value after a disabling edit made while untrusted. The added test keeps the root trusted during the valid edit, so it misses both cases.
  fix: Initialize and maintain the app's validated user layer independently of root trust, including user-file reloads while all windows are untrusted. Test a valid disabling setting followed by corruption and first trust, plus a disabling edit during an untrusted interval, using secondary-window settings delivery.
  instances: crates/fe-workbench/src/lsp.rs:154, crates/fe-workbench/src/lsp.rs:856
