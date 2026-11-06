---
task_uid: "01m6pw5261f0wjs7dqy0pgz9ek"
task: "T460"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T430

Findings of the review of T430 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-05):

- F1 medium crates/fe-workbench/src/spell_ranges.rs:214: Selecting only the innermost literal discards the host literal's interpolation exclusions. Combined injected trees can have string nodes spanning gaps in their included ranges. For example, in javascript`const a = "hello ${lookup("codeword")} world";`, the injected JavaScript string spans the host interpolation, so the host-highlighted "codeword" is checked using the injected literal instead of being excluded. This regresses the existing rule that strings inside interpolations are code.
  fix: Preserve host interpolation exclusions when applying injected literals, and respect each layer's included ranges. Add an integration regression test with an injected string spanning an interpolation containing another string.
  instances: crates/fe-workbench/src/spell_ranges.rs:214
