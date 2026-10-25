---
task_uid: "01m5qphz7nppgv8pk9j2qx1b2q"
task: "T228"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test definition_objects_rust_python_ts (top level, crates/fe-syntax/tests/definition_objects.rs): at offsets in a Rust fn, impl method and nested fn, a Python def inside a class, and a TypeScript method and const arrow function, the chain is innermost first with the right kind, outer node and inner body range (braces and edge blank lines excluded)"
    required: true
    manual: false
  - id: "C2"
    text: "definition_objects_bodyless_and_unsupported: a trait method signature has outer and no inner; an unsupported language, a text with no tree and an offset outside any definition give an empty list"
    required: true
    manual: false
  - id: "C3"
    text: "definition_objects_is_bounded: on a 50,000-row Rust file the call visits a bounded number of nodes (asserted by a work counter, not wall time)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test syntax_objects_tests::host_maps_definition_objects (crates/fe-gpui/src/syntax_objects_tests.rs, declared by one #[cfg(test)] #[path] line in commands.rs): the EditorHost implementation returns the same chain as fe-syntax for a Rust view, and the default trait method returns none"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-syntax -p fe-editor -p fe-gpui and clippy pass; no query file under crates/fe-syntax/queries changes"
    required: true
    manual: false
---

# fe-syntax: definition_objects lists the functions and classes around an offset with their outer and inner (body) ranges, behind a new EditorHost method (Mac T1151)

## Problem

The user asked on 2026-10-24 for the Neovim treesitter text objects as vim defaults: `af`/`if` (function), `ac`/`ic` (class or type). fe-vim does not depend on fe-syntax and its `text_object::select` sees only the text, so it cannot find a function. This card is the syntax side and the host seam; the next card is the vim keys.

## Current state

- `fe_syntax::outline` (`crates/fe-syntax/src/outline.rs`) lists every definition of 24 languages with `OutlineKind` (Function, Method, Class, Struct, Enum, Interface, Trait, Impl, Type...), its node `range`, `name_range` and `parent`, from `queries/outline/<lang>.scm`; `Outline::item_at` (outline.rs:141) finds the innermost item at an offset. It gives the whole node, not the body.
- The host seam pattern is `EditorHost::larger_syntax_node` (`crates/fe-editor/src/commands.rs:62`, default `None`), implemented by fe-gpui from `fe_syntax::larger_syntax_node` (`crates/fe-gpui/src/commands.rs:325`, `crates/fe-syntax/src/select.rs:25`).

## Change

- New `crates/fe-syntax/src/textobject.rs` (module line and `pub use` in `lib.rs`): `pub enum DefinitionKind { Function, Class }`, `pub struct DefinitionObject { kind, outer: Range<usize>, inner: Option<Range<usize>> }` and `pub fn definition_objects(syntax: &Syntax, offset: usize) -> Vec<DefinitionObject>`, innermost first: the outline items containing `offset` (Function, Method -> Function; Class, Struct, Enum, Interface, Trait, Impl, Type -> Class). `outer` is the item's node. `inner` is the node's `body` field (tree-sitter field name; Python's block, a brace body, a Lua/Ruby body) without its braces and the blank edge lines of a braced body (nvim-treesitter-textobjects' `@function.inner`), `None` for a bodyless declaration (a trait method signature). Use the outline's ranged query run, never the whole-file outline, per call (see `outline_scopes`, outline.rs:207); no tree, or an unsupported language, gives none.
- `EditorHost` (fe-editor commands.rs): `fn definition_objects(&self, offset: usize) -> Vec<SyntaxObject>` (neutral `SyntaxObject { function: bool, outer, inner }` types in fe-editor, default empty), implemented in fe-gpui commands.rs by mapping fe-syntax's.
- No fe-vim change; no new query files (the T1123 to T1125 cards own queries).

## Tests

New `crates/fe-syntax/tests/definition_objects.rs`: Rust (fn, impl method, nested closure inside fn, trait method signature has no inner), Python (def, class, nested def), TypeScript (class method, arrow function assigned to const: Function), Go and Lua, offsets on the name, in the body and between items; an unsupported language and a tree-less text give none; a 50,000-row Rust file answers within a fixed work bound (assert node visits or a generous 500 ms). A fe-gpui test `commands::syntax_objects_tests` (new file declared by one line in commands.rs) proves the host maps them.

## Implementation notes

- Not the `Find::Overlapping` run `outline_scopes` uses: tree-sitter's query cursor enters every sibling along the path even with a byte range set, and a pattern rooted at a container (an impl's methods) matches all its children, so that run grows with the file (review F1, round 1). Instead `outline::items_containing` walks the ancestors of the offset's leaf and runs the outline query rooted at each one (`Find::At`, `set_max_start_depth(0)`), keeping the items that contain the offset, lowest pattern winning per node as in the outline. Each run stops after `MAX_OPERATIONS_AT` (100) hundred tree-sitter operations, counted by the query cursor's progress callback. Past roughly a thousand members, an item that only a container pattern lists (a Kotlin secondary constructor, a Swift protocol method) is missed. Turning container patterns off altogether lost those items in the samples, so they stay on, capped.
- Work counter (`definition_objects_counted`, `#[doc(hidden)]`): an upper bound of each ancestor, plus each run's operations rounded up to the next hundred, plus the nodes looked at for the bodies' edge tokens. Finding the leaf itself is tree-sitter's `descendant_for_byte_range`, as for `larger_syntax_node`. C3's fixture (over 50,000 rows: 1,000 modules, a 3,000-method impl, a fn of 20,000 blank rows, 5,000 one-row fns) stays under 20,000, and a fn whose body is one 50,000-row expression, asked at its name, stays under 2,000. The method deep in the big impl is about 10,800 because it hits the cap; the others are 600 to 1,500.
- `inner`: the `body` field; else, in Lua, the function at the assignment target's place among the values (review F2); else the `body` of the `value` field (a const's arrow function, a Lua table field's function). Inside the braces, the edges are the first and last children that aren't whitespace-only or zero-width tokens. Each edge node keeps its bounds unless its own edge is whitespace: a visible whitespace token, or a hidden token of at most 8 bytes of whitespace outside its visible children, such as Go's newline terminator at the end of a `statement_list`. Then the edge descends one level, at most 3 levels down. So an expression, a raw string (`r#"…"#`, whose delimiters are hidden tokens; review F4) or a comment is never cut, a deep expression costs nothing (review F6), and blank rows cost nothing (F3). Child scans are counted in the work bound. An empty braced body gives an empty range just after `{`. A Go `type S struct {...}` has no `body` field and so no inner.
- Documented limit (review round 2, F5, filed as T241 per the round-2 process): past the per-ancestor budget, items that only a container pattern captures (Kotlin secondary constructors; Swift init, deinit, subscript and protocol methods; Ruby aliases) drop out of the chain in containers of more than roughly a thousand members.
- `samples_match_the_whole_outline` checks every offset of the outline samples (C, C++, C#, Go, Java, JS, Kotlin, Lua, PHP, Python, Ruby, Rust, Scala, Swift, TS, TSX...) against the whole outline's chain.
- Under `cargo test` (one process), `element::minimap::tests::a_cold_fence_seen_only_in_the_minimap_gets_its_colours` fails because an earlier test has already loaded Python; it passes alone and under nextest. Not related to this change.
