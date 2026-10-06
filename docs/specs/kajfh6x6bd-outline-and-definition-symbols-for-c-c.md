---
task_uid: "01m48zjegg9fx6whkajfh6x6bd"
task: "T113"
rev: 6
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-syntax integration test tests/more_languages.rs::outlines_match_golden_files: for each of cpp, csharp, ruby, php, swift, kotlin, scala and lua, outline() of the sample in tests/more_languages_samples/ equals its .outline golden, and every golden has at least one type or module item with a nested method"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax integration test tests/more_languages.rs::symbols_match_golden_files: symbols() of each sample equals its .symbols golden, and every outline item of the sample is also a symbols item with the same kind and range"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax integration test tests/more_languages.rs::members_and_variables: a C# property and enum member, a Kotlin class property and enum entry, a Swift stored property and enum case, a PHP class property and enum case, and a Scala val in a class body are Field or Variant nested under their type; top-level Lua local x = 1, Kotlin val x and Swift let x are Variable; no local variable or parameter in any sample is a symbol"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax integration test tests/more_languages.rs::methods_nest_under_their_type: a C++ member function in a class in a namespace and a Ruby def in a class in a module are Method items whose parent chain is the type then the namespace or module; Lua nests lexically, so function M:f() is a Method named f at the top level, and a function taking self inside a table constructor (local Account = { deposit = function(self, ...) end }) is a Method nested under the table's Module; Ruby attr_accessor :name gives Field name"
    required: true
    manual: false
  - id: "C5"
    text: "fe-syntax integration test tests/more_languages.rs::supported_flags: symbols_supported is true for the eight languages, and markdown, toml, json, bash and yaml still give Err(NoOutline::Unsupported) from symbols()"
    required: true
    manual: false
  - id: "C6"
    text: "Each new .scm file is headed with its source and licence or ours; THIRD_PARTY.md lists every adapted file with source, version or commit and licence (MIT or Apache-2.0 only); the new files use no capture other than @name, @definition.\u003ckind\u003e, and @_-prefixed captures used only by predicates (as in definitions/c.scm)"
    required: true
    manual: false
  - id: "C7"
    text: "scripts/check-no-gpui.sh prints ok: fe-syntax; Cargo.toml, Cargo.lock and crates/fe-syntax/Cargo.toml are unchanged; in tests/outline.rs and tests/symbols.rs only the three supported-language-set assertions change (outlines_match_golden_files and symbols_match_golden_files sample coverage, and the unsupported_languages_have_no_symbols list), to account for the eight languages whose samples are in more_languages_samples/; their goldens are unchanged and every test in both files passes"
    required: true
    manual: false
---

# Outline and definition symbols for C++, C#, Ruby, PHP, Swift, Kotlin, Scala and Lua

# Outline and definition symbols for C++, C#, Ruby, PHP, Swift, Kotlin, Scala and Lua

Milestone M4 "Navigate". T64 gave outlines, and T102 definition symbols, to rust, python,
javascript, typescript, tsx, go, java and c only. `fe-nav` (T103) indexes only languages with
`symbols_supported`, so go-to-def, workspace symbols and the outline panel do nothing in eight
languages `fe-syntax` already highlights. The user asked (2026-10-06) for these eight. This card
adds an outline query and a definitions query for each. It touches only `fe-syntax` files and
`THIRD_PARTY.md`, and adds **no grammar crate**: every grammar is already a dependency.

## What exists (checked 2026-10-06)

| Language name | Crate (already in `Cargo.toml` / `crates/fe-syntax/Cargo.toml`) | Licence | Ships `queries/tags.scm` |
|---|---|---|---|
| `cpp` | tree-sitter-cpp 0.23.4 | MIT | yes (`TAGS_QUERY`) |
| `csharp` | tree-sitter-c-sharp 0.23.5 | MIT | yes (`TAGS_QUERY`) |
| `ruby` | tree-sitter-ruby 0.23.1 | MIT | yes (`TAGS_QUERY`) |
| `php` | tree-sitter-php 0.24.2 | MIT | yes (`TAGS_QUERY`) |
| `swift` | tree-sitter-swift 0.7.3 | MIT | yes (`TAGS_QUERY`; also `outline.scm`) |
| `kotlin` | tree-sitter-kotlin-sg 0.4.1 (fwcd's grammar) | MIT | yes (file only, no constant) |
| `scala` | tree-sitter-scala 0.26.2 | MIT | yes (file only) |
| `lua` | tree-sitter-lua 0.5.0 | MIT | yes (`TAGS_QUERY`) |

- `crates/fe-syntax/src/outline.rs`: T64's `QUERIES` table (one `OutlineQuery` per language, the
  concatenation of its `parts`; TypeScript is `javascript.scm` then `typescript.scm`), and T102's
  definitions queries appended after the outline query for `symbols()`. Only `@name` and
  `@definition.<kind>` captures are allowed; `from_capture` maps module, class, struct, enum,
  interface, trait, impl, type, function, method, constant, macro, and T102's field, variant and
  variable.
- `crates/fe-syntax/queries/outline/` and `crates/fe-syntax/queries/definitions/` (T102).

## Change

### Queries (new files)

For each of `cpp`, `csharp`, `ruby`, `php`, `swift`, `kotlin`, `scala`, `lua`:

- `crates/fe-syntax/queries/outline/<language>.scm`: types (class, struct, enum, interface/protocol,
  trait, type alias), modules and namespaces, functions, methods (a function directly in a type body
  is a Method), constants and macros where the language has them; the same set T64 chose for its
  languages.
- `crates/fe-syntax/queries/definitions/<language>.scm`: fields and properties → Field; enum cases,
  members and entries → Variant; module- or file-level variables → Variable (no locals or
  parameters), as T102 did for its languages.

Sources and attribution:

- Adapt from the grammar crate's own `queries/tags.scm` at the pinned version (MIT, same crate),
  and where it lacks a construct, from nvim-treesitter's queries for that language (Apache-2.0) or
  from the grammar's `node-types.json`. Never from Zed's crates.
- Keep only `@definition.*` and `@name` captures: drop `@reference.*`, `@doc`, `#strip!`,
  `#select-adjacent!` and any predicate `fe-syntax` doesn't evaluate. Map upstream kinds onto ours:
  Scala `object` → Module, `property` → Field, upstream `variable` at top level → Variable;
  `definition.class` on a C# `record` or Kotlin `data class` stays Class. No new `OutlineKind`s.
- Each file is headed with its source (path, crate version or nvim-treesitter commit, licence) and
  what was changed, or "ours" for a file written from `node-types.json`.
- `THIRD_PARTY.md`: one row per adapted file in "Code copied or adapted into this repository" (path,
  source, version or commit, licence), and a note to ship the Apache-2.0 text if any file adapts
  nvim-treesitter.
- `cpp` may reuse `c.scm` as its first part (outline and definitions), as TypeScript reuses
  `javascript.scm`, if C's patterns compile against the C++ grammar; otherwise a standalone file.
- Lua and Ruby have no declared types: Lua's `function M.f()` / `function M:f()` are Function /
  Method named `f`; Ruby's `class`, `module`, `def`, `def self.x` and constant assignments, and
  `attr_reader`/`attr_writer`/`attr_accessor` symbols as Field.

### `crates/fe-syntax/src/outline.rs`

Eight `QUERIES` entries (and their definitions parts), and the table length. No change to the
machinery, error rules, nesting, naming or cap. `outline()` and `symbols()` now succeed for these
languages; `symbols_supported` is true for them.

### Tests (new files)

- `crates/fe-syntax/tests/more_languages.rs`, an integration test on the public API, gated on the
  eight language features.
- Samples in `crates/fe-syntax/tests/more_languages_samples/`: `cpp.cpp`, `csharp.cs`, `ruby.rb`,
  `php.php`, `swift.swift`, `kotlin.kt`, `scala.scala` and `lua.lua`, each with a `<sample>.outline`
  and a `<sample>.symbols` golden in T64's golden format, rewritten with `UPDATE_GOLDEN=1`. Each
  sample covers every kind its queries emit, nesting (a method in a class in a namespace or module)
  and one construct that must not appear (a local variable or parameter). Read `CARGO_MANIFEST_DIR`
  at run time.

## Out of scope

- Languages not listed (bash, html, css, yaml, sql, dockerfile, make, xml, json).
- T105's imports tier for these languages (its import-line table stays as is) and the word rule
  (Ruby `empty?`, PHP `$x` and Kotlin backtick names are not words to T105).
- Any new grammar crate, or any change to highlight queries.
- Objective-C, C++ templates' specialisations as separate symbols, PHP in HTML templates beyond what
  the grammar's PHP language parses.

## Decided (lead defaults; the user may revisit)

- **No grammar crates added**: all eight are dependencies already (MIT).
- **One card for eight languages**: about 8 × 50 lines of queries plus a table, well under the cap;
  samples and goldens are test files.
- Upstream `tags.scm` as the starting point (attributed), mapped onto our kinds, rather than adding
  `OutlineKind`s for `object` or `property`.
