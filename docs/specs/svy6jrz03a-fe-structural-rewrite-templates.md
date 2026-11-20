---
task_uid: "01m7wj2wdk373a4rsvy6jrz03a"
task: "T711"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`swap_arguments_rewrites_foo_a_b_to_bar_b_a` passes: `foo($A, $B)` -\u003e `bar($B, $A)` over a Rust file with three calls gives three edits and the right result text"
    required: true
    manual: false
  - id: "C2"
    text: "`unwrap_becomes_question_mark_with_the_receiver_kept` passes: `$X.unwrap()` -\u003e `$X?`"
    required: true
    manual: false
  - id: "C3"
    text: "`multi_capture_substitutes_source_between_first_and_last_with_separators` passes: `foo($$$ARGS)` -\u003e `bar(0, $$$ARGS)`"
    required: true
    manual: false
  - id: "C4"
    text: "`multiline_capture_is_reindented_to_the_match_indentation` passes (tabs and spaces both)"
    required: true
    manual: false
  - id: "C5"
    text: "`dollar_dollar_in_a_template_is_a_literal_dollar` passes and `unknown_metavariable_in_a_template_is_an_error_with_a_position` passes"
    required: true
    manual: false
  - id: "C6"
    text: "`edits_are_in_file_order_non_overlapping_and_apply_to_the_same_text_as_a_sequential_rewrite` passes"
    required: true
    manual: false
  - id: "C7"
    text: "`rewrite_that_breaks_the_syntax_is_flagged_by_the_reparse_check_and_a_good_one_is_not` passes: introduces an ERROR node vs not"
    required: true
    manual: false
---

# fe-structural: rewrite templates, substituting captures with reindented source, edits in the Replacer::edits shape, and a reparse check (structural search SS5) (Mac T2018)

# fe-structural: rewrite templates, substituting captures with reindented source, edits in the Replacer::edits shape, and a reparse check (structural search SS5)

Design: the plan at /private/tmp/claude-501/-Users-daryl-code-fastedit/61585dba-8898-4006-bf18-ba245a480e31/scratchpad/structural-search-plan.md (copy its relevant sections into your reading; the plan is not in the repo, so each card restates what it needs). Do not widen --scope; if a file outside it is needed, stop and say so. Never run cargo builds beyond the crates named; gate tests affected crates only.

`Rewrite::compile(&Pattern, template: &str) -> Result<Rewrite, RewriteError>` (parse once into literal and capture parts; every `$NAME`/`$$$NAME` must be bound in the pattern; `$$` is `$`). `Pattern::edits(&self, rewrite: &Rewrite, source: &str, tree, opts, stop) -> Result<Vec<Edit>, ..>` where `Edit { range: Range<usize>, text: String, flagged: bool }` is the same shape as `fe_project::Replacer::edits` returns `(Range<usize>, String)`: also provide `Edit::into_pair`. Substitution: single capture -> its source text; multi -> the source slice from first captured start to last captured end (separators preserved), and for zero captures the empty string; a capture spanning several lines is reindented: strip the capture's own base indent (the indent of its first line's column in the file) and add the match start line's indent. Overlap rule: outermost first; once a match is taken, matches inside are dropped (nested option off).
Reparse check: after producing all edits for a file, apply them to a copy, `parse_str` the result, and count ERROR/MISSING nodes before and after; if the count rose, find the offending edits by applying them one at a time and set `flagged` on those (flagged edits are returned, the caller decides to skip). Bound this check: files over 1 MiB skip it (flagged=false, `checked: false` in the result). Tests in three new files; fast.

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

- `Pattern::edits` returns `Edits { edits, checked }` (the spec asks for
  `checked` in the result); see
  `docs/decisions/svy6jrz03a-fe-structural-rewrite-templates-substituting-captures-with.md`
  for the template, indentation, overlap and check rules.
