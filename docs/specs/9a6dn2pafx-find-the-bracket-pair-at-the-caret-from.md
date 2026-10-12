---
task_uid: "01m4pzqvp6d2fbbf9a6dn2pafx"
task: "T23"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax test brackets::rust_pairs: in fn f(a: [u8; 2]) { g(\"(\") }, offsets at f's (, after ], at { and after } give their pairs; the ( inside the string literal and an offset not at a bracket give None; at )( the ( is paired"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test brackets::recovery_and_other_languages: a Python call missing its ) gives None for that ( and still pairs a complete inner pair; JSON [ ] and { } and JavaScript ( ) pair"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test brackets::wide_parent_is_linear: pairing the [ of a JSON array of 200,000 numbers takes less than a twentieth of the time its full parse takes, measured in the same test"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-syntax, cargo clippy -p fe-syntax --all-targets -- -D warnings and scripts/check-syntax-features.sh pass"
    required: true
    manual: false
---

# Find the bracket pair at the caret from the syntax tree in fe-syntax, skipping brackets in strings, comments and error recovery (Mac T457)

Filed on helium from Mac T457 (~/coordination/helium-cards/T457.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-i.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

Nothing finds the bracket that matches the one at the caret, so the editor can't show the pair, a staple of reading
nested code (the read path is the plan's first priority, line 4; M6 is "Editing polish", line 110). fe-vim's `%`
works on the text alone, without the tree. The syntax tree already knows the pairing: in every grammar fe-syntax
links, `(`/`)`, `[`/`]` and `{`/`}` are anonymous leaf nodes that are children of the same node, and brackets inside
strings and comments are not leaves at all.

## Change

New module `crates/fe-syntax/src/brackets.rs` (`mod brackets;` and
`pub use brackets::{BracketPair, bracket_pair};` in lib.rs):

- `#[derive(Clone, Debug, PartialEq, Eq)] pub struct BracketPair { pub open: Range<usize>, pub close: Range<usize> }`.
- `pub fn bracket_pair(syntax: &Syntax, offset: usize) -> Option<BracketPair>`, on the root tree
  (`Syntax::tree`): the bracket leaf starting at `offset` is tried first, then the one ending at `offset` (so a caret
  between `)(` pairs the `(`, as nvim's matchparen does with the cursor on it). Its counterpart is the matching leaf
  among the same parent's children, counting nesting of that bracket kind between them.
- `None` when no bracket leaf is at `offset`, when the counterpart is missing or is a zero-width `MISSING` node
  (error recovery), or when either is inside an `ERROR` node.
- Work is linear in the parent's child count at most (a `TreeCursor` walk over siblings, no per-child
  `Node::child(i)` lookups, which would make it quadratic).

Known limit, stated in the doc comment: injected layers (Markdown code fences, HTML `<script>`) are not looked at.

## Tests

New file `crates/fe-syntax/tests/brackets.rs`.
