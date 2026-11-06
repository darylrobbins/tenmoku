---
task_uid: "01m6qajtkpe7wh2ddkrvdwha62"
task: "T466"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T460

Findings of the review of T460 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106a), for helium batch 105:

- F1 medium crates/fe-workbench/src/spell_ranges.rs:570: Every string fragment now clones and sorts every exclusion from all open literals, including exclusions far outside that fragment. A PHP JavaScript heredoc containing N short strings with escapes has O(N) host exclusions and O(N) fragments, so this introduces quadratic work. Whole-token expansion can process the entire heredoc from a single visible row, and this range calculation runs on the UI thread, causing substantial stalls for files below the size limit.
  fix: Restrict each literal's exclusions to those overlapping the current fragment before merging, or maintain a reusable union with cursors as spans advance. Add coverage demonstrating bounded work for a large heredoc containing many injected strings and host exclusions.
  instances: crates/fe-workbench/src/spell_ranges.rs:570
