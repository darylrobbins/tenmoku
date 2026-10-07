---
task_uid: "01m49vhs0r92jxe1rgbftj9tzc"
task: "T153"
rev: 2
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T137

Findings of the review of T137 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/outline.rs:1075: Resolving a using directive can revisit that same directive until the shared lookup budget is exhausted. For `namespace A { namespace N { void f(); extern int x; } } namespace B { using namespace A; } void B::N::f() {} int B::N::x = 0;`, resolving A from B repeatedly searches B's directive before reaching global A. B::N consequently becomes Unknown, misclassifying f as Method and x as Field.
  fix: Track active directive lookups or resolve directive targets independently so revisiting a directive does not prevent lookup in enclosing namespaces. Add regressions covering functions and data through directives declared in nested namespaces.
  instances: crates/fe-syntax/src/outline.rs:1032, crates/fe-syntax/src/outline.rs:1075

- F2 medium crates/fe-syntax/src/outline.rs:1231: C++ alias and using-directive targets are selected from unfiltered named children, which include comments. `namespace M /* comment */ = A;` selects the comment as the alias target, while `using namespace A /* comment */;` selects the trailing comment as the directive target. With A::N a namespace, definitions through M::N or imported N are then incorrectly classified as methods and fields.
  fix: Select target children by their grammar kinds, excluding comments, in both collection paths. Add commented alias and directive regressions covering functions and data in nested namespaces.
  instances: crates/fe-syntax/src/outline.rs:1221, crates/fe-syntax/src/outline.rs:1231

- F3 medium crates/fe-syntax/queries/definitions/swift.scm:30: The recursive Swift captures also capture tuple labels, and bound_by treats every pattern edge as binding. In `let (a, (left: b, right: c)) = (1, (left: 2, right: 3))`, left and right are direct identifiers in the nested tuple pattern, so they become spurious Variables alongside a, b and c; type-member declarations similarly produce spurious Fields.
  fix: Distinguish binding leaves from tuple labels during Swift pattern extraction. Add labeled nested tuple regressions at file and type scope, with matching function-local exclusions.
  instances: crates/fe-syntax/queries/definitions/swift.scm:30, crates/fe-syntax/queries/definitions/swift.scm:33, crates/fe-syntax/src/outline.rs:847

- F4 medium crates/fe-syntax/queries/definitions/scala.scm:42: Scala binding extraction still omits actual bindings in supported pattern forms. `val (all @ x, y) = pair` captures all and y but never x because only capture_pattern's name field is queried. Named tuple patterns also fail: their identifiers are not queried, and even nested ordinary tuples are rejected when bound_by encounters named_pattern or named_tuple_pattern.
  fix: Capture binding identifiers in capture_pattern's pattern field and named-pattern values, and traverse named tuple wrappers while excluding their labels. Cover file/package variables, type fields and function-local exclusions.
  instances: crates/fe-syntax/queries/definitions/scala.scm:34, crates/fe-syntax/queries/definitions/scala.scm:77, crates/fe-syntax/src/outline.rs:833

- F5 medium crates/fe-syntax/src/outline.rs:903: Ruby assignment-chain traversal stops at parenthesized_statements. Consequently `A = (B = 1)` and `A, B = (C = [1, 2])` still omit the inner constants, including inside class and module bodies. Parentheses do not change these assignment targets or their permitted scope.
  fix: Traverse transparent parenthesized assignment expressions before continuing through the enclosing assignment, without admitting unrelated statements or function-local constants. Add single/multiple parenthesized-chain regressions at file, class and module scope.
  instances: crates/fe-syntax/src/outline.rs:903

- F6 medium crates/fe-syntax/src/outline.rs:794: Lua function-value association rescans the target list to find each target's index and then rescans the value list to reach that index. The new query invokes this for every target even when no RHS value is a function, making long assignments quadratic. outline() can perform all this work while emitting zero items, so MAX_OUTLINE_ITEMS does not bound it.
  fix: Build and cache target-to-function-value associations once per assignment, or otherwise associate the two lists in one traversal. Add a scaling regression that verifies linear work for long mixed or non-function assignments.
  instances: crates/fe-syntax/src/outline.rs:794, crates/fe-syntax/src/outline.rs:808, crates/fe-syntax/queries/outline/lua.scm:47
