---
task_uid: "01m6kndec3nag9mw5rt0tcjdz3"
task: "T1381"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1028

Findings of the review of T1028 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-syntax/src/outline.rs:452: Range-bounded work remains unresolved: the overlap filter runs after matches.next() has enumerated each match. Contextual patterns rooted at enclosing containers still enumerate unrelated siblings in Rust, Python, Ruby, Swift, Kotlin, Scala, Go and Lua. Consequently, repeated one-row scroll queries scale with container or document size. The new large-impl test checks retained results, not query work; Markdown caching and the C++ predicate bypass resolve only the other portions of F2.
  fix: Use item-rooted scope queries with ancestor predicates, or another bounded traversal, for all contextual patterns. Add checks that measure raw matches or traversal work as unrelated sibling counts increase, rather than only counting retained items.
  instances: crates/fe-syntax/src/outline.rs:441, crates/fe-syntax/src/outline.rs:452
