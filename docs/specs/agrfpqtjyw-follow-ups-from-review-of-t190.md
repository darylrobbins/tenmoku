---
task_uid: "01m4dvykv55kqnwfagrfpqtjyw"
task: "T192"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T190

Findings of the review of T190 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/imports.rs:232: The replacement only recognizes `extends` followed by ASCII whitespace and `=`. It misses affected forms such as `<Foo extends foo="/" />` and `<Foo extends\u00a0="/" />`, as well as grammar-supported intervening comments. These produce the same type-parameter/regex misparse, so removing the previous tree-based recovery silently loses subsequent imports. The grammar accepts these forms after respelling the attribute.
  fix: Identify affected JSX attributes without requiring this restricted suffix, while preserving genuine TypeScript constraints and bounded total work. Add regressions for boolean attributes followed by other attributes, Unicode whitespace, and intervening comments, each with a final real import.
  instances: crates/fe-syntax/src/imports.rs:232

- F2 medium crates/fe-syntax/src/imports.rs:227: Cancellation is polled only after finding an `extends` occurrence. Searching a large prefix or suffix without that word, or scanning a long whitespace run after it, performs unbounded work between polls. Cancellation therefore cannot interrupt these recovery scans; the new test only covers densely repeated occurrences and misses this case.
  fix: Scan in bounded chunks or use a single forward scanner that polls cancellation after a bounded number of bytes, including during whitespace lookahead. Add cancellation coverage for large gaps between occurrences and long whitespace runs.
  instances: crates/fe-syntax/src/imports.rs:227, crates/fe-syntax/src/imports.rs:232
