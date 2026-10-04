---
task_uid: "01m417z4nbww38kvk6g5cqhjhg"
task: "T30"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test fold::tests::candidates_per_language: for a sample in each of the 16 languages, fold_ranges returns the expected row ranges (brace bodies end before their closing row; JSON objects and arrays; Markdown sections and fenced code; Python blocks); syntax_features stage passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test fold::tests::fold_range_at_prefers_the_header_then_innermost"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test tests::fold_candidates::fold_uses_candidates_then_indentation: a caret folds the candidate at its row (uneven brace block, Markdown section) and falls back to indentation without one; a multi-row selection folds exactly its rows"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test tests::fold_candidates::fold_all_and_unfold_all: fold_all folds every candidate (indented blocks when there are none) and moves folded-away carets to headers; unfold_all restores every row"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui tests::every_binding_does_what_it_should and tests::bindings_are_all_tested pass with cases for cmd-k cmd-0 and cmd-k cmd-j"
    required: true
    manual: false
---

# Fold by tree-sitter blocks, and add fold all and unfold all

Paddock T-25, part 1 of 2: fold candidates from tree-sitter, and the fold commands that use them (P3). Part 2 (T-25b) adds the gutter chevrons and
clicks.

## Problem

Fold (cmd-alt-[) folds the selected rows. With only a caret, it folds the indented block the caret
heads or sits in (`EditorModel::fold` → `indented_block`, `crates/fe-editor/src/lib.rs`). Unfold is
cmd-alt-]. Indentation works for most code. It fails for brace languages whose blocks aren't indented
consistently, and for Markdown sections, which aren't indented at all. There's no fold all or unfold
all command, though `DisplayMap::unfold_all` and `EditorModel::unfold_all` exist.

## Change

- **Candidates.** Add `crates/fe-syntax/src/fold.rs`, exported from `crates/fe-syntax/src/lib.rs`:
  - `fold_ranges(syntax: &Syntax, rows: Range<u32>) -> Vec<RangeInclusive<u32>>`: the row ranges of
    foldable nodes whose start row is in `rows`, sorted, spanning at least two rows. A fold starts at the
    node's first row and ends at its last row, or the row before it when that row holds only the closing
    delimiter, so a folded `{ … }` shows its `}`.
  - `fold_range_at(syntax, row) -> Option<RangeInclusive<u32>>`: the innermost candidate headed by `row`,
    else the innermost one containing it.
  - Foldable node kinds come from a table in `fold.rs` keyed by language name (`syntax.language().name()`),
    for all 16 languages in `language.rs`: blocks, bodies, declaration lists, objects/arrays, multi-line
    comments, and Markdown `section` and fenced code blocks. Use node kinds, not `folds.scm` files, so no new
    query files need licence audit and `language.rs` is left alone. Walk only the part of the tree
    overlapping `rows` (as highlighting does), using `Syntax::tree()`. gpui-kit's
    `extract_fold_ranges_in_range` (Apache-2.0) may be read for reference, per CLEAN_ROOM.md. Give any copied
    code its header and a `THIRD_PARTY.md` row; that file is a human path, so the lead adds the row.
  - With no tree (no language, over `MAX_PARSE_BYTES`, or before the first parse), there are no
    candidates.
- **Model** (`crates/fe-editor/src/lib.rs`, still syntax-free):
  - `fold` takes a candidate source, `&dyn Fn(u32) -> Option<RangeInclusive<u32>>`. For a caret, it uses
    the candidate at the caret's row and falls back to `indented_block` when there is none. A selection
    spanning rows still folds exactly those rows.
  - `fold_all(candidates: impl IntoIterator<Item = RangeInclusive<u32>>)` folds every candidate, and
    every indented block when the list is empty.
  - `pub fn fold_rows` / `unfold_rows(RangeInclusive<u32>)` for T-25b's clicks.
  - Carets folded away move to the fold's header, as `fold` does now.
- **UI** (`crates/fe-gpui/src/lib.rs`, `crates/fe-gpui/src/view.rs`): `FoldAll` and `UnfoldAll` actions,
  bound to cmd-k cmd-0 and cmd-k cmd-j. The `Fold` and `FoldAll` handlers pass `fe_syntax::fold_range_at`
  / `fold_ranges(.., 0..row_count)` over `view.syntax` when its version matches the buffer's. Add
  binding cases to `crates/fe-gpui/src/tests.rs`, which `tests::bindings_are_all_tested` requires.
- Model tests go in a new `crates/fe-editor/src/tests/fold_candidates.rs`. Declaring it in `tests/mod.rs`
  is a one-line touch outside scope.

## Decisions (the user, 2026-10-04)

- Fold all / unfold all use VS Code's chords, cmd-k cmd-0 and cmd-k cmd-j: the editor's first chord
  bindings.
- Fold all folds every candidate, nested ones included, even though an 8 MB file walks the whole tree
  (hundreds of ms, once, on a command the user asked for). Don't limit it to top-level candidates.

## Implementation notes

- **Languages.** `language.rs` now lists 27 languages, not 16, so `fold.rs`'s `KINDS` table has an entry
  for each, and `candidates_per_language` has a sample for every name `language_names()` returns. The
  test also checks that the samples cover every compiled language. `every_kind_is_in_its_grammar`
  catches a misspelt kind.
- **Closing rows.** A fold stops before its last row when that row *starts with* the node's closing
  delimiter (whitespace before it only). This is a little looser than "holds only": `} else {` and
  `});` rows stay visible too, so folding an `if` body doesn't hide its `else`. The text check also
  keeps `/* … */` rows whose text runs up to the `*/`. The delimiters are anonymous last children
  (`}`, `]`, `)`, `end`, `fi`, `done`, `endif`) plus `end_tag`, `ETag`, `jsx_closing_element` and
  Markdown's closing fence. Trailing blank rows are trimmed, so a Markdown section ends at its last text.
  A delimiter at the node's very start is an opening one left unclosed (`"```\n"`, `"/*\n"`) and
  isn't trimmed. Many grammars' comments have no child nodes; for those, the last row is checked for
  just `*/`, `-->`, `=end` or `]]` (`COMMENT_CLOSERS`).
- **Bodies after their header.** Python and Lua `block`, Scala `indented_block`/`indented_cases` and
  Swift `statements` start on the row after the header. They fold from the row where the node before
  them ends (the `:`, `then`, `=` or `{`), skipping comments (extras) in between. The walk looks
  past `rows` for them across up to 16 such comments (`MAX_EXTRAS`).
- **Go.** `type_declaration` and `const_declaration` are left out. A single `type S struct {` ends
  after its `}` but has no closer of its own, so folding it hid the `}`. `field_declaration_list` and
  the like cover those blocks.
- **Outside the listed scope** (small, needed):
  - `Syntax::text()` (`pub(crate)`, `syntax.rs`) reads the rows for the closing and blank checks.
  - `DisplayMap::fold_many` / `FoldMap::fold_many` (`fe-display`) add folds in one sort and one
    rebuild. Without them, fold all over n candidates cost O(n²).
  - Callers of the old `fold()` in `fe-editor/src/tests/mod.rs` and `fe-gpui/src/view/window_tests.rs`
    now pass `&|_| None`.
- **Cost.** Measured in release on an 8 MB Rust file: `fold_ranges` over the whole file took 163 ms
  (43k folds), a 60-row viewport 34 µs, and `fold_range_at` 2.5 µs. A 60-row viewport in the middle
  of a million-element JSON array took 45 µs. Candidates that start outside the rows asked for aren't
  measured, so an enclosing flat node isn't walked.
- No gpui-kit code was copied, so `THIRD_PARTY.md` needs no new row.
- `fe-gpui` `tests::folds_follow_the_syntax_tree` checks the wiring end to end with a Lua buffer that has
  no indentation. GPUI test notation reserves `{}`, so a brace language can't be used there.
