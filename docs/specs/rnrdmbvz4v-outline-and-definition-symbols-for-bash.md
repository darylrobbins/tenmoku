---
task_uid: "01m4rgwgrq9v8pzjrnrdmbvz4v"
task: "T40"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test outline_scripts: the bash sample's outline lists both f() {} and function g {} as Function items with their names and ranges, and its symbols add top-level variables"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test outline_scripts: the make sample's outline lists its rule targets as Function and define names as Macro, with no .PHONY or % pattern target; its symbols add top-level variable assignments"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test outline_scripts: the sql sample's outline lists CREATE TABLE, VIEW and MATERIALIZED VIEW names (schema-qualified ones by their last name) as Table, CREATE FUNCTION as Function and CREATE TYPE as Type; its symbols nest each table's columns as Field"
    required: true
    manual: false
  - id: "C4"
    text: "outline_supported and symbols_supported are true for bash, make and sql and unchanged for every other language; every existing fe-syntax outline and symbols test passes with only the bash row removed from more_languages.rs's unsupported list"
    required: true
    manual: false
  - id: "C5"
    text: "Each new .scm file starts with an 'ours' header line; cargo test -p fe-syntax -p fe-nav, cargo clippy -p fe-syntax --all-targets -- -D warnings and scripts/check-syntax-features.sh pass"
    required: true
    manual: false
---

# Outline and definition symbols for Bash, Make and SQL: shell functions, Makefile targets and macros, SQL tables, views, functions and types (Mac T497)

Filed on helium from Mac T497 (~/coordination/helium-cards/T497.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-u.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The outline panel (M3, "Browse") and the search-based navigation index (fe-nav, M4) only know symbols in
languages with an outline query. `QUERIES` in `crates/fe-syntax/src/outline.rs` (:1680) has 18 entries; Bash,
Make and SQL have none, so a shell script, a Makefile or a schema file opens with an empty outline, and
go-to-definition by search finds none of their functions, targets or tables (`fe_nav::extract::symbols_language`
keeps only `symbols_supported` languages). The plan's first priority is the read path: users browse code more
than they edit it. Their grammars ship no `tags.scm` (`tree-sitter-bash` 0.25, `tree-sitter-make` 1.1,
`tree-sitter-sequel` 0.3 ship only highlights), so the queries are ours, as `queries/outline/toml.scm` is.
`crates/fe-syntax/tests/more_languages.rs` :340-351 asserts bash has no symbols; that was T113's scope line, not
a decision against it.

## Change

New queries, written from each grammar's `node-types.json`, each starting with the header
`;; outline query, ours; capture convention of tree-sitter tags` (or `;; definitions query, ours; …`), using
the existing `@name` / `@definition.<kind>` captures:

- `crates/fe-syntax/queries/outline/bash.scm`, `queries/definitions/bash.scm`: `function_definition` names
  (both `f() {}` and `function f {}`) as `definition.function`. Definitions add top-level `variable_assignment`
  names as `definition.variable`.
- `crates/fe-syntax/queries/outline/make.scm`, `queries/definitions/make.scm`: each `rule`'s targets
  as `definition.function`, except special targets starting with `.` (`.PHONY`, `.SUFFIXES`) and pattern targets
  containing `%`; `define_directive` names as `definition.macro`. Definitions add top-level
  `variable_assignment` names as `definition.variable`.
- `crates/fe-syntax/queries/outline/sql.scm`, `queries/definitions/sql.scm`: `create_table`, `create_view` and
  `create_materialized_view` names as `definition.table`, `create_function` names as `definition.function`,
  `create_type` names as `definition.type`. The name is the `object_reference`'s `name` field (`t` in
  `public.t`). Definitions add each `create_table` column name as `definition.field`, nested under its table.

Three `OutlineQuery::new("bash" | "make" | "sql", …)` entries in `QUERIES` (array length 21). Nothing else in
outline.rs changes. In `more_languages.rs`, remove the `("bash", …)` row from the unsupported list (:347); no
other edit there.

## Tests

New file `crates/fe-syntax/tests/outline_scripts.rs`, with its samples in the new directory
`crates/fe-syntax/tests/outline_scripts_samples/` (`bash.sh`, `make.mk`, `sql.sql` and their `.outline` and
`.symbols` expectations, in the format of `outline_samples/` and `symbols_samples/`).

## Implementation notes

- Outside the listed scope: `tests/outline.rs` and `tests/symbols.rs` each assert that every outlined (or
  symbols) language has a sample, from `outline_samples/`/`symbols_samples/` plus `more_languages_samples/`.
  With bash, make and sql supported they failed, so both now also read `outline_scripts_samples/`. And
  `tests/symbols.rs`'s `unsupported_languages_have_no_symbols` lists the languages with symbols exactly;
  bash, make and sql are added to it. No existing assertion was weakened.
- Also outside it, from review round 1: `fe_nav::extract::is_symbol` dropped every `Table` item, for TOML,
  so SQL tables and views never reached go-to-definition. TOML has no symbols, so the filter now keeps
  tables; `fe-nav/tests/definition.rs` `sql_tables_and_views_are_definitions` covers same-file and project
  lookup.
- Make: a rule with one target is the item, recipe and all. A rule with several, `install uninstall:`, lists
  each target word on its own, since one node is one item and the rule would merge the names. Variables are
  every `variable_assignment` without `target_or_pattern`, so ones in a conditional or under
  `override`/`export` count (make has no scope there); a target-specific one, `tool: CFLAGS += -g`, doesn't.
  The grammar's rule and assignment nodes end after their trailing newlines, so their ranges do too.
  Any target node counts, by its text: a word, an archive member, `lib.a(a.o)`, or one with expansions,
  `$(OUT)/stamp` (review round 2).
- Bash variables: direct children of `program`, plain, several on a line (`variable_assignments`) or in a
  `declaration_command` (`export`, `readonly`); an element's, `arr[0]=1`, is listed as the array's.
- SQL functions: the name is the `object_reference` before the arguments, unanchored, so comments on either
  side don't hide it and a `RETURNS SETOF t` type, another `object_reference` after them, isn't taken for
  it. A quoted column name, `"when"`, is a `literal` in this grammar, so columns take either node.
- Quoted SQL names (review round 2, F3): the outline lists a name as written, `"audit log"`, as outline.rs
  is unchanged. fe-nav finds it without its quotes: `extract::symbol_name` strips one pair of `"` or a
  backtick from a SQL name, for the index and for same-file lookup; the range stays the quoted name.
- Follow-up, not done here (review F2): fe-nav's `word_at` takes a word of `fe_project::is_word_char`s, the
  same chars its ripgrep fallback and reference search match whole, so a name with punctuation (Make's
  `compile-rule` or `main.o`, a Bash `my-func`) is listed and indexed but the caret's word is only part of
  it. Widening the word per language means changing the search's and references' word boundaries too, a
  navigation change past this card's outline scope; it wants its own card.
