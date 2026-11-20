---
task_uid: "01m7wj2vcqb18paehvkbb5en52"
task: "T710"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`multi_wildcard_matches_zero_one_and_many_arguments` passes: `foo($$$ARGS)` on `foo()`, `foo(a)`, `foo(a, b, c)`"
    required: true
    manual: false
  - id: "C2"
    text: "`multi_wildcard_in_the_middle_backtracks` passes: `foo($A, $$$MID, $Z)` on `foo(1,2,3,4)` gives A=1, MID=2,3, Z=4"
    required: true
    manual: false
  - id: "C3"
    text: "`multi_wildcard_captures_cover_from_first_to_last_node_including_separators` passes (the range text is `2, 3`)"
    required: true
    manual: false
  - id: "C4"
    text: "`statement_list_multi_wildcard_matches_a_block_body` passes: `if $C { $$$BODY }` in Rust"
    required: true
    manual: false
  - id: "C5"
    text: "`where_kind_constraint_filters_by_node_kind` and `where_regex_constraint_filters_by_text` and `where_not_constraint_inverts` pass"
    required: true
    manual: false
  - id: "C6"
    text: "`where_inside_requires_an_ancestor_of_the_kind` and `where_not_inside_excludes_it` pass: `$X.unwrap()` where not inside function named test"
    required: true
    manual: false
  - id: "C7"
    text: "`constraint_on_an_unbound_metavariable_is_a_compile_error_with_a_position` passes"
    required: true
    manual: false
  - id: "C8"
    text: "`pathological_multi_wildcards_stay_polynomial` passes: `foo($$$A, $$$B, $$$C)` over a 200-argument call returns within the stop callback bounds"
    required: true
    manual: false
---

# fe-structural: $$$ multi-wildcards over sibling runs with backtracking, $_ anonymous, and where constraints (kind, regex, text, not, inside) (structural search SS4) (Mac T2017)

# fe-structural: $$$ multi-wildcards over sibling runs with backtracking, $_ anonymous, and where constraints (kind, regex, text, not, inside) (structural search SS4)

Design: the plan at /private/tmp/claude-501/-Users-daryl-code-fastedit/61585dba-8898-4006-bf18-ba245a480e31/scratchpad/structural-search-plan.md (copy its relevant sections into your reading; the plan is not in the repo, so each card restates what it needs). Do not widen --scope; if a file outside it is needed, stop and say so. Never run cargo builds beyond the crates named; gate tests affected crates only.

Implement multi-wildcards (`__tsMVM_` placeholders in a list-like child sequence: match a run of zero or more siblings, lazily, backtracking against the next non-wildcard pattern sibling; memoise (pattern idx, node idx) to keep it polynomial; two multi-wildcards in a row are allowed (first lazy). Captured range = first to last matched sibling's span (empty at the insertion point for zero matches; record the offset). `$$$` alone is an unnamed multi. `$_` single anonymous: matches one node, no binding.
Constraints: extend `Pattern::compile` input with the `where` section from the plan: lines after a line `where` (the part before is the pattern; a `where` line is only recognised when it is alone on its line). Grammar: `$NAME: kind=<k>` | `regex=<re>` | `text=<s>`, each optionally `not`; `inside: kind=<k>` and `not inside: kind=<k>` (any ancestor), optionally `field`-less. Use the `regex` crate (check Cargo.lock; add via workspace dep if absent, MIT/Apache); compile once. Unknown key or unbound NAME -> PatternError with the offset. Evaluate constraints at bind time (kind/regex/text) and after a match (inside).
Keep matcher.rs edits to the multi hook; put new logic in multi.rs and constraints.rs. Tests in the two new files named in the criteria.

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

- See `docs/decisions/hvkbb5en52-fe-structural-multi-wildcards-over-sibling-runs.md`.
- Scope was widened by the lead (not by this lane) with
  `crates/fe-structural/Cargo.toml` and `Cargo.lock` (the `regex`
  dependency, `{ workspace = true }`; the lock changes only fe-structural's
  dependency list), `src/pattern.rs` (`Pattern` stores the compiled `where`
  section and `compile` splits it off; the carried T709 F1 fix, a
  `PatternNode::extra` flag), `src/error.rs` (new error kinds),
  `src/captures.rs` and `tests/match_rust.rs`.
- `captures.rs`: per the spec, a multi metavariable captures one range, first
  to last sibling; its doc said one range per node, and now matches.
- `tests/match_rust.rs`: T709's `a_multi_metavariable_is_unsupported_for_now`
  is deleted, with `Stopped::Unsupported`; C1 covers that case now.
- Carried T709 F1 (pattern trivia by kind name) is fixed in pattern.rs and
  matcher.rs; F2 (cancellation not asked inside a candidate) in matcher.rs
  and multi.rs. Tests for both are in match_multi.rs.
- C6's "function named test" needs more than `inside: kind=<k>`: `inside`
  takes an optional ` name=<text>` (the ancestor's `name` field).
- Review round 1: F1 (backtracking across lists) is fixed by a
  continuation-passing search. F2 (the bound with repeated names) is fixed
  by a sound global memo plus a compile error, `VaryingRepeats`, for more
  than one varying repeated name. F3 (trailing spaces in values) is fixed.
  The carried T708 F1 (placeholder search) and F2 (suffix in the tree) are
  fixed in pattern.rs too, with a new `PatternNode::open_end`. Details are
  in the decision doc.
