---
task_uid: "01m43rw34w36t8prksksh5zfwd"
task: "T51"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T30

Findings of the review of T30 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-syntax/src/fold.rs:579: The header correction makes lookup fail inside intervening comments. For both new Python/Lua comment fixtures, fold_range_at(syntax, 1) returns None instead of 0..=2: the body's corrected header is row 0, and boundary lookahead rejects it because row 0 is outside rows. Lua's unindented fixture also has no indentation fallback.
  fix: Let boundary lookahead honor the candidate starts domain, which already permits enclosing headers during fold_range_at. Test lookup on every intervening comment row.
  instances: crates/fe-syntax/src/fold.rs:579

- F4 medium crates/fe-syntax/src/fold.rs:576: MAX_EXTRAS silently limits valid header lookup to 16 comments. A Python or Lua function with 17 leading comment nodes followed by x() produces 0..=18 during full extraction, but header-only extraction returns nothing and fold_range_at(syntax, 0) finds no candidate. Fold and Fold All therefore disagree.
  fix: Remove the arbitrary comment-count cutoff or locate the structural body without losing candidates; add full-file, header-only, and lookup regressions beyond 16 extras.
  instances: crates/fe-syntax/src/fold.rs:84, crates/fe-syntax/src/fold.rs:576

- F3 medium crates/fe-syntax/src/fold.rs:674: Opaque closing-row detection still misses delimiter-only rows with sufficiently long trailing whitespace. Ruby's opaque comment token includes trailing spaces after =end; for '=begin\na\nb\n=end' followed by 13 spaces, the 16-character limit rejects the closer and returns 0..=3 instead of 0..=2, hiding the closing row.
  fix: Match the delimiter and then stream-check the remaining characters for whitespace without a total row-length limit. Add regressions for trailing spaces and tabs through both candidate APIs.
  instances: crates/fe-syntax/src/fold.rs:674

- F5 medium crates/fe-syntax/src/fold.rs:561: An existing traversal edge case loses enclosing bodies when the caret is on a blank row between the header and first statement. For 'function f()\n\nx()\nend', full extraction finds 0..=2, but lookup at row 1 finds nothing: the first child selected is the body beyond rows, and this branch ascends without checking its candidate.
  fix: Apply the body-boundary candidate check when the first selected child starts beyond rows, and test blank rows before bodies.
  instances: crates/fe-syntax/src/fold.rs:561

## Implementation notes

- One helper, `Candidates::after`, handles the node the walk finds just past `rows`. It steps over
  comments (extras) with a cursor and returns that node's candidate. Callers rely on `of`'s `starts`
  filter rather than `rows`. `fold_range_at` therefore finds a body from rows inside its header's comments
  (F2), and from blank rows before it (F5). The F5 case is the first child past `rows`, which now gets
  the same check before the walk ascends.
- `MAX_EXTRAS` is gone (F4). Stepping a cursor over the comments is linear, as
  `goto_first_child_for_point` already is over a node's children, so the walk's cost class doesn't
  change. `Node::next_sibling` would have been quadratic.
- A childless comment's closing row is its first word, which must be a closer (at most 5 chars are
  collected), then whitespace of any length (F3).
- Tests: `fold/lookup_tests.rs`.
