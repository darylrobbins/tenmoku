---
task_uid: "01m5xdb4sh4h4mtekb887a8dnb"
task: "T368"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The proposal_demo example builds and prints a dry run for each separate buffer notification; edits batched into one notification print a superseded message for earlier versions (Known limit F1: needs an fe-buffer snapshot on BufferEvent::Edited)"
    required: true
    manual: false
---

# Follow-ups from review of T54

Findings of the review of T54 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/examples/proposal_demo.rs:121: Observing the view instead of polling still loses intermediate dry runs: when multiple edits precede one notification, only the final version receives a dry run; earlier events print a superseded message. This leaves the requirement to print a dry run after every buffer change unmet. The recorded typing check exercises separate notifications, not the batching case.
  fix: Capture and process each edit's corresponding state so every buffer change produces its own dry run, and verify multiple edits before one observer notification.
  instances: crates/fe-gpui/examples/proposal_demo.rs:121


Known limit (F1): the example can't give every batched edit its own dry run, because BufferEvent::Edited carries only the post-batch Text and BufferSnapshot can't be built or rewound outside fe-buffer. Edits that arrive before one notification print a "superseded" message for the earlier versions. Lifting it needs an fe-buffer change (snapshot on BufferEvent::Edited), tracked as a separate card.
