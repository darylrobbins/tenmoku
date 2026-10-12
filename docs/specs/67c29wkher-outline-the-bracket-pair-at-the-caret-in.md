---
task_uid: "01m4r0dvcweq4kxx67c29wkher"
task: "T28"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test element::bracket_tests::outlines_both: a caret before the ( of f(a) paints two rects, at ( and ); typing inside so that the pair changes paints the new pair on the next frame after the reparse, never the old one"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test element::bracket_tests::nothing_to_outline: no rects with a non-empty selection, with two carets, in plain text, or with the caret not at a bracket; with the ) scrolled out of view only the ( is outlined"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui (the golden screenshots unchanged) and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Outline the bracket pair at the caret in the editor (Mac T461)

Filed on helium from Mac T461 (~/coordination/helium-cards/T461.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-k.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

With `fe_syntax::bracket_pair` (previous card) the editor can find the bracket pair at the caret, but nothing shows
it. Every mainstream editor outlines the pair so nested code can be read at a glance (read path first, plan line 4;
M6 "Editing polish", line 110).

## Change

In `crates/fe-gpui/src/element.rs` only:

- In prepaint, when the primary selection is a caret, the view has a syntax whose tree is at the model's current
  version (`Syntax::version` equals the snapshot's), and `bracket_pair(syntax, head)` gives a pair, compute each
  bracket's rect on visible rows, as `match_rects` (:1246) does for find matches. A bracket on a row that isn't
  visible, or inside a fold, gets no rect; the other is still drawn.
- Paint each rect as a 1 px outline (no fill) in the theme's `cursor` colour at 50 % alpha, over the selection and
  find-match fills and under the text. No new theme key.
- Nothing while the selection isn't empty, with several selections (only the primary caret counts), in plain text,
  or while the tree lags the text (the next frame after the reparse draws it).
- Record the painted rects for tests (`#[cfg(test)]` field next to `painted_selections`).

Not in this card: a go-to-matching-bracket command.

## Tests

New file `crates/fe-gpui/src/element/bracket_tests.rs`, declared from element.rs with `#[path]` beside its existing
tests module.
