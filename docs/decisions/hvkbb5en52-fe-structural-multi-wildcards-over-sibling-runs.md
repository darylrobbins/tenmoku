# fe-structural: multi wildcards over sibling runs, and `where` constraints (T710, structural search SS4)

Choices the spec left open, or where the code departs from its wording.

## Multi wildcards (`multi.rs`)

- A child list holding a multi wildcard is matched by backtracking over the
  pattern's children (trivia skipped) against the node's children (extras
  skipped). A multi takes the fewest siblings it can, more only when the
  rest of the list fails; the first of two in a row is the lazier. A
  multi that is the list's last child takes everything left, and no run
  leaves fewer siblings than the fixed (non-multi) pattern children after
  it need.
- A run may hold anything, separators included (`foo($$$A, last)` on
  `foo(a, b, last)` gives `a, b`), but no MISSING node. The pattern's
  separators are siblings the file must have: `foo($$$A, last)` doesn't
  match `foo(last)`.
- Backtracking spans the whole pattern (review F1): the rest of the
  pattern is a continuation passed down into each list, so a list's split
  stands only if everything after it matches. `foo($$$A, $$$B) + bar($$$A)`
  matches `foo(x, y) + bar(x)` with A = `x`. A subtree with no multi
  wildcard has at most one way to match and is compared directly, with no
  continuation.
- Memo (review F2): a failed state is `(pattern list, file node, pattern
  child i, sibling j)` plus the ranges bound to the live names, those used
  both before and after the state in pattern order. Where the list's node
  sits in the file and the list sits in the pattern fixes the continuation,
  so the key is sound across lists. It is reset for each candidate.
- Bound (review F2): with no live name a candidate takes O(p·n²) steps (p
  pattern nodes, n siblings). Matching with several repeated variables is
  NP-hard, so compile refuses (`PatternErrorKind::VaryingRepeats`, at the
  second name's second use) a pattern with more than one repeated name
  whose first use can bind more than one way. That is a use after the first
  multi wildcard of a list holding two or more, at any depth. The one such
  name allowed multiplies the bound by the ranges it can take: measured
  cubic for `foo($$$A, $$$B, $$$C, $$$A, zz)`, about 8x per doubling of n.
  Repeats whose first use is fixed (`$A == $A`, `foo($A, $$$B) + bar($A)`)
  are free. A later use of a bound multi takes only the run whose byte
  length matches, found by binary search.
- Wrappers: a pattern node whose only non-trivia child is a multi (at any
  depth) stands for that multi in its parent's list. Python's
  `def f():\n    $$$BODY` parses the placeholder as an
  `expression_statement`, so without this the run would be the statement's
  children, not the block's.
- The capture is one range, from the first sibling's start to the last's
  end (extras between them included). An empty run's range is empty, at
  the end of the sibling before it, else the start of the one after it,
  else the parent's start: `foo()` gives `4..4`, inside the `(`.
- A multi standing for one node (the pattern root is `$$$A`) binds that
  node.
- Repeated multi names compare source text, as single ones do, so
  `foo(a,b)` and `foo(a, b)` runs differ.
- `Stopped::Unsupported` is gone: every pattern is matched.

## Cancellation (T709 F2)

- The walk and the matcher share one step count: each file node the walk
  visits, each node a comparison visits (extras in a child list included),
  and each run a multi tries. `stop` is asked every 1024 steps; once it says
  stop, every comparison fails, and `find_all` returns `Cancelled`.

## Pattern extras (T709 F1)

- `PatternNode` gains `extra`, copied from `Node::is_extra` at compile; the
  matcher skips pattern children by that flag, not by a `comment` kind
  name. gitignore's non-extra `comment` now matches (test in
  match_multi.rs).

## Placeholders and suffixes (T708 F1, F2)

- Placeholder spellings: the guided walk (blame the hole that is no node,
  else the one nearest the error) runs first. When it gives up, every
  combination it didn't try is parsed, up to 243 (all of them for five
  metavariables). JavaScript `import $A from $B` (identifier, then string)
  and JSON `{$K: $V}` (two strings) need this.
- A pattern child that is only the parse suffix (`;`, a newline) is left
  out of the copied tree and of `literals()`, and its parent gets
  `PatternNode::open_end`: a file node may have more children after the
  pattern's. `let $X = 1` matches `let a = 1;`.

## Constraints (`constraints.rs`)

- The section starts after the first line that trims to `where`; the code
  is everything before that line. `Pattern::source()` keeps the whole text,
  and error offsets are into it.
- Lines: `$NAME: [not ]kind=|regex=|text=<value>` (`$$$NAME` accepted too),
  `[not ]inside: kind=<k>[ name=<text>]`. Blank lines are skipped. `text=`
  and `regex=` values run to the end of the line, trailing spaces included
  (review F3); only the line ending (`\n`, `\r\n`) goes. `kind=` and
  `name=` take one word.
- `inside` has a `name=` qualifier: the ancestor's `name` field text, so C6's
  "not inside a function named test" is
  `not inside: kind=function_item name=test`. No `field=` option.
  Ancestors only: the matched node itself is never its own `inside`.
- `regex=` is unanchored (`Regex::is_match` on the covered text), compiled
  once at compile time; `text=` is exact.
- `kind=` on a multi: every named node of the run is of the kind (an empty
  run passes). Kind names aren't checked against the grammar (fe-syntax
  exposes no grammar handle): an unknown kind matches nothing.
- Several constraints on one name, and several `inside` lines, must all
  hold. Metavariable constraints are checked when the name first binds, so
  they steer a multi's run (`$$$REST: regex=c$`); a back-reference only
  compares text.
- Errors (new `PatternErrorKind`s): `UnknownConstraint` (a line that is
  neither form, an unknown key, an empty value, `inside:` without `kind=`,
  `not` before `$NAME`; offset at the part not understood),
  `UnboundMetavar` (a name the pattern lacks, or an anonymous one like
  `$_`; offset at its `$`), `InvalidRegex` (offset at the value).
