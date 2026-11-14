---
task_uid: "01m7c01cfza4wfxn89yr2wa0cw"
task: "T629"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features csharp --test csharp_member_highlights passes: initializers_and_named_arguments_are_properties, member_access_is_a_property_but_a_call_is_a_method, property_and_field_declarations_are_properties"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features csharp --test wasm_parity csharp_extension_is_as_built_in passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax --features csharp --test csharp_preproc_highlights and highlights_match_golden_files pass with csharp.cs.golden regenerated"
    required: true
    manual: false
---

# Highlight C# members (initializers, named arguments, member access, declarations) as property (Mac T1818)

# Highlight C# members as properties

The C# highlight query (`tree-sitter-c-sharp` 0.23.5 `HIGHLIGHTS_QUERY`) has no property captures, so member names
are `@variable`, i.e. plain text in the themes without a `variable` colour (all tenmoku-*, Hakuji, Yohen). Decision
2026-11-13: a name that labels a value is `property`. Add patterns to the fastedit-owned
`crates/fe-syntax/queries/csharp.scm` (T1765's file; it holds only the preprocessor keywords today, append after
them) and mirror the whole file into `extensions/grammars/csharp/highlights.scm` (crate query then `csharp.scm`;
`wasm_parity.rs` `csharp_extension_is_as_built_in` requires byte equality, so edit both together; no
`wasm_parity.rs` edit is needed, the arm already lists both strings).

Patterns (check each node/field name against the 0.23.5 `node-types.json`; an unknown name fails the whole query):

- `initializer_expression` assignment: `new A { Name = 1 }`: the `identifier` on the left of an
  `assignment_expression` inside an `initializer_expression`.
- `with_expression` initializer: `a with { X = 2 }`: the left identifier in the `with_initializer`.
- argument names: `F(name: 1)`: `argument` `name_colon` identifier; attribute arguments `[Foo(Bar = 1)]`:
  `attribute_argument` `name_equals`/`name_colon` identifier.
- member access: `a.b` `member_access_expression` `name:` `@property`, then restate the call case so the method
  stays a function: `(invocation_expression function: (member_access_expression name: (identifier) @function.method))`
  (the crate query's existing pattern for it, or `@function`; copy its capture so the call colours do not change).
- declarations: `property_declaration` `name:` and `field_declaration` -> `variable_declaration` ->
  `variable_declarator` `name:` as `@property`.

Later patterns win (`earlier_patterns_win` false) so these go after the crate's; verify precedence with the test, not
by reading.

Shipping: the wasm is unchanged, so no `pack.toml` edit and no `NOT_RELEASED` entry. `extensions/grammars/csharp/`
ships via the grammar registry's own release; the built-in (native) csharp gets the change with the next app
build. Whether the grammar-pack release repacks the directory is T1486/T1487's call; do not touch them.

Regenerate `crates/fe-syntax/tests/golden/csharp.cs.golden` (`UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax
--features csharp golden`) and review: only names listed above change, to `property`; method names, types,
locals and parameters keep theirs.

**Test.** New file `crates/fe-syntax/tests/csharp_member_highlights.rs`, `#![cfg(feature = "csharp")]`, same
`spans` helper as `tests/csharp_preproc_highlights.rs`. One fixture string with: object initializer
`new A { Name = 1 }`, `with { X = 2 }`, named argument `F(count: 3)`, attribute argument `[Obsolete(Message = "m")]`,
`this.field` / `p.Name` member access (property) against `p.Run()` (function.method, not property), a property
declaration `public int Size { get; set; }` and field declaration `private int _n;` (both property). Tests:
`initializers_and_named_arguments_are_properties`, `member_access_is_a_property_but_a_call_is_a_method`,
`property_and_field_declarations_are_properties`. Plus extension parity:
`cargo nextest run -p fe-syntax --features csharp --test wasm_parity csharp_extension_is_as_built_in`.

Linux-OK.
