---
task_uid: "01m54hwz1gsnc4596axtkzrmcq"
task: "T140"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test an_unclosed_lua_loop_indents_its_next_row (new crates/fe-syntax/tests/lua_loop_indent.rs, top-level fn, cfg feature lua) passes: suggest_indent on a blank row after 'for i = 1, 2 do', 'for k, v in pairs(t) do', 'while x do', 'do' and 'repeat' gives Relative { basis_row: 0, delta: 1 }, and after the nested 'function f()\\n    for i = 1, 2 do' and 'if x then\\n    while y do' Relative { basis_row: 1, delta: 1 }; it fails at the base (delta 0)"
    required: true
    manual: false
  - id: "C2"
    text: "The only query change is the \"do\" and \"repeat\" tokens added to lua.scm's first @indent.begin list beside \"then\", with the header's 'Changes: none.' replaced by a line naming them as ours; language.rs, THIRD_PARTY.md and other languages' queries are untouched"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax --all-features passes with no existing test edited (tests/indent.rs lua_rows, unclosed_openers and blank_rows_take_the_next_rows_indentation among them); cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Indent the row after an unclosed Lua for, while, do or repeat a level in, as after an unclosed if ... then (Mac T759)

## Problem

Enter after a Lua loop header that has no `end` yet leaves the new row at the header's own
indentation instead of a level in. `for i = 1, 2 do`, `for k, v in pairs(t) do`, `while x do`, a bare
`do` and `repeat` all get it, at top level and nested (`function f()` / `    for i = 1, 2 do` gives 4,
not 8). `if x then` and `function f()` in the same state do get a level, and so do the loops once their
`end` (or `until`) is typed. Lua has no auto-closed `end`, so this is the row every new loop is typed
into. VS Code's Lua indentation rules indent after `do` and `repeat` as after `then` and `function`.

Reproduced at Mac main b7bd02cf through `suggest_indent` on a blank row inserted after the header (the
`after` helper of crates/fe-syntax/tests/indent.rs): each of the five headers above gives
`Relative { basis_row: 0, delta: 0 }` (column 0); `"if x then"` gives delta 1.

Cause: crates/fe-syntax/queries/indents/lua.scm begins on `(for_statement)`, `(while_statement)`,
`(repeat_statement)` and `(do_statement)` nodes, which an unclosed loop doesn't parse into (it's an
`ERROR`), and on the `"then"` token, which is why `if` works; there is no token pattern for `do` or
`repeat`.

## Change

In crates/fe-syntax/queries/indents/lua.scm, add the `"do"` and `"repeat"` tokens to the first
`@indent.begin` list, next to `"then"`, and replace the header's `; Changes: none.` with a line saying
the two tokens are ours, so an unclosed loop's next row is a level in as an unclosed `if`'s is. Nothing
else changes (THIRD_PARTY.md's row stays as is; language.rs is untouched). Prototyped at b7bd02cf:
all five headers give delta 1, nested ones a level past their parent, and
`cargo nextest run -p fe-syntax --all-features --test indent` (29 tests) still passes.

Regression test: new integration test crates/fe-syntax/tests/lua_loop_indent.rs, gated on
`#![cfg(feature = "lua")]`, with its own `after` helper copied from tests/indent.rs (Buffer::new,
`Syntax::new(language_by_name("lua")..)`, `parse_job().run()`, `suggest_indent` on the inserted blank
row). Top-level fn `an_unclosed_lua_loop_indents_its_next_row`: the five headers at top level give
`Relative { basis_row: 0, delta: 1 }`, and `function f()\n    for i = 1, 2 do` and
`if x then\n    while y do` give `Relative { basis_row: 1, delta: 1 }`.
