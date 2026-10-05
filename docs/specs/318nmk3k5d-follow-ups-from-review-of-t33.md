---
task_uid: "01m42554adhkke7h318nmk3k5d"
task: "T46"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/markdown-reparse.md no longer presents the loaded random-edit benchmark as a worst-case bound (review F1), and its Status line records that the user approved the 'accept ~6–8 ms for now' decision on 2026-10-03"
    required: true
    manual: false
---

# Follow-ups from review of T33

Findings of the review of T33 that don't block the merge (below merge.block_severity):

- F1 medium docs/decisions/markdown-reparse.md:130: The record treats the loaded-machine, random-edit benchmark as a worst-case bound of ~0.4 s. The benchmark averages random edit positions in one generated corpus; neither the byte limit nor Criterion's confidence interval bounds latency across positions, document structures, or machine load. This overstates the evidence supporting acceptance, especially since the record establishes that edits near the start cost more.
  fix: Describe ~0.4 s as the measured random-edit estimate for this near-limit corpus, remove the upper-bound and worst-case latency claims, and explicitly state that worst-case latency remains unmeasured; alternatively measure representative adverse cases and report their limits accurately.
  instances: docs/decisions/markdown-reparse.md:23, docs/decisions/markdown-reparse.md:32, docs/decisions/markdown-reparse.md:130
