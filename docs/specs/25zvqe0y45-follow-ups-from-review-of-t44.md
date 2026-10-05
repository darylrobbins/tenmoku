---
task_uid: "01m43s4qp4yb2kat25zvqe0y45"
task: "T52"
rev: 4
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T44

Findings of the review of T44 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/queries/php-injections.scm:18: Capturing only string_content drops the anonymous newlines between heredoc text nodes. Combined included ranges concatenate those lines: SQL containing `SELECT a -- comment\nFROM t` treats FROM as part of the comment, and newline-sensitive languages also parse incorrectly. The new heredoc test contains only one content line.
  fix: Preserve literal line separators in the injection ranges while excluding PHP interpolations. Add multiline heredoc regression tests covering SQL line comments and a newline-sensitive shipped language.
  instances: crates/fe-syntax/queries/php-injections.scm:18

- F2 medium crates/fe-syntax/src/highlight.rs:171: Flatten discards explicit @none regions, making them indistinguishable from uncaptured bytes when injection highlights are overlaid. The overlay restores the host highlight there. Consequently, Scala inside a PHP nowdoc labelled SCALA still colours cleared interpolation identifiers as PHP string text, leaving the original defect unresolved for injected Scala.
  fix: Carry explicit clearing regions through highlight querying, clipping and injection overlays so they suppress host highlights while ordinary uncaptured bytes retain the existing fallback. Add injected Scala and Kotlin interpolation regressions.
  instances: crates/fe-syntax/src/highlight.rs:171

## Implementation notes

- F1: a heredoc's scanner makes no node of the line break ending each line, so
  the gaps were lost. A new `injection.include-gaps` pattern property (ours)
  makes a combined injection also take the parent's bytes that no child
  covers; `php-injections.scm` sets it on heredocs and nowdocs. Adjacent
  ranges are now merged as they're collected. Regressions:
  `php_heredoc_line_breaks_end_sql_comments` (SQL `--` comments) and
  `php_heredocs_keep_lines_for_yaml` (YAML), in `injection/tests.rs`, plus a
  multi-line commented heredoc in the PHP golden sample.
- F2: `HighlightSpan` takes a highlight type parameter, defaulting to
  `Highlight`. `flatten` and `highlight_query` return `Painted` spans
  (`Option<Highlight>`), with `@none` runs as `None`; `clip` and `overlay` are
  generic, so cleared runs in a layer override the host. `Syntax::highlights`
  drops them before caching. Regression: `tests/injected_clears.rs` checks
  Scala and Kotlin in PHP nowdocs highlight their interpolations as they do
  on their own.
