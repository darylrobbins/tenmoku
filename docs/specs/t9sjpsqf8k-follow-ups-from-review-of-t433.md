---
task_uid: "01m6pw52xcjpb1w3t9sjpsqf8k"
task: "T462"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T433

Findings of the review of T433 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-05):

- F1 medium crates/fe-plugin/src/formatting.rs:120: Handle binding still identifies the originating load by plugin name. If load A's binding completes after load B attaches, bind stamps A's handle with B's load number and overwrites B's handle. Requests then execute on A, and A's timeout or Unloaded result removes B's registrations because failure cleanup sees B's stamped identity. The load check in request cannot detect this misbinding.
  fix: Bind through an attachment token captured when State is created, and reject binding if that originating attachment is no longer current. Add a regression test that delays A's bind until after B attaches and binds, then verifies B still handles requests and survives A's failure.
  instances: crates/fe-plugin/src/formatting.rs:118
