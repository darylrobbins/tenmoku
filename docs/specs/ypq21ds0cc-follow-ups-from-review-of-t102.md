---
task_uid: "01m492py12xwt3f8ypq21ds0cc"
task: "T122"
rev: 4
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F1: C file-scope variables inside #if/#ifdef, #elif and #else branches and nested guards, and fields of named structs/unions inside any preprocessor branch, are listed; fields of anonymous structs and block-scope declarations still aren't"
    required: true
    manual: false
  - id: "C2"
    text: "F2: top-level JS/TS/TSX object and array destructuring (including renamed and rest bindings), exported or not, lists one Variable per bound name and none for property keys; javascript.js.symbols lists a and b from const { a, b }"
    required: true
    manual: false
  - id: "C3"
    text: "F3: C int f [[deprecated]] (void); and int (g [[deprecated]])(void); produce no Variable symbol, while an attributed function-pointer variable still does"
    required: true
    manual: false
  - id: "C4"
    text: "F4: const gen = function* () {}; and const f = (() =\u003e 1);, exported or not, produce no Variable symbol in JS, TS and TSX, and existing outline Function items are unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "F5: Python class-body a, b = 1, 2 lists Fields a and b, and x = y = 1 lists both x and y in a class (Fields) and at module level (Variables); module-level unpacking and local assignments still aren't listed"
    required: true
    manual: false
  - id: "C6"
    text: "F6: a Java @interface Marker { int LIMIT = 1; } lists Field LIMIT nested under the Marker Interface"
    required: true
    manual: false
  - id: "C7"
    text: "F7: C int __stdcall h(void); and extern int real __asm__(\"real\"); produce no symbol named __stdcall or __asm__(\"real\"); real is still listed as a Variable"
    required: true
    manual: false
  - id: "C8"
    text: "F8: const C = class Named { x = 1; }; lists Field x in JS, TS and TSX; fields of anonymous class expressions still aren't listed"
    required: true
    manual: false
  - id: "C9"
    text: "New tests live in crates/fe-syntax/tests/symbols_followups.rs and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T102

Findings of the review of T102 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/queries/definitions/c.scm:29: C definitions disappear behind preprocessor wrappers. File-scope variables in #else, #elif or nested guards are omitted, and conditional fields inside named structs or unions are omitted even in the first branch. The queries require direct children rather than accounting for transparent preprocessor nesting.
  fix: Handle preprocessor branches and arbitrary nesting while preserving file-scope and named-type restrictions; add tests for conditional variables and fields in every branch.
  instances: crates/fe-syntax/queries/definitions/c.scm:11, crates/fe-syntax/queries/definitions/c.scm:16, crates/fe-syntax/queries/definitions/c.scm:29, crates/fe-syntax/queries/definitions/c.scm:31

- F2 medium crates/fe-syntax/queries/definitions/javascript.scm:18: Every variable pattern requires an identifier name, so top-level object and array destructuring definitions are absent in JavaScript, TypeScript and TSX. The supplied sample's `const { a, b } = ...` is silently omitted by its golden. The spec restricts Python module assignments to plain identifiers but makes no such exclusion for JavaScript declarators.
  fix: Capture each binding introduced by supported destructuring patterns, including renamed and rest bindings, without capturing property keys or locals; update the golden and test exported and non-exported declarations.
  instances: crates/fe-syntax/queries/definitions/javascript.scm:18, crates/fe-syntax/queries/definitions/javascript.scm:20, crates/fe-syntax/queries/definitions/javascript.scm:27, crates/fe-syntax/queries/definitions/javascript.scm:29, crates/fe-syntax/tests/symbols_samples/javascript.js.symbols:12

- F3 medium crates/fe-syntax/src/outline.rs:495: Prototype detection skips parenthesized declarators but not attributed declarators. Valid declarations such as `int f [[deprecated]] (void);` and `int (g [[deprecated]])(void);` therefore produce Variable symbols, violating the explicit prototype exclusion.
  fix: Traverse attributed declarators as transparent wrappers when identifying the first relevant declarator operator; retain the distinction between functions and function-pointer variables and test both attributed forms.
  instances: crates/fe-syntax/src/outline.rs:495

- F4 medium crates/fe-syntax/queries/definitions/javascript.scm:18: First-pattern-wins excludes only function initializers already matched by the outline. Generator expressions and parenthesized functions are not matched there, so `const gen = function* () {};` and `const f = (() => 1);` become Variable symbols despite the requirement to exclude function-bound declarators. This affects JavaScript, TypeScript and TSX, including exports.
  fix: Explicitly exclude function-bound initializers, including generator expressions and transparent expression wrappers, from the variable patterns while preserving existing outline Function items; add regression tests.
  instances: crates/fe-syntax/queries/definitions/javascript.scm:18, crates/fe-syntax/queries/definitions/javascript.scm:20, crates/fe-syntax/queries/definitions/javascript.scm:27, crates/fe-syntax/queries/definitions/javascript.scm:29

- F5 medium crates/fe-syntax/queries/definitions/python.scm:8: Python assignment coverage is limited to the outer assignment with one identifier target. A direct class-body `a, b = 1, 2` produces neither field, while `x = y = 1` produces only x in either a class or module. These missing targets are definitions within the specified scopes.
  fix: Handle class-body unpacking targets and chained plain-identifier assignments in classes and modules, listing each eligible binding under its proper parent; retain the exclusion of module unpacking and local assignments and add tests.
  instances: crates/fe-syntax/queries/definitions/python.scm:8, crates/fe-syntax/queries/definitions/python.scm:14

- F6 medium crates/fe-syntax/queries/definitions/java.scm:23: Java constant fields are handled only in ordinary interfaces. A named annotation interface such as `@interface Marker { int LIMIT = 1; }` is already an outline Interface, but its constant_declaration is inside annotation_type_body and LIMIT is omitted from symbols().
  fix: Add annotation-type constant field captures and test that each declarator nests beneath the annotation Interface.
  instances: crates/fe-syntax/queries/definitions/java.scm:23

- F7 medium crates/fe-syntax/queries/definitions/c.scm:24: The wildcard C declarator captures also accept nodes that introduce no identifier. The grammar marks calling-convention modifiers and assembly labels as declarator fields, so `int __stdcall h(void);` produces a phantom Variable named __stdcall, and `extern int real __asm__("real");` produces an extra Variable named __asm__("real"). declared_name returns these nodes unchanged.
  fix: Require new C definition captures to resolve to a declared identifier, rejecting modifier and assembly-label nodes; test ordinary declarations and prototypes carrying these annotations.
  instances: crates/fe-syntax/queries/definitions/c.scm:24, crates/fe-syntax/queries/definitions/c.scm:29, crates/fe-syntax/queries/definitions/c.scm:31

- F8 medium crates/fe-syntax/queries/definitions/javascript.scm:8: Field queries accept class_declaration only, omitting fields of named class expressions. For example, `const C = class Named { x = 1; };` lists C but no Field x in JavaScript, TypeScript or TSX. Named class expressions are not covered by the anonymous-type exclusion.
  fix: Add field patterns for class expressions with an explicit name, preserve the anonymous-class exclusion, and test the behavior in all three grammars.
  instances: crates/fe-syntax/queries/definitions/javascript.scm:8, crates/fe-syntax/queries/definitions/typescript.scm:4


Triage (2026-10-06, main a7975bd): all eight findings confirmed still present in queries/definitions/*.scm and outline.rs (declares_function skips only parenthesized_declarator). All kept. The public fe_syntax::symbols API doesn't change; only which symbols it returns. Scope narrowed to the queries and outline.rs these fixes touch, the JavaScript golden F2 updates, and a new integration test file.
