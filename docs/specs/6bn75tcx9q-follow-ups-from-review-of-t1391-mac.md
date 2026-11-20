---
task_uid: "01m7wtg1wx8qhaak6bn75tcx9q"
task: "T393"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1391 (Mac T2036)

Findings of the review of T1391 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-syntax/queries/cpp-injections.scm:13: The concatenated_string alternatives capture only ordinary string_literal children. The C++ grammar also allows raw_string_literal children, so printf(R"(%d)" " suffix", n) leaves %d uncoloured. This affects all three format-argument positions and violates the requirement to inject each concatenated piece.
  fix: Capture raw_string_content inside raw_string_literal children of concatenated_string in all three patterns. Test mixed ordinary/raw and all-raw concatenations, preserving independent scanning of each piece.
  instances: crates/fe-syntax/queries/cpp-injections.scm:13, crates/fe-syntax/queries/cpp-injections.scm:28, crates/fe-syntax/queries/cpp-injections.scm:45
