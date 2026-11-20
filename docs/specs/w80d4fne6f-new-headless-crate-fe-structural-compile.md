---
task_uid: "01m7wj2sah894ftyw80d4fne6f"
task: "T708"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`rust_call_pattern_compiles_with_two_single_metavariables` passes: `foo($A, $B)` gives root kind call_expression and metavars A, B"
    required: true
    manual: false
  - id: "C2"
    text: "`rust_method_chain_pattern_root_is_the_call_not_the_statement_wrapper` passes: `$X.unwrap()` has root call_expression, not source_file or expression_statement"
    required: true
    manual: false
  - id: "C3"
    text: "`python_and_go_patterns_compile_where_dollar_is_not_an_identifier_char` passes: `print($A)` (python) and `fmt.Println($A)` (go)"
    required: true
    manual: false
  - id: "C4"
    text: "`json_pattern_falls_back_to_a_string_placeholder` passes: `{\"name\": $V}` compiles against the json wasm fixture"
    required: true
    manual: false
  - id: "C5"
    text: "`pattern_with_a_syntax_error_reports_the_error_position_and_does_not_panic` passes"
    required: true
    manual: false
  - id: "C6"
    text: "`placeholder_text_never_collides_with_pattern_text` passes: a pattern that already contains `__tsMV_A__` is rejected or the placeholder is renamed"
    required: true
    manual: false
---

# New headless crate fe-structural: compile an ast-grep-style pattern into a tree pattern for a language, metavariables through placeholder tokens (structural search SS2) (Mac T2015)

# New headless crate fe-structural: compile an ast-grep-style pattern into a tree pattern for a language, metavariables through placeholder tokens (structural search SS2)

Design: the plan at /private/tmp/claude-501/-Users-daryl-code-fastedit/61585dba-8898-4006-bf18-ba245a480e31/scratchpad/structural-search-plan.md (copy its relevant sections into your reading; the plan is not in the repo, so each card restates what it needs). Do not widen --scope; if a file outside it is needed, stop and say so. Never run cargo builds beyond the crates named; gate tests affected crates only.

Create the crate `crates/fe-structural` (headless; `[lints] workspace = true`; deps: fe-syntax, tree-sitter, log, thiserror only if already in the workspace; add `fe-structural = { path = "crates/fe-structural" }` to root Cargo.toml [workspace.dependencies]). members = crates/* so no members edit.

`pattern.rs`: `Pattern::compile(language: &'static fe_syntax::Language, source: &str) -> Result<Pattern, PatternError>`. Steps: (1) scan source for `$NAME`, `$_`, `$$$NAME`, `$$$`, and `$$` (a literal dollar); NAME is [A-Z_][A-Z0-9_]* (ast-grep rule). (2) Replace each metavariable with a placeholder token and parse with `fe_syntax::parse_str` (SS1). Candidates in order: identifier `__tsMV_<NAME>__` (multi `__tsMVM_<NAME>__`; `$_` gets a unique numbered name), then a double-quoted string `"__tsMV_..__"`, then a number. Accept a candidate when the tree has no ERROR or MISSING node and every placeholder is exactly one leaf node whose text is the placeholder (record its kind). (3) Pattern root: from the tree root, descend while a node has exactly one named child and covers the same non-whitespace text span; stop at the innermost node covering all of the pattern text (so `$X.unwrap()` gives call_expression, a `fn f() {}` pattern gives function_item). (4) Store the pattern tree as an owned structure (kind ids, kind names, leaf text, named flag, child list, wildcard marker with name and single/multi), NOT a borrowed Tree, so it is Send + Sync and cheap to share between scan threads. `PatternError` carries a kind and a byte offset in the user's source (map back through the replacement offsets). Expose `Pattern::language()`, `root_kind()`, `metavariables() -> &[Metavar]`, `literals() -> Vec<&str>` (non-wildcard leaf texts, for SS7's prefilter).
Tests: new files crates/fe-structural/tests/pattern_rust.rs and pattern_langs.rs (names in the criteria). For the json case register the fixture from crates/fe-syntax/tests/fixtures/tree-sitter-json.wasm by relative path from CARGO_MANIFEST_DIR. Matching is NOT in this card.

## Helium filing note

- Lent from the Mac as y153, the user's request (structural search SS1-SS9 on a user-approved design: one
  language per search, injected languages skipped, `inside` the only relational rule; user docs help 1-2).
- linux-ok: headless tests. The macOS gate runs when the batch lands on the Mac.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Windows: imports and helpers that only `#[cfg(unix)]` tests use go under `#[cfg(unix)]` too, or Windows Clippy
  fails with -D warnings (Mac T2028). Linux gates can't catch this.
- Golden and pin tests: a new command changes typed_command_ids_tests.rs and command_catalog_tests.rs; a new
  setting changes the settings schema/template goldens, settings_model.rs and the l10n files; a new binding can
  break binding-count tests in fe-gpui. If one fails that isn't in scope, widen it with `rgt task respec --scope`
  (it replaces the list, so pass the whole list) before you finish, and say why in the spec's Implementation notes.
- Criteria tests need full cargo test paths (`module::tests::name`, or `fe-x::binary name` for an integration
  test whose name exists in several binaries).
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if anything in the criteria can't be met as
  written. Run rgt finish once, and wait for its result before you report.
- New crate fe-structural: add it to the workspace members and Cargo.lock only; check `cargo metadata --locked` before you finish.

## Implementation notes

- Each placeholder choice is parsed as written, then with `\n`, then with `;`
  after it: tree-sitter-rust has a MISSING `;` for a bare expression and
  tree-sitter-go a MISSING terminator (and a type-conversion parse) for a
  bare `fmt.Println(x)`, so C1-C3 can't pass under "no ERROR or MISSING"
  otherwise. The suffix is outside the span the root is chosen by.
- Kind and field names are `Arc<str>`: `Node::kind` borrows the tree and
  fe-syntax exposes no `'static` grammar.
- `$_NAME` is anonymous like `$_`, as in ast-grep; anonymous names are
  `_any0`, `_any1`, …
- Details: `docs/decisions/w80d4fne6f-new-headless-crate-fe-structural-compile-an.md`.
