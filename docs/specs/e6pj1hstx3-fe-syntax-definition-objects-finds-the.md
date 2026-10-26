---
task_uid: "01m5rht3at2rbs8we6pj1hstx3"
task: "T241"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "In a Kotlin class, a Swift class and protocol, and a Ruby class of 5,000 members each, a secondary constructor, an init, deinit, subscript and protocol method, and an alias placed last are in definition_objects' chain with the right kind"
    required: true
    manual: false
  - id: "C2"
    text: "definition_objects_counted stays within a fixed work bound on those files and on T228's definition_objects_is_bounded fixture"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-syntax and clippy pass; no query file under crates/fe-syntax/queries changes"
    required: true
    manual: false
---

# fe-syntax: definition_objects finds the Kotlin, Swift and Ruby items only container patterns capture past the per-ancestor query budget (T228 review F5)

## Problem

T228 review round 2, finding F5. `fe_syntax::definition_objects` (T228) finds the definitions around an offset by running the outline query rooted at each ancestor of the offset (`outline::items_containing`, `Find::At`). Each run stops after `MAX_OPERATIONS_AT` (100) hundred tree-sitter operations, so a call stays bounded when a pattern rooted at a container lists all the container's members. Past roughly a thousand members, an item that only a container pattern captures is silently dropped from the chain:

- Kotlin secondary constructors (`class_body [... (secondary_constructor)] @definition.method`)
- Swift initializers, deinitializers, subscripts and protocol methods (`class_body` / `protocol_body` patterns)
- Ruby aliases (`class body: (body_statement [(alias)])`)

Items that also have a standalone pattern (Rust fns in an impl, Python defs in a class) are unaffected.

## Change

Find container-dependent captures along the offset's ancestor branch without scanning the members before it, and without discarding results at the budget. For example: classify each outline pattern by the path from its root to its item capture once per language, and for an ancestor that a container pattern could capture, check the pattern against just that ancestor and its parents. Or derive item-rooted queries in code. No query file under `crates/fe-syntax/queries` changes.

## Tests

Large-container regressions in `crates/fe-syntax/tests/definition_objects.rs`: a Kotlin class with 5,000 members and a secondary constructor last, a Swift class and protocol with 5,000 members and an init / deinit / subscript / protocol method last, and a Ruby class with 5,000 methods and an alias last. Each is found with the right kind, and `definition_objects_counted`'s work stays within a fixed bound.

## Implementation notes

- `OutlineQuery::rooted` splits the outline query once per language for `items_containing`. Each pattern's source is read back (`contained_items`, a small reader of query syntax starting at `Query::start_byte_for_pattern`). A pattern whose item capture sits below its root on a path of named nodes, alternations, fields and `!field`s alone becomes one item-rooted pattern per item capture (its own text plus the pattern's predicates on its captures) in a derived query, `Rooted::contained`, and is disabled in the copy of the outline query run per ancestor, so pattern indices stay the same. At each ancestor both run; a derived match is kept only if `Contained::holds` finds the item's parents of the path's kinds, in its fields (by a cursor descent by byte, not a scan of the container's children), without its negated fields, and it takes the outline pattern's index, so the first pattern still wins on a node.
- Derived this way: Rust impl and trait bodies, Python class bodies and module constants, Go constants, the C++ field-declaration method, Ruby class/module/`class << self` bodies (direct and `private def`), Swift class, enum and protocol bodies, Kotlin class and enum bodies, Scala template and enum bodies, Lua's module tables. Patterns with an anchor, a sibling or a predicate-tested capture on the path (Lua assignment targets, make targets, the C# constant, Ruby's multiple-assignment constants, the C++ `@_declaration` method) stay whole in the budgeted query. `container_patterns_run_rooted_at_their_items` pins the split.
- `definition_objects_counted` now counts the body children it looks at rather than all of a body's children, so a class of 5,000 members costs its edges (`inner`, `content_start`, `content_end`, via `not_blank`).
- Carried F7: `deep_edge_expressions_are_bounded` queries the function name of bodies 3,000 nodes deep down the left (`1 + 1 + …`) and right (`- - … 1`) edges, checking the whole inner range and a fixed work bound.
