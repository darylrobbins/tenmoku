---
task_uid: "01m55cebw0794yssnpvg5sk4vm"
task: "T162"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo test -p fe-vim and cargo test -p fe-gpui pass with no test file added, removed or changed (nothing under crates/fe-vim/tests/ or crates/fe-vim/src/tests/, and no *_tests.rs)."
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-vim/src/ex/table.rs defines the entry type (full name, shortest abbreviation, whether it can edit, a pure parse, a run over the range) and COMMANDS, empty here; ExCommand gains one variant for a table command and keeps its derives (Clone, Debug, PartialEq, Eq); ex.rs's command() and execute() reach every entry through COMMANDS alone, after the names ex.rs and lines.rs already take and before the lines::command fallback."
    required: true
    manual: false
  - id: "C3"
    text: "ex_edits in crates/fe-gpui/src/view/host.rs answers the table variant from its entry's can-edit flag, so a later entry needs no host.rs change; table.rs's module docs give the recipe: a new crates/fe-vim/src/ex/\u003cname\u003e.rs plus its #[path] mod line and its entry in table.rs, nothing in ex.rs, host.rs or lib.rs."
    required: true
    manual: false
---

# fe-vim: look ex commands up in a table so each new one is a module of its own under src/ex/, with no change in behaviour (Mac T822)

## Problem

The user approved a "Vim breadth" track on 2026-10-17: the rest of fe-vim's "Not planned for M7's first ex
cards" list (crates/fe-vim/src/lib.rs, the `# Ex` section), among them `:pu`, `:undo`, `:set` and `:r`.
Every ex command fe-vim has today is wired through `crates/fe-vim/src/ex.rs` in three places: a variant of
the public `ExCommand` enum, a name check in `command()` (the `if` chain from `host(name)` down to the
`lines::command` fallback), and an arm in `execute()`'s match. A fourth place outside the crate also
matches every variant: `ex_edits` in `crates/fe-gpui/src/view/host.rs` (whether a line can edit a
read-only buffer). `ex/lines.rs`, `ex/global.rs` and `ex/normal.rs` already hold their commands' bodies,
but each new command still edits ex.rs and host.rs, so every ex command card conflicts with every other
one and with any ex.rs change, and they could only run one at a time.

## Change

A refactor with no change in behaviour, so the command cards that follow each add a module of their own.

- New `crates/fe-vim/src/ex/table.rs` (declared `mod table;` in ex.rs): the entry type for one ex command
  a module defines (a `pub(super) const COMMAND` in that module), and `COMMANDS`, the list of them, empty
  in this card. An entry holds the command's full name and its shortest abbreviation (`"put", 2` for
  `:pu`, matched as ex.rs matches names today: `name.len() >= min && full.starts_with(name)`), whether it
  can edit the buffer, a pure `parse` of what follows the name (with `command()`'s `check`, so a line
  before the first line fails where nvim fails), and a `run` over the line's range, given what
  `execute()` already has (the target, the actor, the resolved `lines`, the range's addresses and the
  parsed arguments). Each command module is declared in table.rs beside its entry as
  `#[path = "<name>.rs"] mod <name>;`, so it lives at `crates/fe-vim/src/ex/<name>.rs` and can use ex.rs's
  private helpers (`counted_lines`, `row`, `to_row_start`, `first_non_blank`, `register_and_count`,
  `trailing`, `uncommented`, `Parser`) as `super::super::…` or through re-exports in table.rs.
- `ExCommand` gains one variant for a table command (for example `Table { name: &'static str, args:
  String }`, or a small public struct), keeping its derives (`Clone, Debug, PartialEq, Eq`) and `Ex::parse`
  pure. `command()` looks the name up in `COMMANDS` after the commands ex.rs and lines.rs already take by
  name (so no existing name changes meaning) and before the `lines::command` fallback; `execute()` runs
  the entry's `run`. A name no entry takes is E492 as now.
- `crates/fe-gpui/src/view/host.rs` `ex_edits`: one arm for the table variant, answered by the entry's
  flag (expose it through a `pub fn` on the variant's type or on `Ex`), so later entries need no host.rs
  change.
- table.rs's module docs give the recipe: a new `crates/fe-vim/src/ex/<name>.rs` defining `COMMAND`, plus
  its two lines in table.rs (the `#[path]` mod and the entry), and nothing in ex.rs, host.rs or lib.rs; the
  command documents itself in its module docs.

Leave every existing command where it is (moving `:noh` or the line commands would change public
`ExCommand` variants that tests name). Leave lib.rs and its "Not planned" list alone: the lead files one
docs chore after the track lands.

ex.rs is also changed by Mac T769 (the two-letter `:s` forms, in the host's stack 24), so start this on a
trunk that has T769 landed.

## Implementation notes

- `table.rs` holds `Entry`, `COMMANDS` (empty) and the lookups; `ExCommand::Table(TableCommand)` keeps the name and arguments and answers `edits()` from its entry, which `ex_edits` calls.
- `command()` shares its `check` between `lines::command` and the table through a once-closure, wrapped after the `:browse` recursion so the generic doesn't recurse.
- T48 F14 (`keys_edit` treating `/` and `v` as edits) is left: fixing it changes behaviour and needs new test cases, both against this card's "no change in behaviour" and C1.
