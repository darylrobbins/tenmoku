---
task_uid: "01m7wj2tbdn789878kam4wzgkg"
task: "T709"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`rust_unwrap_pattern_finds_every_unwrap_call_in_a_file` passes and gives byte ranges that slice to the exact call text"
    required: true
    manual: false
  - id: "C2"
    text: "`match_ignores_whitespace_and_comments_between_tokens` passes: `foo(1,2)` matches `foo( 1, /* c */ 2 )`"
    required: true
    manual: false
  - id: "C3"
    text: "`metavariable_binds_the_whole_subexpression` passes: `foo($A)` on `foo(a.b(c))` captures `a.b(c)`"
    required: true
    manual: false
  - id: "C4"
    text: "`repeated_metavariable_requires_equal_source_text` passes: `$A == $A` matches `x == x` and not `x == y`"
    required: true
    manual: false
  - id: "C5"
    text: "`python_and_js_patterns_match_with_their_own_grammars` passes (match_python.rs, match_js.rs)"
    required: true
    manual: false
  - id: "C6"
    text: "`nested_matches_are_reported_outermost_first_and_not_overlapping_by_default` passes, and `nested_matches_option_reports_inner_ones_too` passes"
    required: true
    manual: false
  - id: "C7"
    text: "`cancel_callback_stops_a_match_over_a_large_generated_file_promptly` passes: a 20k-line generated source with the callback set returns early with Cancelled"
    required: true
    manual: false
---

# fe-structural: match a compiled pattern against a tree: kind and leaf-text comparison, trivia skipped, single metavariables bound, repeated names compared (structural search SS3) (Mac T2016)

# fe-structural: match a compiled pattern against a tree: kind and leaf-text comparison, trivia skipped, single metavariables bound, repeated names compared (structural search SS3)

Design: the plan at /private/tmp/claude-501/-Users-daryl-code-fastedit/61585dba-8898-4006-bf18-ba245a480e31/scratchpad/structural-search-plan.md (copy its relevant sections into your reading; the plan is not in the repo, so each card restates what it needs). Do not widen --scope; if a file outside it is needed, stop and say so. Never run cargo builds beyond the crates named; gate tests affected crates only.

`Pattern::find_all(&self, source: &str, tree: &tree_sitter::Tree, opts: &MatchOptions, stop: &mut dyn FnMut() -> bool) -> Result<Vec<Match>, Stopped>`. `Match { range: Range<usize>, captures: Captures }`, `Captures` maps metavariable name -> Vec<Range<usize>> (one for single, several for multi in SS4). Algorithm per the plan: DFS the file tree, test nodes whose kind id equals the pattern root's; `matches(pat, node)`: same kind; named leaf compares text; unnamed token compares kind; wildcard binds, or compares to the earlier binding's source text; children compared pairwise skipping `is_extra()` nodes (comments); a different child count is a mismatch (multi-wildcards come in SS4: for now `$$$` compile-time accepted but find_all returns Err(Unsupported) if a pattern holds one). `MatchOptions { nested: bool }`. Check `stop` every 1024 visited nodes. The source is parsed by the caller with `fe_syntax::parse_str`; add a convenience `Pattern::find_in_text(&self, text)` that parses and finds for tests and for SS7.
Do not compare tree-sitter field names. Language quirks to cover in tests: Rust's `foo!(..)` macro bodies are token trees (leaf compare by text; fine), Python's `print($A)` vs `print(x, y)` (arg-count mismatch is correct here; SS4 gives `$$$`). Tests in the four new files named in the criteria; one fn per behaviour; short sources.

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

## Implementation notes

- See `docs/decisions/8kam4wzgkg-fe-structural-match-a-compiled-pattern-against.md`.
- `Stopped` is an enum: `Cancelled`, `Unsupported` (any `$$$`), `ParseFailed`
  (only from `find_in_text*`). `find_in_text_with(text, opts, stop)` sits
  beside the spec's `find_in_text(text)` for SS7 and the nested test.
- `PatternNode` has no extra flag and `pattern.rs` is out of scope: pattern-side
  comments are known by kind name (`*comment*`), a documented known limit.
- C5's test name exists in two binaries (match_python.rs, match_js.rs), so the
  criteria file names both as `fe-structural::<binary>.<name>`.
