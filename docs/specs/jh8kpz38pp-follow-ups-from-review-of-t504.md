---
task_uid: "01m7z4932han0v92jh8kpz38pp"
task: "T738"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T504

Findings of the review of T504 that don't block the merge (below merge.block_severity):

- F3 medium extensions/grammars/scss/highlights.scm:55: The pseudo-element pattern captures both the base tag and the pseudo-element name. For a::after, the golden consequently colours a as attribute instead of tag. This affects ordinary tag selectors followed by pseudo-elements.
  fix: Constrain the capture to the tag_name immediately following "::", then regenerate and inspect the golden so a remains tag and after remains attribute.
  instances: extensions/grammars/scss/highlights.scm:55, extensions/grammars/scss/test/sample.scss.golden:282
