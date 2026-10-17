---
task_uid: "01m557xc5atxpad157gf2tzp1e"
task: "T140"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-editor::indent_rows_tab_stops indent_and_outdent_go_to_the_next_or_previous_indent_stop (tests/indent_rows_tab_stops.rs, top level) passes: with tab size 4 and Spaces(4), indent_rows over a\\n  b\\n c\\n    d gives \"    a\\n    b\\n    c\\n        d\" and outdent_rows over \"      a\\n  b\\n    c\\n\\t  d\" gives \"    a\\nb\\nc\\n    d\"; with Tabs, indent of \"a\\n  b\\n\\t  c\" gives \"\\ta\\n\\tb\\n\\t\\tc\" and outdent of \"\\t  a\\n\\t\\tb\" gives \"\\ta\\n\\tb\"; with Spaces(2), indent of \"  a\\n\\n  b\" leaves the blank row and gives \"    a\\n\\n    b\"; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Only the seven assertions the spec lists change, to the results it gives: five in rows_tests.rs (rows::tests::indent_and_outdent) and two in indent_style_tests.rs (indent_style::tests::editing_follows_it); every other fe-editor test passes as is, tests/indent_style_bounds.rs and the single-caret EditorModel::tab included"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-editor passes with no other existing test edited; cargo clippy -p fe-editor --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Indent and Outdent move each row's indentation to the next or previous indent stop, as VS Code's Indent Line and Outdent Line do, instead of adding or removing a whole indent unit (Mac T788)

## Problem

`EditorModel::indent_rows` and `outdent_rows` (`crates/fe-editor/src/rows.rs`), behind Indent, Outdent
and Tab/Shift-Tab over several rows, insert or remove a whole indent unit at each row's start, whatever
the row's indentation is. VS Code's Indent Line and Outdent Line (`ShiftCommand`,
src/vs/editor/common/commands/shiftCommand.ts, with the default `editor.useTabStops`) move each row's
indentation to the next indent stop (`nextIndentTabStop`) or the previous one, and write the result in
the editor's style: spaces, or as many tabs as fit plus spaces. fastedit's own single-caret `tab`
already goes "up to the next multiple of the indent width" (`EditorModel::tab`, lib.rs), so rows and
carets disagree.

Reproduced at Mac main 3e3eaa03 with a new integration test (tab size 4): with `Spaces(4)`, indenting
`a\n  b\n c\n    d` gives `    a\n      b\n     c\n        d` (VS Code: `    a\n    b\n    c\n        d`);
outdenting `      a` gives `  a` (VS Code: `    a`) and `\t  d` gives `  d` (VS Code: `    d`); with
`Tabs`, indenting `  b` gives `\t  b` (VS Code: `\tb`) and outdenting `\t  a` gives `  a` (VS Code: `\ta`).
Code with an odd indentation never lines up again however often it is shifted.

## Change

Replace both bodies with one `shift_rows(indent: bool)` in rows.rs. For each touched row, measure the
leading spaces and tabs as display columns (tabs to `tab_size` stops); the new width is the next multiple
of the indent width for indent (`IndentStyle::Spaces(n)` gives n, `Tabs` and no style give `tab_size`)
and the previous multiple for outdent (0 stays 0, no edit). Write it as `indent_unit` would: spaces, or
with `Tabs` `width / tab_size` tabs then the remainder in spaces. Keep today's rules otherwise: indent
skips blank rows, outdent leaves a row with no indentation alone, and when the new indentation only adds
to or only cuts the end of the old one the edit is that insertion at the row's start or that cut, so
carets and selection ends at a row's start move as they do now; otherwise the whole indentation is
replaced and a caret inside it goes to the row's start (`map_offset`). Out of scope: VS Code's
indentation rules from `onEnterRules`, indenting a lone blank row, and keeping a selection's start in
column 1.

Seven existing assertions pin the whole-unit shift and change to VS Code's results, and no others:
in `crates/fe-editor/src/rows_tests.rs` (`rows::tests::indent_and_outdent`) indent of
`[a\n\n  b\n]c` → `    [a\n\n    b\n]c`, outdent of `    [a\n\n      b\n]c` → `[a\n\n    b\n]c`, outdent of
`[  a\n\t\tb\nc]` → `[a\n    b\nc]` (no style writes spaces), outdent of `    a\n\t  |b` → `    a\n    |b`
and `ran("  a|b", indent_rows)` → `    a|b`; in `crates/fe-editor/src/indent_style_tests.rs`
(`indent_style::tests::editing_follows_it`) `Tabs` outdent of `      |a` → `\t|a` and no-style outdent of
`      |a` → `    |a`. Test in a new `crates/fe-editor/tests/indent_rows_tab_stops.rs`. A prototype of this
passed all of `cargo test -p fe-editor` with those seven edits.
