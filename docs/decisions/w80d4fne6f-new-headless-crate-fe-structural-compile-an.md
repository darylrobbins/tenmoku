# fe-structural: compiling ast-grep-style patterns (T708, structural search SS2)

`fe_structural::Pattern::compile(language, source)` turns a pattern such as
`foo($A, $$$REST)` into an owned tree for one language. Matching is a later
card. These are the choices the spec left open, or where the code departs
from its wording.

## Metavariables

- `$NAME`, `$$$NAME`, `$_`, `$$$` and `$$` (a literal `$`), NAME being
  `[A-Z_][A-Z0-9_]*`. A `$` that starts none of these stays literal text, as
  does a name that runs on into a lowercase letter (`$Abc`) or a
  non-ASCII character: ast-grep reads `$Abc` as no metavariable either.
- Any name that starts with `_` (`$_`, `$_FOO`) is anonymous, as in
  ast-grep: each occurrence is its own metavariable, numbered `_any0`,
  `_any1`, … in pattern order. The names are lowercase so no `$NAME` can
  spell one. `metavariables()` lists them with `anonymous: true`.
- One name used as both `$A` and `$$$A` is an error (`MixedMetavar`).

## Placeholders

- Identifier `__tsMV<tag>_<NAME>__` (multi `__tsMV<tag>M_<NAME>__`), then
  the same in double quotes, then the number `1`, per occurrence. `<tag>` is
  empty unless the pattern already holds `__tsMV`, else the first number
  `n` for which it holds no `__tsMV<n>`: no placeholder can equal text the
  user wrote (C6).
- A placeholder is accepted when the smallest node spanning its bytes
  starts and ends exactly there. For an identifier that is the leaf; for a
  string it is the string node, whose content is a child, so "exactly one
  leaf" in the spec is read as "exactly one node". That node is the
  wildcard, and its kind is recorded. Placeholders are found by their byte
  ranges in the substituted text, never by searching for their text.
- Choices start all-identifier. When an attempt fails, the first
  placeholder that isn't a node of its own moves to its next spelling; if
  all are nodes, the one nearest the first error does. Linear: at most
  2 × occurrences + 1 choices.

## Suffixes

Each choice is parsed as written, then with `\n` after it, then with `;`.
The spec's "no ERROR or MISSING node" otherwise rejects the criteria's own
examples:

- tree-sitter-rust's expression statements need a `;`: `foo(a, b)` alone
  has a MISSING `;`.
- tree-sitter-go's statements need a terminator: `fmt.Println(x)` without
  a newline has a MISSING terminator and parses as a type conversion.

A suffix is never part of the pattern span the root is chosen by, so
`$X.unwrap()` still gives `call_expression`. Trying the bare text first
keeps `fn f() {}` a `function_item` (with `;` it would be a file of two
items).

## Tree

`PatternNode` holds kind id, kind name, named flag, field name, leaf text
(as the pattern spells it, `$$` already `$`), children (named and anonymous,
extras included) and a wildcard marker. tree-sitter's `Node::kind` borrows
the tree, and fe-syntax exposes no `'static` grammar, so kind and field
names are `Arc<str>`, one per name per pattern. `Pattern` is `Send + Sync`
(a unit test checks).

`literals()` returns every non-wildcard leaf's text, punctuation included;
SS7 picks what is worth prefiltering on.

## Errors

`PatternError { kind, offset }`, the offset in the user's source:

- `Syntax`: the first ERROR or MISSING node, in tree order, of the bare-text
  parse of the first choice. Placeholder bytes map to their metavariable's
  start, copied bytes byte for byte, a position past the end to the end.
  An ERROR that wraps the whole pattern gives its start.
- `MetavarNotANode`: no error node, but a placeholder isn't one node.
- `Empty`, `MixedMetavar`, and `ParseFailed` when `parse_str` returns
  `None` (no grammar could be set, or a wasm deadline passed).
