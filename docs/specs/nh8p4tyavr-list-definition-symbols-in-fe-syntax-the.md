---
task_uid: "01m48tr2a6xckj66nh8p4tyavr"
task: "T102"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax integration test tests/symbols.rs::symbols_match_golden_files: every sample in tests/symbols_samples/ (rust, python, javascript, typescript, tsx, go, java, c) is parsed whole and symbols() equals its \u003csample\u003e.symbols golden (T64 golden format); UPDATE_GOLDEN=1 rewrites them"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax integration test tests/symbols.rs::symbols_extend_the_outline: for every sample in tests/outline_samples/ whose language has symbols, each outline() item appears in symbols() with the same kind, name, range and parent name; tests/outline.rs and every .outline golden pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax integration test tests/symbols.rs::rust_fields_and_variants_nest: struct S { a: u8, b: String } and enum E { A, B(u8), C { x: i32 } } give Field a and b under Struct S and Variant A, B and C under Enum E; a let binding inside a fn and a tuple-struct field are not listed"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax integration test tests/symbols.rs::variables_are_module_level_only: Python x = 1 at module level is Variable, MAX = 3 stays Constant, y = 2 in a class body is Field under the class, z = 3 inside a def is not listed; JavaScript top-level const a = 1 and export let b = 2 are Variable, const f = () =\u003e 1 stays Function, a const inside a function is not listed"
    required: true
    manual: false
  - id: "C5"
    text: "fe-syntax integration test tests/symbols.rs::members_of_ts_go_java_and_c: a TypeScript interface property and enum member, a Go struct field (both names of a, b int) and package-level var, a Java field and enum constant, and a C struct field, enumerator and file-scope int g; (not the prototype int f(void);) are listed with kinds Field, Variant or Variable under the right parent"
    required: true
    manual: false
  - id: "C6"
    text: "fe-syntax integration test tests/symbols.rs::unsupported_languages_have_no_symbols: markdown, toml and json give Err(NoOutline::Unsupported) from symbols() and symbols_supported is false for them and true for the eight code languages; a Rust file over its parse limit gives Err(TooLarge)"
    required: true
    manual: false
  - id: "C7"
    text: "scripts/check-no-gpui.sh prints ok: fe-syntax; Cargo.toml, Cargo.lock and crates/fe-syntax/Cargo.toml are unchanged; each new query file starts with the \"definitions query, ours\" header"
    required: true
    manual: false
---

# List definition symbols in fe-syntax: the outline plus fields, enum variants and top-level variables

# List definition symbols in fe-syntax: the outline plus fields, enum variants and top-level variables

Milestone M4 "Navigate", first card of `fe-nav` (search-based navigation). The plan's Architecture
table gives `fe-nav` "a tags index plus ripgrep for instant go-to-def, references and symbols". The
index (the next card) needs every *definition* in a file, which is more than an outline shows. This
card adds that list to `fe-syntax`, next to T64's outline and from the same machinery. It touches
only `fe-syntax` files and no `Cargo.toml` or `Cargo.lock`, so it can run now.

## Problem

T64's `outline()` lists types, functions, methods, modules, constants and macros. Go-to-definition
also lands on struct and class fields, enum variants and module-level variables (`Foo::Bar`,
`self.count`, `config.timeout`, `export const routes = [...]`). Adding those to the outline queries
would change what the outline panel (T78) shows and every T64 golden. Copying the outline machinery
into `fe-nav` would duplicate T64's dedup, nesting, naming and cap rules.

## What exists (checked 2026-10-06)

- `crates/fe-syntax/src/outline.rs`: `OutlineQuery { language, parts, compiled }`, one per language
  in the `QUERIES` table, each the concatenation of its `parts` (TypeScript is `javascript.scm` then
  `typescript.scm`). `OutlineJob::run` runs the query over the root tree; the first pattern in the
  query wins when several capture the same node; parents come from range containment; at most
  `MAX_OUTLINE_ITEMS` items; only `@name` and `@definition.<kind>` captures are allowed.
- Outline queries exist for rust, python, javascript, typescript, tsx, go, java, c, markdown and toml.
- `crates/fe-syntax/tests/outline.rs` and the goldens in `tests/outline_samples/`.

## Change

### `crates/fe-syntax/src/outline.rs`

- `OutlineKind` gains `Field`, `Variant` and `Variable`, mapped by `from_capture` from
  `definition.field`, `definition.variant` and `definition.variable`. Mark `OutlineKind`
  `#[non_exhaustive]` so later kinds don't break downstream `match`es.
- `pub fn symbols(syntax: &Syntax) -> Result<Outline, NoOutline>` and `pub fn symbols_job(syntax)
  -> Result<OutlineJob, NoOutline>`: the language's outline query **followed by** its definitions
  query, compiled together as one query (a second compiled `OnceLock` on the same table entry, or a
  second table; implementer's choice). Outline patterns come first, so first-pattern-wins keeps
  every outline item's kind, and the new patterns only add items. Same error rules, nesting, naming,
  cap and `version` as `outline()`.
- `pub fn symbols_supported(language_name: &str) -> bool`: true only for languages with a
  definitions query. `symbols()` returns `Err(NoOutline::Unsupported)` for the others, including
  markdown and toml: headings and tables aren't code definitions.
- `outline()` and `outline_job()` are unchanged in behaviour.

### Queries: `crates/fe-syntax/queries/definitions/<language>.scm` (new)

Each headed "definitions query, ours; capture convention of tree-sitter tags", written from the
grammar's `node-types.json` like T64's (don't paste upstream `tags.scm` text).

| File | Adds |
|---|---|
| `rust.scm` | named struct and union fields → Field; enum variants → Variant |
| `python.scm` | assignments and annotated assignments directly in a class body → Field; module-level assignments to a plain identifier → Variable (all-caps ones stay Constant from the outline query) |
| `javascript.scm` (also used by typescript and tsx, before `typescript.scm`) | class fields (`field_definition`) → Field; `const`/`let`/`var` declarators directly under `program` or an `export_statement`, not bound to a function → Variable |
| `typescript.scm` | `public_field_definition` → Field; interface and object-type property signatures → Field; enum members → Variant |
| `go.scm` | struct field names (each name of `a, b int`) → Field; package-level `var` specs → Variable |
| `java.scm` | field declarators → Field; enum constants → Variant |
| `c.scm` | struct and union field declarators → Field; enumerators → Variant; file-scope variable declarations (not prototypes) → Variable |

No locals, parameters or fields of anonymous types in this cut.

### `crates/fe-syntax/src/lib.rs`

Re-export `symbols`, `symbols_job` and `symbols_supported`; extend the module docs by one line.

### Tests (new files)

- `crates/fe-syntax/tests/symbols.rs`, an integration test on the public API, gated on the same
  language features as `tests/outline.rs`.
- Samples in `crates/fe-syntax/tests/symbols_samples/`: `rust.rs`, `python.py`, `javascript.js`,
  `typescript.ts`, `tsx.tsx`, `go.go`, `java.java` and `c.c`, each with a `<sample>.symbols` golden
  in T64's golden format (`<kind> "<name>" <row>:<col>-<row>:<col>`, two spaces per depth),
  rewritten with `UPDATE_GOLDEN=1`. Read `CARGO_MANIFEST_DIR` at run time, as T64 does.

## Out of scope

- Languages without an outline query (cpp, csharp, ruby, php, swift, kotlin, scala, lua, bash …).
- Any change to the outline panel or to `outline()` results.
- Local variables, parameters, imports and re-exports as definitions.

## Decided (lead defaults; the user may revisit)

- **One combined query per language (outline, then definitions)** rather than a second pass, so a
  file is queried once and nesting stays consistent (fields nest under their struct).
- The new kinds are listed by `symbols()` only; the outline stays as T64 shaped it.
