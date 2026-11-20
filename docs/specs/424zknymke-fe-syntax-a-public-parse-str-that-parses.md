---
task_uid: "01m7wj2q7sm5xf76424zknymke"
task: "T706"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "`parse_str_rust_and_python_give_trees_with_the_expected_root_kinds` passes"
    required: true
    manual: false
  - id: "C2"
    text: "`parse_str_stops_when_the_callback_says_so_and_returns_none` passes"
    required: true
    manual: false
  - id: "C3"
    text: "`parse_str_parses_a_registered_wasm_grammar` passes (fixture crates/fe-syntax/tests/fixtures/tree-sitter-json.wasm, register_language as the existing wasm tests do)"
    required: true
    manual: false
  - id: "C4"
    text: "`parse_str_returns_none_for_a_language_with_no_grammar_set_error_free` passes: a Language whose set_language fails gives None, never a panic"
    required: true
    manual: false
---

# fe-syntax: a public parse_str that parses a &str in a language on the calling thread, wasm included, under a stop callback (structural search SS1) (Mac T2014)

# fe-syntax: a public parse_str that parses a &str in a language on the calling thread, wasm included, under a stop callback (structural search SS1)

Design: the plan at /private/tmp/claude-501/-Users-daryl-code-fastedit/61585dba-8898-4006-bf18-ba245a480e31/scratchpad/structural-search-plan.md (copy its relevant sections into your reading; the plan is not in the repo, so each card restates what it needs). Do not widen --scope; if a file outside it is needed, stop and say so. Never run cargo builds beyond the crates named; gate tests affected crates only.

Add `pub fn parse_str(language: &Language, text: &str, out_of_time: impl FnMut() -> bool) -> Option<tree_sitter::Tree>` in new `crates/fe-syntax/src/parse_str.rs`, exported from lib.rs (one `mod` and one `pub use` line). It uses `runtime::ThreadParser::for_language` and the existing `syntax::parse_text` (see syntax.rs ~1075, which wants a `fe_text::Text`; build one with `Text::new(text)`), so wasm grammars work and the grammar deadlines and panic handling apply unchanged. Also export `pub fn tree_sitter_language(&self) -> &tree_sitter::Language` on `Language` ONLY if the engine needs it (SS3 does not; skip it). Do not change existing behaviour. Tests in the new file crates/fe-syntax/tests/parse_str.rs; look at how other tests in crates/fe-syntax/tests register the json wasm fixture. Keep tests fast (no long soak).

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
