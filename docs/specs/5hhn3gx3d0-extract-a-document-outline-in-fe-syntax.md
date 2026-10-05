---
task_uid: "01m46r9w5vkgvr9x5hhn3gx3d0"
task: "T64"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax integration test tests/outline.rs::outlines_match_golden_files: every sample in tests/outline_samples/ (rust, python, javascript, typescript, tsx, go, java, c, markdown, toml) is parsed whole and its outline matches \u003csample\u003e.outline (kind, name, item range, two-space indent per depth); every language for which outline_supported is true has a sample; CARGO_MANIFEST_DIR is read at run time"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax integration test tests/outline.rs::rust_items_nest_under_impls_traits_and_modules: in a Rust sample, a mod holds its struct, enum and trait; impl Display for Foo is an Impl named \"impl Display for Foo\" holding its fn as a Method; a trait fn signature is a Method under the Trait; a free fn is a Function; const, static and macro_rules! items are listed; a function captured by both the method and function patterns appears once"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax integration test tests/outline.rs::python_and_typescript_methods_nest_in_classes: Python methods (plain and decorated) are Methods under their Class, a nested def is a Function under its function, and a module-level ALL_CAPS assignment is a Constant; TypeScript and TSX list interface, type alias, enum, class with its methods, and an exported const arrow function as a Function"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax integration test tests/outline.rs::markdown_headings_nest_by_level: for # A, ## B, ### C, ## D, # E (ATX) plus a setext level-2 heading, items are Heading(level) named without markers or trailing #s, nested A \u003e B \u003e C, A \u003e D and E \u003e the setext heading; a # line inside a fenced code block is not listed"
    required: true
    manual: false
  - id: "C5"
    text: "fe-syntax integration test tests/outline.rs::items_are_preorder_and_ranges_nest: for every sample, items are in pre-order by range start, each child range lies within its parent range, name_range lies within range, depth is the parent depth + 1 (0 at the top), row is the row of name_range.start, and item_at(offset) gives the innermost item containing offset and None outside every item"
    required: true
    manual: false
  - id: "C6"
    text: "fe-syntax integration test tests/outline.rs::no_outline_without_a_full_tree: JSON gives Err(Unsupported); Rust text of parse_limit()+1 bytes gives Err(TooLarge), also after a window_job parse has finished; a Rust buffer before its first parse, and after an edit before reparsing, gives Err(NotParsed); after reparsing, the Outline version equals Syntax::version()"
    required: true
    manual: false
  - id: "C7"
    text: "fe-syntax integration test tests/outline.rs::outline_is_capped: a Rust file of 20,000 one-line fns gives exactly MAX_OUTLINE_ITEMS (10,000) items with truncated set; one of 10,000 fns gives 10,000 items with truncated unset"
    required: true
    manual: false
  - id: "C8"
    text: "fe-syntax integration test tests/outline.rs::outline_job_runs_on_another_thread: OutlineJob is Send (static assertion), and running the job on a spawned thread gives the same items as outline() on the calling thread"
    required: true
    manual: false
  - id: "C9"
    text: "scripts/check-no-gpui.sh prints ok: fe-syntax; Cargo.toml, Cargo.lock, crates/fe-syntax/Cargo.toml, crates/fe-syntax/src/syntax.rs, crates/fe-syntax/src/language.rs and THIRD_PARTY.md are unchanged; the existing highlight golden test still passes"
    required: true
    manual: false
---

# Extract a document outline in fe-syntax from per-language outline queries

# Extract a document outline in fe-syntax from per-language outline queries

Milestone M3 "Browse". The plan's Architecture table gives `fe-syntax` the "outline via
tree-sitter-tags", and `fe-workbench` the outline panel. This card is the GPUI-free model only:
given a parsed `Syntax`, list the document's symbols with name, kind, range and nesting. The panel
is a later card on top of this one and T63. It can run now, in parallel with T61: it touches only
`fe-syntax` files and no `Cargo.toml` or `Cargo.lock`.

## Problem

There is no way to list a file's symbols. The outline panel, breadcrumbs and "go to symbol in file"
all need one list per buffer: items in document order, each with a kind, the range to select, and
its parent. It has to come from the tree `Syntax` already keeps, with no second parse. A huge
file must not make it unbounded.

## What exists (checked 2026-10-05)

- Every grammar crate we link is MIT (THIRD_PARTY.md, "Tree-sitter grammars"). These ship a
  `queries/tags.scm` as `TAGS_QUERY`: rust 0.24.2, python 0.25.0, javascript 0.25.0, typescript
  0.23.2, go 0.25.0, c 0.24.2, cpp 0.23.4, java 0.23.5, ruby 0.23.1, php 0.24.2, c-sharp 0.23.5,
  lua 0.5.0 and swift 0.7.3. Scala 0.26.2 and kotlin-sg 0.4.1 have the file but no constant.
  tree-sitter-md, bash, css, html, json, toml-ng and yaml ship none.
- The tags queries are written for code navigation, not for an outline, and are uneven:
  - Rust's has no `impl` blocks (only `@reference.implementation`) and no `const` or `static`.
  - TypeScript's has no enums or type aliases.
  - Go's has no struct or interface kinds.
  - Python's doesn't tell methods from functions.
  - All of them mix in `@reference.*` patterns.
- The `tree-sitter-tags` crate (MIT) parses the source itself from one contiguous `&[u8]`. We keep
  a rope and already have the tree. It would also be a new dependency in `Cargo.lock`.
- `fold.rs` already keeps a per-language table outside `language.rs`; this card does the same.

So: **our own outline queries**, run with `tree_sitter::Query` (already a dependency) over the tree
`Syntax` holds. They use the tags capture convention (`@definition.<kind>` around the item, `@name`
on its name), so an upstream `tags.scm` pattern stays easy to compare against. Write them from each
grammar's node types (`src/node-types.json`). Don't paste upstream `tags.scm` text, so no
THIRD_PARTY.md row is needed: T63 holds that file.

## Change

### `crates/fe-syntax/src/outline.rs` (new), declared and re-exported from `lib.rs`

- Types:
  - `OutlineKind`: `Module`, `Class`, `Struct`, `Enum`, `Interface`, `Trait`, `Impl`, `Type`,
    `Function`, `Method`, `Constant`, `Macro`, `Table` and `Heading(u8)` (level 1–6).
    `OutlineKind::from_capture` maps the capture suffix: `definition.method` gives `Method`, and
    `definition.heading.2` gives `Heading(2)`.
  - `OutlineItem { name: String, kind: OutlineKind, range: Range<usize>, name_range: Range<usize>,
    row: u32, depth: u32, parent: Option<u32> }`. `range` is the item node's bytes, `name_range`
    is the name's, and `row` is the 0-based row where the name starts. `parent` indexes into
    `items`.
  - `Outline { items: Vec<OutlineItem>, truncated: bool, version: Version }`, with `items` in
    pre-order (document order, a parent before its children).
  - `NoOutline`: `Unsupported`, `TooLarge` or `NotParsed`.
- Getting one:
  - `outline_job(syntax: &Syntax) -> Result<OutlineJob, NoOutline>`:
    - `Unsupported`: the language has no outline query, or `syntax.failed()`.
    - `TooLarge`: the text is over `language.parse_limit()`, even when a window tree exists.
    - `NotParsed`: no tree yet, or `syntax.needs_parse()`.
    - Otherwise an `OutlineJob` holding the language, a clone of the tree, the `Text` (an O(1)
      clone) and the version. `OutlineJob` is `Send`, and `OutlineJob::run(self) -> Outline` can
      run on a background thread.
  - `outline(syntax) -> Result<Outline, NoOutline>` is `outline_job(syntax)?.run()`.
  - `outline_supported(language_name: &str) -> bool`, so a UI can hide the panel.
- `Outline::item_at(offset) -> Option<usize>`: the innermost item whose `range` contains `offset`,
  for "follow the caret" and breadcrumbs.
- These are free functions that use `Syntax`'s existing `tree()`, `needs_parse()`, `failed()`,
  `language()`, `version()` and the `pub(crate)` `text()`. **Don't edit `syntax.rs` or
  `language.rs`.**
- Queries: a private table, in the manner of `fold.rs`'s `KINDS`, from language name to its query
  parts, concatenated in order. Each query compiles once into a `OnceLock` on first use; a compile
  failure is a panic naming the language, as for highlights. TSX and TypeScript both use
  `javascript.scm` then `typescript.scm`.
- Running a query:
  - One `QueryCursor` pass over the root tree. Built-in text predicates (`#match?`, `#eq?`) read
    the rope through a `TextProvider` over `syntax::chunks`.
  - Only captures named `definition.*` and `name` count. Any other capture is a panic in the
    compile step, so a typo can't slip through.
  - **Dedup:** when several patterns capture the same item node, the first pattern in the query
    wins. That's how `Method` beats `Function` for a function inside an `impl`.
  - **Name:** the source text of the `@name` capture. When a match has several `@name` captures,
    it's the text from the first one's start to the last one's end, which is how Rust's `impl`
    gets `impl<T> Display for Foo<T>`. Whitespace runs collapse to one space, the result is
    trimmed, and it's cut at 200 chars on a char boundary.
  - **Nesting:** an item's parent is the nearest earlier item whose `range` contains its own,
    found with a stack in one pass over the items sorted by (start, longer first). `depth` follows
    from `parent`.
- **Bounds:** at most `pub const MAX_OUTLINE_ITEMS: usize = 10_000` items. The query stops at the
  cap and sets `truncated`. Only files at or under the parse limit get an outline at all.
- **Root tree only:** symbols in injected layers (Markdown fences, HTML `<script>`) aren't listed.

### Queries: `crates/fe-syntax/queries/outline/<language>.scm` (new)

The first set, each file headed "outline query, ours; capture convention of tree-sitter tags":

| File | Items |
|---|---|
| `rust.scm` | `mod` → Module, `struct`/`union` → Struct, `enum` → Enum, `trait` → Trait, `impl` → Impl (named by its header, `"impl" @name … type: (_) @name`), `type` → Type, `fn` → Method inside an `impl` or `trait` `declaration_list` (including `function_signature_item`), else Function, `const`/`static` → Constant, `macro_rules!` → Macro |
| `python.scm` | `class` → Class, `def` directly in a class body (plain or decorated) → Method, else Function, module-level `NAME = …` with an all-caps name → Constant |
| `javascript.scm` | class declarations → Class, `method_definition` → Method, function and generator declarations → Function, `const`/`let`/`var` declarators whose value is an arrow function or function expression → Function |
| `typescript.scm` (after `javascript.scm`) | `interface` → Interface, `type` alias → Type, `enum` → Enum, `abstract class` → Class, `namespace`/`module` → Module, method signatures in interfaces and abstract classes → Method, `function_signature` → Function |
| `go.scm` | `func` → Function, method declarations → Method (method name only), `type_spec` with a struct type → Struct, interface type → Interface, else Type, `method_elem` → Method, top-level `const` specs → Constant |
| `java.scm` | class and record → Class, interface and annotation type → Interface, enum → Enum, method and constructor → Method |
| `c.scm` | function definitions (through pointer declarators) → Function, `struct`/`union` with a body → Struct, `enum` with a body → Enum, `typedef` → Type, `#define` → Macro |
| `markdown.scm` | each `section` → `Heading(n)` from its ATX marker or setext underline, named by the heading content, so sections nest by level |
| `toml.scm` | `[table]` → Table, `[[array]]` → Table, named by the key |

No fields, properties or local variables in this first cut. The other grammars (cpp, csharp, ruby,
php, swift, kotlin, scala, lua, bash, yaml, …) have no outline yet and return `Unsupported`.

### Tests (new files)

- `crates/fe-syntax/tests/outline.rs` is an integration test using only the public API: `Buffer`,
  `Syntax::new`, `parse_within` or `ParseJob` then `finish`, and `outline`.
- Its samples live in `crates/fe-syntax/tests/outline_samples/`: `rust.rs`, `python.py`,
  `javascript.js`, `typescript.ts`, `tsx.tsx`, `go.go`, `java.java`, `c.c`, `markdown.md` and
  `toml.toml`. Each has a `<sample>.outline` golden next to it.
  - Golden format: one line per item, indented two spaces per depth:
    `<kind> "<name>" <row>:<col>-<row>:<col>` for the item range, with 1-based rows and byte
    columns.
  - `UPDATE_GOLDEN=1` rewrites the goldens, as the highlight goldens do. Review the diff.
- Read `CARGO_MANIFEST_DIR` at **run time** (`std::env::var`), not with `env!`. The shared cargo
  target can keep a compile-time path from a deleted `.verify/` snapshot (the T11 hazard).

## Out of scope

- The outline panel, breadcrumbs, and Cmd-Shift-O, which are later `fe-workbench` cards.
- Workspace symbols and tags indexes, which belong to `fe-nav` in M4.
- Languages beyond the table above.
- Keeping an outline current incrementally. A caller reruns the job after a reparse; `version`
  says which text it describes.
- A bench. Add one if the panel shows lag on files near the parse limit.

## Decided (lead defaults; the user may revisit)

- **Our own outline queries rather than `tree-sitter-tags` or the grammars' `tags.scm`.** The
  plan said "via tree-sitter-tags". This keeps the capture convention but drops the crate, which
  would re-parse and add a dependency. It also replaces the uneven upstream queries.
- **No outline for files over their parse limit** (`NoOutline::TooLarge`). There's no outline of the
  parsed window either: it would change as you scroll, and it can start mid-item.
- Outlines come from the root language only. Injected code isn't listed.
- **At most 10,000 items**, with `truncated` set beyond that.
- First languages are Rust, Python, JavaScript, TypeScript, TSX, Go, Java, C, Markdown and TOML.
  Rust `impl`s are named by their header (`impl Display for Foo`). Go methods are named without
  their receiver.

## Implementation notes

- tree-sitter-md makes a `section` only for ATX headings. A setext heading's item is the
  `setext_heading` node alone, so it nests under the section around it but holds nothing itself.
- An ATX closing sequence (`## D ##`) is inside the grammar's `heading_content`, so heading names
  drop trailing `#`s when a space comes before them (or when they're the whole content), as
  CommonMark does. `name_range` still covers the whole content node.
- Items found are deduplicated by tree node id, keeping the lowest pattern index. The cap counts
  distinct item nodes, in the order the query cursor returns matches.
