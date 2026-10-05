---
task_uid: "01m44sf9p2exp6barm6d29dyw9"
task: "T56"
rev: 3
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "Injection matching checks the deadline before querying each split query root; once the deadline has passed it marks the result incomplete and runs no further root queries"
    required: true
    manual: false
  - id: "C2"
    text: "Once MAX_INJECTIONS matches are found, no further split roots are queried"
    required: true
    manual: false
  - id: "C3"
    text: "The fix stays in injection.rs (e.g. a stop flag checked at the top of the per-root closure); for_each_query_root in syntax.rs is not changed"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in crates/fe-syntax/src/injection/root_traversal_tests.rs cover a buffer of many small split roots (e.g. many short JavaScript statements): an already-expired deadline yields an incomplete result with no injections, and reaching the cap before the final root stops querying later roots, asserted by counts or results, not wall-clock time"
    required: true
    manual: false
---

# Follow-ups from review of T49

Findings of the review of T49 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/injection.rs:391: The match cap stops only the current iterator; for_each_query_root continues visiting every remaining split root. Deadline cancellation likewise does not stop that outer traversal. Tree-sitter resets its callback counter for each root and checks it every 100 operations, so many short JavaScript statements can be traversed without any deadline checks, even after expiry. Once MAX_INJECTIONS is reached, the remaining roots are still walked without checks. This leaves input-sized work outside the intended bounds.
  fix: Make query-root traversal stoppable and propagate a stop through its recursive visitor. Check the deadline before querying each root, mark expiry incomplete, and stop the entire traversal when the match cap is reached. Add regression coverage for many small split roots, including expired deadlines and reaching the cap before the final root.
  instances: crates/fe-syntax/src/injection.rs:391
