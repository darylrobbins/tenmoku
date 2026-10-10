---
task_uid: "01m4kmen9yap9fvn8zthpx1bre"
task: "T10"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T3

Findings of the review of T3 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-syntax/src/injection.rs:358: Restoring highlight_query removes the injected query's completion check. If the last injection times out, subsequent recursion does not always check the deadline: find returns immediately for languages without injections, such as TOML or Python, and paint returns immediately beyond MAX_DEPTH. Consequently, injection::paint can report complete and Syntax::highlights caches truncated injected highlights indefinitely. A Markdown fence containing TOML can reproduce this; the existing deadline test uses HTML, whose injection query supplies the later check.
  fix: Use highlight_query_finished for injected layers and combine its completion flag into self.complete, as before this revision. Add deterministic coverage for a timeout inside the final injected layer with no injection query and for the depth-limit case.
  instances: crates/fe-syntax/src/injection.rs:358
