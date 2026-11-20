# fe-structural: matching a compiled pattern against a tree (T709, structural search SS3)

`Pattern::find_all(source, tree, opts, stop)` finds where a compiled
pattern matches in a file's tree; `find_in_text(text)` and
`find_in_text_with(text, opts, stop)` parse with `fe_syntax::parse_str`
first. These are the choices the spec left open, or where the code departs
from its wording.

## API

- `Match { range, captures }`; `Captures` is an ordered list of
  `(name, Vec<Range<usize>>)` in the pattern's metavariable order, with
  `get`, `single`, `iter`, `len`, `is_empty`. Only named metavariables
  appear; `$_` and the like capture nothing. A name used twice captures its
  first occurrence's range, once.
- The spec's error type is named `Stopped` and holds the reasons a search
  gives no matches: `Cancelled` (the callback), `Unsupported` (a `$$$`
  anywhere in the pattern, until SS4) and `ParseFailed` (only from
  `find_in_text*`, when `parse_str` returns `None` without the callback
  having said stop). `Unsupported` is checked before parsing.
- `find_in_text_with` passes `stop` to the parse as well, so a cancel
  during the parse is `Cancelled` too.

## Matching

- Candidates are file nodes of the pattern root's kind. Kind equality is
  named-ness plus kind id, or kind name when the ids differ (a grammar can
  alias one name to several ids). A pattern whose root is a lone wildcard
  (`$A`) takes any named node that isn't an extra or MISSING.
- Named leaf: same text. Anonymous token: same kind (its text is fixed by
  the grammar). A pattern leaf also needs the file node to have no
  non-extra children.
- Children are compared pairwise in order; file extras (`is_extra()`) are
  skipped, and a count difference is a mismatch. Field names are never
  compared.
- `PatternNode` (SS2) records no extra flag, and `pattern.rs` is outside
  this card's scope, so on the pattern side a comment is recognized by its
  kind name containing `comment` (true of every bundled grammar). Known
  limit: a grammar whose extras are named otherwise compares a comment
  written into the pattern like code, so that pattern can't match; and a
  non-extra kind named `*comment*` is skipped in the pattern but not in the
  file. Recording `is_extra` in `PatternNode` fixes both if it is ever
  needed.
- A MISSING node (zero width, from error recovery) never matches, wildcard
  or not: it is no text in the file.
- A repeated name compares the two nodes' source text byte for byte, as the
  spec says: `f(x) == f( x )` doesn't match `$A == $A`.
- Without multi wildcards there is no backtracking: the first mismatch
  ends the attempt, and the bindings are cleared per candidate.

## Walk

- One `TreeCursor` walks the file in pre-order; a second is reset to each
  candidate and walks its subtree, so matching allocates nothing per node.
- Pre-order reports outer matches before inner ones. By default a matching
  node isn't descended into, so no two matches overlap; with
  `MatchOptions { nested: true }` it is, and inner matches follow it.
- `stop` is asked before the first node and then every 1024 file nodes
  visited (nodes compared inside a candidate don't count: a candidate is
  bounded by the pattern's size).
- The tree's language isn't checked against the pattern's: fe-syntax
  exposes no grammar handle to compare, and a foreign tree just finds no
  kinds in common.
