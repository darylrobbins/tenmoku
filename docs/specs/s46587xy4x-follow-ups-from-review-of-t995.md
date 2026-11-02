---
task_uid: "01m6cffkv4x9f8ecs46587xy4x"
task: "T1302"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Follow-ups from review of T995

Findings of the review of T995 that don't block the merge (below merge.block_severity):

- F1 medium docs/decisions/5at38c8v89-ai-chat.md:361: § 9 still says the capability strings may change with T839's sign-off and the grants card follows it. This leaves card 2 coupled to a future T839 decision, contradicting the accepted answer that it follows this record as written without waiting for T839's sign-off.
  fix: Replace the remaining provisional instruction with an explicit statement that card 2 implements this record's accepted reconciliation and current strings without waiting for T839's sign-off.
  instances: docs/decisions/5at38c8v89-ai-chat.md:361
