---
task_uid: "01m53hnmfkrk1c9ny6q50k4bay"
task: "T123"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "ex_day_one: :Ex and :Explore, :ol, :oldfiles and :bro ol, :cope, :copen, :lop, :ccl, :lcl and :Replace x parse to their HostCommands; a range is E481, :Ex foo is E488 and :Replace with no argument is E471"
    required: true
    manual: false
  - id: "C2"
    text: "vim_day_one_tests::explore_and_problems: :Ex focuses the project tree with the current file selected; :copen shows and focuses the Problems panel and :cclose hides it"
    required: true
    manual: false
  - id: "C3"
    text: "vim_day_one_tests::oldfiles: :oldfiles opens the Open Recent picker, and escaping it returns focus to the editor in normal mode"
    required: true
    manual: false
  - id: "C4"
    text: "vim_day_one_tests::replace: with a project search for foo, :Replace bar asks the same confirmation as Replace All and, confirmed, replaces in every result file; with no search it says No search to replace"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim -p fe-workbench and cargo clippy -p fe-vim -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Vim ex commands for the day-one features: :Ex, :oldfiles, :copen and :cclose, and :Replace across files (Mac T522)

## Problem

The day-one features (Open Recent, the Problems panel, project-wide replace, the project tree) have commands and
keys but no ex commands, so a vim user has to leave the command line habit for them. User 2026-10-12: every
day-one feature must be vi friendly, following Vim/Neovim and common plugin (netrw, quickfix) conventions.
fe-vim's ex host commands are in `HostCommand` (crates/fe-vim/src/ex.rs:232), run by
`crates/fe-workbench/src/vim_host.rs`.

## Change

`crates/fe-vim/src/ex.rs`, new `HostCommand` variants parsed in `host()` (no range; nvim's abbreviations):

- `:Ex[plore]` → `Explore`: focus the project tree with the current file's row revealed and selected (netrw opens
  the current file's folder). An argument is "E488: Trailing characters" (a path argument is a later card).
- `:ol[dfiles]` and `:bro[wse] ol[dfiles]` → `OldFiles`: open the Open Recent picker (`workbench.open_recent`)
  rather than printing a list, since the picker is how fastedit chooses one.
- `:cope[n]` and `:lop[en]` → `OpenProblems` (`problems.show`); `:ccl[ose]` and `:lcl[ose]` → `CloseProblems`
  (hide the Problems panel if shown).
- `:Replace {text}` → `ReplaceInFiles { text }`: user-defined-style (capitalised, as Vim requires of user commands),
  runs the project search panel's Replace All with its current query and `{text}` as the replacement, through the
  same confirmation. No argument is "E471: Argument required"; no query in the panel is "No search to replace".
  Documented in ex.rs's `HostCommand` docs.

`crates/fe-workbench/src/vim_host.rs` runs each by dispatching the feature's command in the window. After a
picker or panel opened from the command line is dismissed, focus is back in the editor in normal mode.

## Tests

New files `crates/fe-vim/tests/ex_day_one.rs` (parsing, abbreviations, errors) and
`crates/fe-workbench/src/vim_day_one_tests.rs`, declared from vim_host.rs.
## Rev 2 (neon lead, 2026-10-17): search_panel.rs in scope for :Replace

`:Replace {text}` must put `{text}` in the project search panel's replace field before `confirm_replace_all`, but
`SearchPanel.replace` is private and `replace_input()` is test-only. `crates/fe-workbench/src/search_panel.rs` is now
in scope for a small `pub(crate)` entry point, e.g. `confirm_replace_all_with(template, window, cx)`, that sets the
replace text and runs the same confirm flow as the panel's own Replace All. No other change to the panel.

## Implementation notes

- fe-vim: the five commands parse as the Change says. `:bro[wse] {cmd}` is a modifier that runs `{cmd}` (a
  terminal Vim ignores it but for `:oldfiles`); `:bro` alone is E471. `:Ex!`, `:ol!`, `:copen!`, `:ccl!` and
  `:Replace!` are E477. `:Replace` is matched by its full name only, so `:Replaced x` stays a `User` command.
- fe-workbench: `:Ex` shows and focuses the project tree and reveals the line's editor's file; `:oldfiles` runs
  `RecentOp::Open` (its dismissal focuses the editor, in normal mode); `:copen`/`:lop` run `problems.show`;
  `:ccl`/`:lcl` toggle the dock off only while it shows Problems.
- `:Replace {text}` (rev 2): `SearchPanel::confirm_replace_all_with` puts `{text}` in the replace field, shown, and
  runs `confirm_replace_all`, so it asks the same question and replaces the same files; nothing while a replacement
  runs. An empty query answers "No search to replace" before the panel is asked.
