---
task_uid: "01m7z28b4fy9we5m73xrwhdmee"
task: "T737"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T499

Findings of the review of T499 that don't block the merge (below merge.block_severity):

- F1 medium extensions/grammars/groovy/test/sample.groovy:36: Adding `return` avoids the previously failing arithmetic case instead of fixing its highlighting. The function-capture query is unchanged, so ordinary implicit-return `x * x` remains incorrectly highlighted, and the golden no longer exercises that case.
  fix: Restore the implicit-return arithmetic sample, adapt the query so both operands are variables while preserving actual function-call highlighting, and regenerate the golden.
  instances: extensions/grammars/groovy/test/sample.groovy:36, extensions/grammars/groovy/highlights.scm:280, extensions/grammars/groovy/test/sample.groovy.golden:104
