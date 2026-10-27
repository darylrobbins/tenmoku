---
task_uid: "01m5wp9jc25swa5scb51mdvmmd"
task: "T364"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T40

Findings of the review of T40 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-syntax/queries/outline/make.scm:23: Punctuation-bearing Bash and Make definitions remain unresolved: word_at still selects only part of compile-rule, main.o, archive targets and hyphenated Bash functions, while lookup requires the complete indexed name. Deferring this work does not resolve the previous blocker.
  fix: Select complete Bash and Make symbol names, align fallback search boundaries, and test same-file and project lookup for punctuation-bearing functions, macros, variables and targets.
  instances: crates/fe-syntax/queries/outline/make.scm:8, crates/fe-syntax/queries/outline/make.scm:15, crates/fe-syntax/queries/outline/make.scm:19, crates/fe-syntax/queries/outline/make.scm:23, crates/fe-syntax/queries/definitions/make.scm:9, crates/fe-syntax/queries/outline/bash.scm:5, crates/fe-nav/src/search.rs:728

- F3 medium crates/fe-nav/src/extract.rs:203: Quoted SQL lookup is only partially fixed. The new "audit log" symbol is indexed as audit log, but word_at selects audit or log, so neither matches. Quoted columns parsed as literal nodes, including the fixture's "when", are also rejected by the string-highlight filter before lookup. The navigation test uses unquoted references and misses these failures.
  fix: Select and normalize complete quoted SQL identifiers consistently, distinguish identifier literals from strings during navigation, preserve source ranges, and test quoted declarations and references in same-file and project lookup.
  instances: crates/fe-nav/src/extract.rs:203, crates/fe-nav/src/search.rs:533, crates/fe-nav/src/search.rs:554, crates/fe-nav/src/search.rs:728, crates/fe-syntax/queries/outline/sql.scm:5, crates/fe-syntax/queries/outline/sql.scm:8, crates/fe-syntax/queries/outline/sql.scm:11, crates/fe-syntax/queries/outline/sql.scm:14, crates/fe-syntax/queries/outline/sql.scm:19, crates/fe-syntax/queries/definitions/sql.scm:8
