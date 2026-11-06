---
task_uid: "01m6q17nrkd9nm1fe89ec0pxa6"
task: "T463"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T285

Findings of the review of T285 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-plugin/src/decorations.rs:158: Deletion tracking remains incomplete. In "abcdef", replacing 2..4 with "X" leaves an empty decoration originally at 3..3 alive: its span maps to 2..3 instead of collapsing. Replacing an entire decorated range or gutter line likewise retains its record. Also, an empty decoration initially at zero has no span forever: inserting "X" at zero moves it inside the text, but deleting 0..7 still retains it. These cases affect held and late sets, survive refresh, and continue consuming capacity; the late empty-decoration cases previously dropped correctly.
  fix: Track deletion and replacement through undo-aware edits rather than inferring survival solely from final span emptiness. Handle empty decorations that move from a boundary into the text, and apply the same deletion rules to late sets, reads, refresh, and capacity accounting. Add replacement and boundary-to-interior regressions.
  instances: crates/fe-plugin/src/decorations.rs:158, crates/fe-plugin/src/decorations.rs:252, crates/fe-plugin/src/decorations.rs:657, crates/fe-plugin/src/decorations.rs:679
