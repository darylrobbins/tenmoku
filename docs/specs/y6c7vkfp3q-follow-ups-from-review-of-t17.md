---
task_uid: "01m4pz0jygnv7jt9y6c7vkfp3q"
task: "T31"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T17

Findings of the review of T17 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:740: The displacement uses summed character widths rather than the inlay's shaped width, so the reported invisible-caret failure remains possible with contextual shaping. For example, use 30,000 buffer characters, inlay offset 40, head 60, and 8,200 W characters measured individually at 10px but shaped at 20px each. The three attempts at scroll 327.5, 164327.5, and 560 all leave the caret invisible: the second attempt subtracts only 82,000px, and its shifted slice still cannot extend to the inlay within the byte limit. The unchanged unwrap_or(0) then selects a known invisible layout.
  fix: Reconcile the slice using the inlay displacement measured in the shaped row, retaining that measurement across scroll attempts as needed. Ensure completion does not select a known invisible candidate, and add a wide-inlay regression where shaped width exceeds summed character widths.
  instances: crates/fe-display/src/layout.rs:740
