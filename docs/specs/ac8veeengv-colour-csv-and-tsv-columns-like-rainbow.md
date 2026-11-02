---
task_uid: "01m6c9qmsjv8wp9qac8veeengv"
task: "T1300"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-grammar-csv holds grammar.js and generated ABI-15 csv and tsv parsers, with a README saying how to regenerate them"
    required: true
    manual: false
  - id: "C2"
    text: "x.csv is csv; x.tsv and x.tab are tsv"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-syntax/tests/csv_columns.rs: in a,,\"c,d\",e the fields a, \"c,d\" and e take the highlights of columns 0, 2 and 3; a quoted field with an embedded newline keeps the column count; column 17 is plain; the same holds for tsv"
    required: true
    manual: false
  - id: "C4"
    text: "Goldens csv.csv and tsv.tsv exist; each_language_highlights_a_snippet, candidates_per_language, every_listed_language_has_tokens and every_language_has_an_entry pass; language_label gives CSV and TSV"
    required: true
    manual: false
  - id: "C5"
    text: "cargo deny check licenses bans sources passes (the RUSTSEC advisories already on main are T1301's); cargo nextest run -p fe-syntax -p fe-gpui passes"
    required: true
    manual: false
---

# Colour CSV and TSV columns like Rainbow CSV with our own small column grammar

## Problem

CSV and TSV files open as plain text. The user asked (2026-11-01) for per-column colours, as VS Code's
Rainbow CSV gives, if a grammar or a cheap highlighter allows it.

fe-syntax highlights only through tree-sitter queries (`Syntax::highlights`), so a non-tree-sitter highlighter
would be a new seam through fe-gpui. The upstream grammar can't do it: amaanq/tree-sitter-csv (crates.io
`tree-sitter-csv` 1.2.0, MIT) needs tree-sitter ~0.20, its unquoted `text` token
`[^,\d\s"][^, \n\r"]+` rejects fields with spaces and one-character fields, and `repeat(separator)` drops
empty fields, so no query can count columns from its tree.

## Change

Write our own small grammar, `crates/fe-grammar-csv` (our licence), for two dialects, `csv` (`,`) and `tsv`
(tab), from one `grammar.js` factory. Generate `src/parser.c` for each with tree-sitter CLI 0.25 (ABI 15,
`npx tree-sitter-cli@0.25 generate`) and commit the generated files; a `cc` build.rs; `LANGUAGE_CSV` and
`LANGUAGE_TSV` via `tree-sitter-language`. Every column must be a named node, so a column's index is its
position among the row's children: a row is `field*` then an optional last field, where each `field` is its
optional content (quoted `"…"` with `""` escapes and embedded newlines, else anything but the separator,
quote or newline) followed by the separator. `a,,c` is three children; `,b` is two.

Highlight query `crates/fe-syntax/queries/csv.scm` (shared by both): anchored patterns
`(row . (field (content) @c0))`, `(row . (field) . (field (content) @c1))`, … for the first 16 columns,
the capture names cycling through 8 highlight names from our vocabulary (`keyword`, `string`, `number`,
`function`, `type`, `constant`, `property`, `variable.parameter`), so the colours come from the theme.
Columns past 16 are plain. The separators stay plain.

Two languages, `csv` (extension `csv`) and `tsv` (extensions `tsv`, `tab`), one feature `csv`, on by
default. No comment tokens; auto pairs `""` only. Fold nothing. Labels `CSV` and `TSV` in `language_label`
(`crates/fe-gpui/src/pane_item.rs`). Goldens `tests/golden/csv.csv`, `tsv.tsv`. Expected size: under 10 KB
of object code for both. Record the grammar as ours in `THIRD_PARTY.md` (nothing third-party).

## Implementation notes

- `content` is wider than the spec's "anything but the separator, quote or newline", on malformed text only. With the literal token, a stray or unclosed quote (`5" disk,x`) was a parse error that left every later row of the file plain, and `a"b"c,d` shifted the columns after it. So an unquoted field may hold a quote after its first character, a quoted field may have text after its closing quote, and a quote never closed ends at the separator. Well-formed fields parse as the spec says. `tests/shape.rs` in the grammar crate checks that no string of up to 5 characters of `a , tab " newline space` is a parse error.
- `tree-sitter.json` is there because CLI 0.25 falls back to ABI 14 without it. Generate from `csv/` and `tsv/`, as the README says.
- Object code at `-O2` on arm64 is about 2 KB per dialect, 4 KB for both.
