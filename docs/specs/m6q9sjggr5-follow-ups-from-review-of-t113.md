---
task_uid: "01m49jxvmw2g3nj1m6q9sjggr5"
task: "T137"
rev: 2
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T113

Findings of the review of T113 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-syntax/src/outline.rs:733: C++ member detection compares only the innermost qualifier against file-wide sets of unqualified names. An unrelated B::N type makes A::N::f a Method even when A::N is a namespace, and makes A::N::x a Field. Namespace aliases are not collected either, so namespace M = N followed by definitions of M::f and M::x produces the same incorrect classifications.
  fix: Resolve qualifiers using qualified scope paths and lexical context, including namespace aliases, rather than file-wide basename membership. Add regressions with identically named types and namespaces in different scopes and with namespace aliases, covering both functions and data.
  instances: crates/fe-syntax/src/outline.rs:733, crates/fe-syntax/src/outline.rs:765, crates/fe-syntax/src/outline.rs:771, crates/fe-syntax/src/outline.rs:778, crates/fe-syntax/queries/outline/cpp.scm:44, crates/fe-syntax/queries/definitions/cpp.scm:54, crates/fe-syntax/queries/definitions/cpp.scm:69, crates/fe-syntax/queries/definitions/cpp.scm:91, crates/fe-syntax/queries/definitions/cpp.scm:106

- F12 medium crates/fe-syntax/src/outline.rs:538: Conversion-name extraction traverses only declarator fields, but abstract_reference_declarator has an unnamed declarator child. Consequently operator int&() const is named "operator int&() const" and operator int&&() is named "operator int&&()", retaining parameters and qualifiers instead of producing the conversion target name; declarations and definitions can therefore have different lookup names.
  fix: Traverse unnamed reference and parenthesized abstract-declarator children as well as declarator fields, stopping at the conversion function's parameter list. Test lvalue/rvalue reference conversions in declarations and definitions with differing trailing qualifiers.
  instances: crates/fe-syntax/src/outline.rs:538, crates/fe-syntax/queries/outline/cpp.scm:15, crates/fe-syntax/queries/outline/cpp.scm:19, crates/fe-syntax/queries/outline/cpp.scm:31, crates/fe-syntax/queries/outline/cpp.scm:36, crates/fe-syntax/queries/outline/cpp.scm:43, crates/fe-syntax/queries/outline/cpp.scm:46

- F2 medium crates/fe-syntax/queries/outline/lua.scm:36: The Lua fix retains sibling variables by suppressing function recognition for multiple targets. local g,h = function() end,1 now produces two Variables and no Function g, so g disappears from the outline; the new regression explicitly requires this result. The expression-list anchors also omit local g = function() end,1 despite g receiving the function.
  fix: Associate each assignment target with its corresponding explicit function value and emit that binding as Function without suppressing sibling Variables. Test both outline() and symbols() for local/global mixed assignments and surplus RHS values.
  instances: crates/fe-syntax/queries/outline/lua.scm:36, crates/fe-syntax/queries/outline/lua.scm:45, crates/fe-syntax/queries/definitions/lua.scm:49, crates/fe-syntax/queries/definitions/lua.scm:59, crates/fe-syntax/tests/more_languages.rs:384, crates/fe-syntax/tests/more_languages.rs:386

- F7 medium crates/fe-syntax/queries/definitions/scala.scm:18: Compound-binding extraction handles only direct children of one tuple/list level. Scala val (a,(b,c)) = pair, Swift let (a,(b,c)) = pair, and PHP list($a,list($b,$c)) = $row omit b and c in the permitted scopes. PHP's generic list-child capture also indexes the read-only $key in [$key => $value] = $row as a declared Variable.
  fix: Extract actual bound identifiers recursively, distinguishing PHP destructuring keys from assignment targets and retaining the existing scope exclusions. Add nested top-level/type-member regressions and matching function-local exclusions, plus keyed PHP destructuring cases.
  instances: crates/fe-syntax/queries/definitions/scala.scm:18, crates/fe-syntax/queries/definitions/scala.scm:21, crates/fe-syntax/queries/definitions/scala.scm:28, crates/fe-syntax/queries/definitions/scala.scm:31, crates/fe-syntax/queries/definitions/scala.scm:50, crates/fe-syntax/queries/definitions/scala.scm:53, crates/fe-syntax/queries/definitions/scala.scm:61, crates/fe-syntax/queries/definitions/scala.scm:64, crates/fe-syntax/queries/definitions/swift.scm:31, crates/fe-syntax/queries/definitions/swift.scm:36, crates/fe-syntax/queries/definitions/swift.scm:39, crates/fe-syntax/queries/definitions/php.scm:54, crates/fe-syntax/queries/definitions/php.scm:60

- F8 medium crates/fe-syntax/queries/definitions/cpp.scm:68: Template handling unwraps only one template_declaration. For template<class T> struct A { template<class U> static U x; }; template<class T> template<class U> U A<T>::x = U();, the out-of-class definition is omitted because the captured inner template's scope is another template_declaration, which every relevant scope predicate rejects. This is an ordinary generic member-variable template, not a specialization.
  fix: Determine declaration scope through arbitrarily nested template wrappers, preserving Field/Variable classification and local exclusions. Add a regression for an out-of-class member-variable template of a class template.
  instances: crates/fe-syntax/queries/definitions/cpp.scm:37, crates/fe-syntax/queries/definitions/cpp.scm:53, crates/fe-syntax/queries/definitions/cpp.scm:68, crates/fe-syntax/queries/definitions/cpp.scm:90, crates/fe-syntax/queries/definitions/cpp.scm:105, crates/fe-syntax/src/outline.rs:801

- F16 medium crates/fe-syntax/queries/definitions/scala.scm:85: The anchored case-class patterns omit the grammar's optional constructor annotation. In case class C @Inject() (x: Int)(ctx: String), x is an implicit case-class Field, but none of the four patterns matches because annotation is a named child between the name and first parameter list.
  fix: Allow the constructor annotation in each applicable anchored form while still selecting only the first parameter list. Test annotated constructors with type parameters/access modifiers and a later ordinary parameter list.
  instances: crates/fe-syntax/queries/definitions/scala.scm:85, crates/fe-syntax/queries/definitions/scala.scm:94, crates/fe-syntax/queries/definitions/scala.scm:103, crates/fe-syntax/queries/definitions/scala.scm:114

- F17 medium crates/fe-syntax/queries/outline/ruby.scm:76: Ruby constant patterns require each assignment to be a direct scope child, so A = B = 1 indexes only A even though it defines both constants. The same omission occurs inside class/module bodies and when a multiple assignment has a chained RHS, such as A,B = C = [1,2].
  fix: Recognize constant targets throughout assignment chains rooted in permitted scopes, emitting distinct constant items and terminal qualified names. Add single/multiple chained-assignment regressions at file, class and module scope.
  instances: crates/fe-syntax/queries/outline/ruby.scm:76, crates/fe-syntax/queries/outline/ruby.scm:83, crates/fe-syntax/queries/outline/ruby.scm:90, crates/fe-syntax/queries/outline/ruby.scm:98, crates/fe-syntax/queries/outline/ruby.scm:106, crates/fe-syntax/queries/outline/ruby.scm:114

Scope notes (triage, main 1e99299): F4, F12, F2, F7, F8, F16 and F17 are still true.
Dropped F14 as moot: `declared_name` already strips qualifiers and the `#member?` pattern makes `int Shape::count = 0;` Field "count" (golden cpp.cpp.symbols:35). Its remaining wrong classifications are F4's.
Fix in outline.rs and the cpp/lua/scala/swift/php/ruby queries listed in scope. Update the Lua expectation in `more_languages.rs` (`bindings_and_qualified_names`, F2), and regenerate only the affected `.outline`/`.symbols` goldens. Don't edit the samples. Put the new regressions in the new integration test `crates/fe-syntax/tests/more_languages_followups.rs`, cfg-gated like more_languages.rs. Waits for T134, which also edits outline.rs.
