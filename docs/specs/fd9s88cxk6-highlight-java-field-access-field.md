---
task_uid: "01m7c01e9ps0j1r2fd9s88cxk6"
task: "T633"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features java --test java_member_highlights passes: field_access_is_a_property_and_a_call_is_a_method, field_declarations_are_properties, annotation_keys_are_properties"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features java highlights_match_golden_files passes with java.java.golden regenerated"
    required: true
    manual: false
---

# Highlight Java field access, field declarations and annotation keys as property (java.scm override) (Mac T1822)

# Highlight Java fields, element-value keys and field declarations as properties

Java has no override query (language.rs ~line 853 uses only `tree_sitter_java::HIGHLIGHTS_QUERY`), so
`obj.field`, `int x;` field declarations and annotation `@A(key = 1)` keys are plain `@variable`. Decision
2026-11-13: names that label a value are `property`.

New `crates/fe-syntax/queries/java.scm` (header: "Ours. After the crate's query, so this wins on the same
node."):

- `field_access` `field:` identifier `@property`; keep a method call distinct (`a.b()` is a `method_invocation`,
  its `name` stays `function.method`; the test pins it).
- `field_declaration` `declarator:` -> `variable_declarator` `name:` `@property`.
- `element_value_pair` `key:` identifier `@property`.

Check node and field names against tree-sitter-java 0.23.5's `node-types.json`. Wire: the java `Def` `highlights`
becomes `&[tree_sitter_java::HIGHLIGHTS_QUERY, include_str!("../queries/java.scm")]`. No java extension exists, so
no `extensions/` change. Update the java row in THIRD_PARTY.md ("`HIGHLIGHTS_QUERY`, then `queries/java.scm`").
Regenerate `tests/golden/java.java.golden` (`UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --features java golden`),
review: only the names above change.

**Test.** New file `crates/fe-syntax/tests/java_member_highlights.rs`, `#![cfg(feature = "java")]`, `spans`
helper like `c_printf_highlights.rs`. `field_access_is_a_property_and_a_call_is_a_method` (`this.count`, `a.b.size`
vs `a.run()`), `field_declarations_are_properties` (`private int count = 0;`, `String a, b;`),
`annotation_keys_are_properties` (`@Test(timeout = 5)`). Base fails on the assertions.

Conflicts: shares `language.rs` with A and F; depends on A so the two never run together. Linux-OK.
