---
task_uid: "01m4rbvdwtkwxh78bxsfc7ja25"
task: "T85"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "larger_node: in Rust 'let x = foo(a, b + c);' a caret in b gives b, then b + c, then the argument list, then the call, then the let declaration, and so on to the root, each call strictly larger"
    required: true
    manual: false
  - id: "C2"
    text: "larger_node: a caret just after an identifier (before '.' or ')') gives the identifier, and a caret between two word leaves gives the one starting there"
    required: true
    manual: false
  - id: "C3"
    text: "larger_node: in JSON and Python samples a selection equal to a node gives its smallest strictly larger named ancestor; a MISSING node from error recovery is never returned; None without a tree and for the root's whole range"
    required: true
    manual: false
---

# Find the syntax node enclosing a selection in fe-syntax for expand selection: larger_syntax_node grows a range to the next named node (Mac T490)

## Problem

Users browse code more than they edit it (~/.claude/plans/glittery-foraging-graham.md, Context), and
selecting a whole expression, argument list, block or item to copy or read it is one of the commonest
browse actions. Today the only selection growth is by word (`EditorModel::word_range_at`,
crates/fe-editor/src/lib.rs:692), vim text objects, or select-all. fe-syntax already answers structural
questions from the tree for the editor (bracket pairs, crates/fe-syntax/src/brackets.rs; folds,
crates/fe-syntax/src/fold.rs), but nothing gives the enclosing syntax node of a selection.

## Change

Add `larger_syntax_node(syntax: &Syntax, range: Range<usize>) -> Option<Range<usize>>` in a new
crates/fe-syntax/src/select.rs, exported from crates/fe-syntax/src/lib.rs. The editor commands
(expand and shrink selection, with the shrink history) and keys are a later Mac card in fe-gpui; this card is
fe-syntax only.

- Non-empty `range`: the byte range of the smallest named node of the root tree that contains `range` and is
  strictly larger than it. With none, the root's range if that is larger, else `None`.
- Empty `range` at `offset`: first the leaf touching `offset` whose text is all word chars (alphanumeric or
  `_`; an identifier, keyword or number), the one starting at `offset` before the one ending there; else the
  smallest named node containing `offset`. Feeding each result back in as `range` therefore grows the
  selection every call until the root.
- Zero-width `MISSING` nodes of error recovery are skipped; `ERROR` nodes count as named nodes.
- Descend with one `TreeCursor` as `bracket_pair` does (brackets.rs:47), not `Node::parent` or
  `descendant_for_byte_range`, so a node with many children costs one pass.
- `None` without a tree. Known limit, as `bracket_pair` documents: injected layers are not looked at.

Tests go in a new crates/fe-syntax/tests/larger_node.rs.
