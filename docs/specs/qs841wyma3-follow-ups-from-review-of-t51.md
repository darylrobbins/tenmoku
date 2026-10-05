---
task_uid: "01m44ta63fxkpcbwqs841wyma3"
task: "T57"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "Candidates::of finds a Body's structural header without repeated Node::prev_sibling calls (one forward pass over the parent's children, or a header carried from the traversal), so header lookup is linear in the comment prefix length"
    required: true
    manual: false
  - id: "C2"
    text: "Fold results are unchanged: existing fold tests and lookup_tests pass"
    required: true
    manual: false
  - id: "C3"
    text: "New tests in crates/fe-syntax/src/fold/large_prefix_tests.rs cover a large Python and Lua comment prefix (at least 10,000 comments) before a body, through both header-only extraction and caret lookup, asserting the expected fold ranges; no wall-clock thresholds"
    required: true
    manual: false
---

# Follow-ups from review of T51

Findings of the review of T51 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/fold.rs:608: Unlimited lookahead still performs quadratic header lookup. After scanning N comments with the cursor, this call enters `Candidates::of`, whose repeated `Node::prev_sibling` calls at lines 617–618 each rescan preceding siblings. Both header-only extraction and caret lookup now reach this O(N²) path for arbitrarily large Python/Lua comment prefixes; the former cutoff avoided it. Large prefixes below the parse-size limit can stall the synchronous Fold command, and the 20-comment fixture does not expose this.
  fix: Find the structural header in one forward traversal of the parent's children, or retain it during traversal, and reuse it when computing the body candidate. Add a large-prefix scaling regression or benchmark covering both candidate APIs.
  instances: crates/fe-syntax/src/fold.rs:608, crates/fe-syntax/src/fold.rs:560, crates/fe-syntax/src/fold.rs:572

## Implementation notes

- `Candidates::of` finds a body's header with `header`, one cursor pass
  forward over the parent's children, instead of repeated
  `Node::prev_sibling`. With 10,000 comments before the body, folding from
  the header row took about 1 s in a debug build before, about 1 ms after.
- tree-sitter-python's own parse is quadratic in the comments between a `:`
  and its block (about 2 s for 10,000 in a debug build, more under load), so
  `large_prefix_tests` parses without the time cap to stay deterministic.
  That parse runs off the UI thread and is upstream behaviour, not fold's.
