---
task_uid: "01m42182wg4651s8hqe87r501g"
task: "T44"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T19

Findings of the review of T19 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/language.rs:430: Scala's shipped query uses @none to clear the enclosing string highlight inside interpolations, but fe-syntax discards those captures. Interpolated code therefore inherits string colouring; the new golden records e, op, n, expr and env as string text.
  fix: Support explicit highlight-clearing captures or add a provenance-documented Scala override that prevents interpolation code from inheriting string highlighting. Test both $identifier and ${expression}, then regenerate the golden.
  instances: crates/fe-syntax/src/language.rs:430, crates/fe-syntax/tests/golden/scala.scala.golden:170, crates/fe-syntax/tests/golden/scala.scala.golden:282, crates/fe-syntax/tests/golden/scala.scala.golden:471, crates/fe-syntax/tests/golden/scala.scala.golden:476, crates/fe-syntax/tests/golden/scala.scala.golden:478

- F2 medium crates/fe-syntax/src/language.rs:410: PHP's injection query captures heredoc_body and nowdoc_body without injection.include-children. The injection engine removes named children, including string_content and nowdoc_string containing the source, so labelled heredocs and nowdocs inject only gaps such as newlines. The golden's unknown TEXT label does not exercise this defect.
  fix: Use compatible, provenance-documented injection content captures that retain literal source while preserving PHP interpolation boundaries. Add heredoc and nowdoc tests using a shipped language label and assert injected token highlights.
  instances: crates/fe-syntax/src/language.rs:410

- F3 low crates/fe-syntax/tests/golden/xml.xml:37: The XML sample contains only 37 lines, below the spec's explicit 50–100-line requirement. The golden coverage assertion checks sample presence, so passing the gate does not establish this requirement.
  fix: Expand the original XML sample to 50–100 lines with meaningful additional syntax coverage and regenerate its golden.
  instances: crates/fe-syntax/tests/golden/xml.xml:37

## Implementation notes

- F1: `@none` is now honoured generally rather than with a Scala override:
  `Capture::highlight` is an `Option`, and a `@none` capture clears the
  highlights around it in `flatten`. This also fixes Kotlin's interpolations
  (its query uses `@none` the same way), so `kotlin.kt.golden` changes too.
  tree-sitter-md marks `code_fence_content` as `@none`; one pattern of ours
  after its query keeps fenced code `markup.raw`, as before, so the Markdown
  golden is unchanged. Tested by `tests::none_captures_clear_the_enclosing_highlight`
  (`$x` and `${…}`) and the flatten proptest, which now generates `@none`.
- F2: `queries/php-injections.scm` replaces the crate's injections query,
  capturing `string_content` / `nowdoc_string` pieces combined. Tested by
  `injection::tests::php_heredocs_and_nowdocs_inject_their_text` with `SQL`
  labels, and the PHP golden gains a labelled heredoc and nowdoc. The test
  lives in `src/injection/tests.rs`, next to the other injection tests.
- F3: the XML sample is 62 lines, adding DTD element, attribute-list,
  parameter/external entity and notation declarations, a prefixed namespace,
  mixed content and the predefined entity references.
