---
task_uid: "01m4afbqr3rfntxabfatjbgve8"
task: "T166"
rev: 1
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T153

Findings of the review of T153 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/outline.rs:1176: The directive is removed from active before searching its imported namespace, so self-imports and cyclic imports still exhaust the shared budget and prevent subsequent directives from being searched. For example, `namespace A { namespace N { void f(); extern int x; } } namespace B { using namespace B; using namespace A; } void B::N::f() {} int B::N::x = 0;` still misclassifies the definitions as Method and Field.
  fix: Keep the directive active through both target resolution and the imported member lookup, restoring the active stack before returning. Add self-import and cyclic-import regressions with a later successful directive, covering functions and data.
  instances: crates/fe-syntax/src/outline.rs:1176

- F2 medium crates/fe-syntax/queries/outline/lua.scm:44: The new single-target anchors require the target to be the only named child, but Lua attributes are also named children of variable_list. Consequently `local f <const> = function() end`, `local x <const> = 1`, and `local M <const> = { balance = 0 }` lose their function, variable, or module captures. The multiple-target fallback rejects these because the cache correctly counts only one target.
  fix: Make every single-target pattern permit non-target children such as attributes and comments while retaining linear matching. Add attributed single-target regressions for functions, variables, modules, and table fields.
  instances: crates/fe-syntax/queries/outline/lua.scm:44, crates/fe-syntax/queries/outline/lua.scm:69, crates/fe-syntax/queries/outline/lua.scm:74, crates/fe-syntax/queries/definitions/lua.scm:15, crates/fe-syntax/queries/definitions/lua.scm:23, crates/fe-syntax/queries/definitions/lua.scm:38, crates/fe-syntax/queries/definitions/lua.scm:42, crates/fe-syntax/queries/definitions/lua.scm:46

- F3 medium crates/fe-syntax/src/outline.rs:881: Swift block comments have kind multiline_comment, which this loop does not skip. In `let (a, (left /* c */: b, right: c)) = (1, (left: 2, right: 3))`, the label's next sibling is the comment rather than the colon, so left is still emitted as a spurious Variable; the same occurs as a Field at type scope.
  fix: Skip both Swift comment kinds before checking for the label colon, and add commented-label regressions at file and type scope with function-local exclusions.
  instances: crates/fe-syntax/src/outline.rs:881

- F4 medium crates/fe-syntax/queries/definitions/scala.scm:45: The anchored named-pattern captures permit comment nodes but not Scala's separate block_comment nodes. Thus `val (name /* c */ = n, age = a) = person` omits the actual binding n, and the corresponding type-scope declaration omits its Field.
  fix: Permit both comment kinds between the label and binding in both query paths, and add block-comment regressions for file/package variables and type fields.
  instances: crates/fe-syntax/queries/definitions/scala.scm:45, crates/fe-syntax/queries/definitions/scala.scm:90
