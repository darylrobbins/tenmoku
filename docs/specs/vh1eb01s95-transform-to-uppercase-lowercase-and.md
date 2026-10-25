---
task_uid: "01m5qnvzhreyc4b5vh1eb01s95"
task: "T1138"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "case_transform_tests::selection_upper_lower: editor.transform_uppercase / transform_lowercase replace each non-empty selection's text, across several selections, in one undo step, with selections left over the new text"
    required: true
    manual: false
  - id: "C2"
    text: "case_transform_tests::empty_selection_uses_word: with a bare caret inside 'fooBar' the word is transformed and the caret offset kept; between spaces nothing changes and no undo step is recorded"
    required: true
    manual: false
  - id: "C3"
    text: "case_transform_tests::title_case matches VS Code 1.143.1 TitleCaseAction (src/vs/editor/contrib/linesOperations/browser/linesOperations.ts, cited with that tag in the test): lowercase the whole text, then upper-case each WHOLE match of the regex (^|[^\\p{L}\\p{N}']|((^|\\P{L})'))\\p{L}, boundary character included. Expected strings written out: 'hELLO wORLD-foo_bar' becomes 'Hello World-Foo_Bar'; \"a\\u{345}b\" becomes \"A\\u{399}B\" (review F3: the U+0345 boundary is upper-cased too); \"don't\" stays \"Don't\""
    required: true
    manual: false
  - id: "C4"
    text: "case_transform_tests::unicode_lengths: 'straße' uppercases to 'STRASSE' and the selection covers all of it"
    required: true
    manual: false
  - id: "C5"
    text: "Catalog test lists updated; cargo nextest run -p fe-editor -p fe-workbench and clippy pass"
    required: true
    manual: false
---

# Transform to Uppercase, Lowercase and Title Case commands, as VS Code's editor.action.transformTo*

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Transform to Uppercase, Transform to Lowercase and Transform to Title Case (editor.action.transformToUppercase, transformToLowercase, transformToTitlecase; no default keys) are missing. Verified: no such command in fe-editor; fe-vim has its own gU/gu operators (not reachable outside vim and not the same commands).

## Current state
- Editor commands register through `add(registry, name, title, true, ..)` (`crates/fe-editor/src/rows.rs:25-60` is the pattern). Multi-selection edits go through one transaction (`multi_cursor.rs`).
- Word-at-caret logic exists for double-click / ctrl-d selection (`crates/fe-editor/src/occurrences.rs`).

## Change
- New `crates/fe-editor/src/case_transform.rs` (+ `mod` line in `lib.rs`, `register` call in `commands.rs`): `editor.transform_uppercase` ("Transform to Uppercase"), `editor.transform_lowercase` ("Transform to Lowercase"), `editor.transform_titlecase` ("Transform to Title Case").
- VS Code semantics: each non-empty selection is replaced by its transformed text; an empty selection transforms the word at the caret (nothing if the caret is not in a word) and leaves the caret where it was; uppercase uses full Unicode case mapping (so `ß` becomes `SS`); title case upper-cases the first letter of each word and lower-cases the rest, words split at whitespace and punctuation. A selection whose text does not change makes no edit. One transaction, one undo step; selections stay over the transformed text (length may change).
- No default keys.

## Tests
New `crates/fe-editor/src/case_transform_tests.rs`; update `command_catalog_tests.rs` and `typed_command_ids_tests.rs`.

## Implementation notes

- Title case follows VS Code's current `titlecase` expression (lower-case the text, then upper-case a letter at the start or after anything but a letter, digit or apostrophe), so `hELLO wORLD-foo_bar` gives `Hello World-Foo_Bar`: `_` starts a word. Title case matches VS Code 1.143.1 `TitleCaseAction` exactly: each whole regex match, boundary character included, is upper-cased.
- Overlapping targets (a selection and a caret in the same word) are merged and transformed once.
