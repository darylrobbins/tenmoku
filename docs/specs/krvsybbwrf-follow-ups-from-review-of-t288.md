---
task_uid: "01m4kcax906c12gzkrvsybbwrf"
task: "T4"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T288

Findings of the review of T288 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-nav/src/references.rs:868: Legacy-encoded hits repeatedly decode entire line prefixes: place does so twice per hit, and in_preview performs three more lossy prefix decodes. A supported multi-megabyte line with thousands of hits therefore processes hundreds of gigabytes and repeatedly allocates large strings, even for hits outside the preview window. This replaces the previous inexpensive offset lookups with O(line length × hit count) work and can stall reference searches.
  fix: Build reusable offset mappings or incrementally process the sorted hit boundaries once per line. Compute preview offsets only within its bounded window and skip hits outside that window. Add a regression that checks bounded decoding work for a long legacy-encoded line with many hits.
  instances: crates/fe-nav/src/references.rs:868, crates/fe-nav/src/references.rs:898



_Moved from the Mac board (T304) to run on neon; it lands back on main through the lead._
