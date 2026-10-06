---
task_uid: "01m490bzq4x84ntnmhssj0xc2z"
task: "T118"
rev: 4
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F1: a FinderIndex created for a relative root that doesn't exist yet, finished, then given the directory with a file in it and a Rescan, indexes that file; the same holds for a missing root reached through a symlinked ancestor, and a root replaced by a symlink after construction is still not followed on rescan"
    required: true
    manual: false
  - id: "C2"
    text: "New tests live in crates/fe-project/src/tests/finder_followups.rs and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T91

Findings of the review of T91 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-project/src/finder.rs:649: Rescan no longer recovers a root whose stored path could not be canonicalized during construction. For example, create FinderIndex::new("missing-project") while the directory is absent, finish it, create missing-project/a.rs, then apply Rescan: Start::root rejects the retained relative path, leaving the index empty. The base walker indexed a.rs. Absolute fallback paths containing symlinked ancestors have the same problem.
  fix: During construction, preserve an absolute root with existing ancestors resolved once, including when the final directory is missing; keep later rescans no-follow. Add regression tests for missing relative roots and missing roots reached through symlinked ancestors.
  instances: crates/fe-project/src/finder.rs:649


Triage (2026-10-06, main a7975bd): F1 confirmed still present (FinderIndex::new falls back to the given path when canonicalize fails; rescan's Start::root then rejects a relative or symlinked-ancestor root). Kept. Scope narrowed to finder.rs plus a new test file; the bench and PERF.md aren't touched by this fix.
