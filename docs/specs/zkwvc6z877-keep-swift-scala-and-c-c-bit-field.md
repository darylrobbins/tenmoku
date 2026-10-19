---
task_uid: "01m5a3sfbc1y1af5zkwvc6z877"
task: "T205"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Swift properties and enum payloads, Scala typed patterns and C/C++ bit-fields report their own suffix as detail"
    required: true
    manual: false
  - id: "C2"
    text: "Sibling declarations are not consumed (existing leakage regressions still pass)"
    required: true
    manual: false
  - id: "C3"
    text: "Regression tests in outline_detail_suffix.rs fail at the base and pass with the fix"
    required: true
    manual: false
---

# Keep Swift, Scala and C/C++ bit-field detail suffixes in the Outline's ancestor allowlist (T197 review F6, Mac T991 follow-up)

## Problem
T197 review F6 (known limit after respec): the ancestor allowlist in crates/fe-syntax/src/outline.rs (around lines 1608, 1703-1712) drops details the earlier extraction found: Swift `struct S { var x: Int }` loses `: Int`, Swift `enum E { case a(Int) }` loses `(Int)`, Scala `val Some(x: Int) = opt` loses `: Int` (typed_pattern excluded), C/C++ `struct S { unsigned x:3; };` loses `:3` (field_declaration excluded).
## Change
Resolve these captures to bounded declaration or pattern ranges that include their own suffix without consuming sibling declarations. Add detail assertions for Swift properties and enum payloads, Scala typed patterns and C/C++ bit-fields while keeping the sibling-leakage regressions.

## Implementation notes

- A new per-language table, `OWN_DECLARATIONS` in outline.rs, lists the declarations and patterns in which each name has its own suffix: Swift's `property_declaration`, `protocol_property_declaration` and `enum_entry`, Scala's `typed_pattern`, and the `field_declaration` of C and C++. A name-only item climbs into one of these as it does into the `NAME_DECLARATIONS` kinds, but climbs no further, and inside it a comma at bracket depth 0 ends the detail. It doesn't skip to the next name, as Go's `X, Y int` does. As a result Swift's `var a: Int, b: String` gives `: Int` and `: String`, `case f, g(Int)` gives `f` no detail, C's `int a:1, b:2` gives `:1` and `:2`, and `int x, y[4][5]` still leaves `x` empty.
- Details still end at `=` and `{`, so a Swift raw value (`case d = 1`), a computed property's body, a protocol's `{ get }` and a C++ default member initializer are left out. Names in a Swift tuple pattern, as in `let (p, q): (Int, Int)`, still get no detail: `pattern` isn't listed, so the type isn't split between the names.
- The 256-child limit on entering a parent applies here too.
- Tests are in `crates/fe-syntax/tests/outline_detail_suffix.rs`. All of them fail at the base except `swift_raw_values_and_tuple_names_dont_take_a_siblings_suffix`, a regression test for sibling leakage. The task's regression test, `outline_detail_suffix`, is a test named for the file: the gate's `test_one` matches the exact test name. It checks one case of each kind, each beside a sibling.
