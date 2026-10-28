---
task_uid: "01m5wecfknjkp9x2xeg8mt72b5"
task: "T249"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Follow-ups from review of T81

Findings of the review of T81 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-lsp/src/formatting.rs:67: Unknown capabilities are treated as an absent provider. Before initialize completes, and during restart, capabilities() returns None, so both document and range formatting immediately return Ok(None), even for a server that supports formatting. This incorrectly reports 'no formatter' and silently skips formatting on an early save. The tests hide this case by waiting for initialization before opening the buffer.
  fix: Distinguish unknown capabilities from a confirmed absent provider. Check support after initialization in the server actor, retaining the three-second timeout, or return a retryable availability error while capabilities are unknown. Add startup and restart coverage for both formatting methods.
  instances: crates/fe-lsp/src/formatting.rs:67
