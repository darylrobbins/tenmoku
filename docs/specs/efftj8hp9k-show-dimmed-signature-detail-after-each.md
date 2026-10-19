---
task_uid: "01m57yae59z65weaefftj8hp9k"
task: "T197"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "OutlineItem.detail holds the text after the name, capped at 80 chars, empty for headings; a name-only capture whose suffix lies outside the allowlisted ancestor kinds (Swift property types and enum payloads, Scala typed patterns, C/C++ bit-fields) may have an empty detail (documented known limit)"
    required: true
    manual: false
  - id: "C2"
    text: "TreeRow draws the detail dimmed after the label, ellipsized before the badge"
    required: true
    manual: false
  - id: "C3"
    text: "Outline rows carry their detail, proved in the new test files"
    required: true
    manual: false
---

# Show dimmed signature detail after each symbol name in the Outline panel (Mac T991)

## Problem
VS Code's Outline shows dimmed detail (a function's signature, a field's type) after each symbol name. fastedit's rows show the name only: `OutlineItem` (crates/fe-syntax/src/outline.rs:99-115) has no detail, and `render_rows` (crates/fe-workbench/src/outline_panel.rs:301) passes only `item.name`. `TreeRow` has a `badge` at the row's right end (kiln/crates/kiln-ui/src/tree_row.rs:120) but no dimmed text directly after the label.
## Change
1. fe-syntax: add `detail: String` to `OutlineItem`, filled by `OutlineJob::run` (and the symbols path) from the source text between the end of the name and the first `{`, `;` or `=` or end of the item's first line, whitespace-collapsed, cut at 80 chars; empty for headings and when nothing follows. Cover Rust, TypeScript, Python, Go, Java, C at least where their queries exist.
2. kiln-ui: add `TreeRow::detail(text, color)` drawn right after the label in the dimmed list colour, ellipsizing before the badge; the label never shrinks below its own text.
3. outline_panel.rs: set `.detail(&item.detail, ..)` from the theme's dim colour.
Tests in new files prove the detail strings and that rows carry them.

## Implementation notes

- The detail is read from the tree's tokens after the name, not the raw text: strings and other literals are whole tokens and comments are skipped, so brackets, `=` or `{` inside them count for nothing (review F1).
- It ends at the item's `body` field (and the `:` before Python's) or a Go field's `tag`, or, outside brackets, at `;`, `=`, a line break, or a `{` that doesn't open a type (TypeScript's `object_type`). A Rust tuple struct's or variant's `(u32, u32)` body field is kept as the detail. `<`/`>` count as brackets only in type parameter and argument lists. A comma after a whole item is part of its detail, as Ruby's `def f a, b`.
- Nothing is rewritten: commas are kept, so a multi-line list reads `(a: u32, b: u32,)` and `(u32,)` stays a tuple (F3). A closer of a bracket opened before the name is left out, so C's `int (twice)(int x)` reads `(int x)` (F4).
- An item captured as its name alone reads on only through the ancestor kinds listed for its language in `NAME_DECLARATIONS`: Go's `field_declaration`, Scala's `identifiers` and val/var definitions and declarations, C's and C++'s `array_declarator`s (review F2, F5). Its comma skips names of its own kind, so Go's `X, Y int` and Scala's `val x, y: Int` give each name the type, and ends at anything else. Other name-only items (an enum's last variant, destructured names, a C declarator beside another) have no detail. A parent with over 256 children isn't entered and at most 256 tokens are read, which keeps a long Lua `a1, a2, … = …` linear.
- Besides headings, keys and tables of JSON, YAML and TOML have no detail.
- `TreeRow::detail` with an empty text draws nothing. With a detail the label takes its own text's width (flex basis auto) and the detail grows from zero into the rest, so the detail gives way first; a label wider than the whole row still ellipsizes. `TreeRow::detail_text` reads it back for host tests.
- The panel colours the detail `cx.theme().colors.text.muted`.
- Known limit (C1, rev 2; follow-up T205): a name-only capture whose suffix lies outside the `NAME_DECLARATIONS` kinds has an empty detail, as Swift's `var x: Int` and `case a(Int)`, Scala's typed pattern `val Some(x: Int) = opt`, and a C/C++ bit-field `unsigned x:3`.
- The token walk is iterative, on a tree-sitter cursor, so a deeply nested default such as `1+1+…` can't overflow the stack (rev 2 review F7). A comment between tokens counts as whitespace, and a line break before, in or after it as a line break: `impl /*note*/Trait` reads `impl Trait` (F8). No space comes before a comma.
