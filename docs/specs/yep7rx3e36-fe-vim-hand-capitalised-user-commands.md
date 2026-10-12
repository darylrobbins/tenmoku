---
task_uid: "01m4rhrn9hc2en4myep7rx3e36"
task: "T548"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::ex_user_commands::user: ':Gitsigns stage_hunk' gives User { name: \"Gitsigns\", bang: false, range: None, args: \"stage_hunk\" }; \":'\u003c,'\u003eGitsigns stage_hunk\" after selecting rows 3-5 gives range Some(2..5) (0-based rows); ':Git! commit --amend' gives bang true, args 'commit --amend'; ':gitsigns' stays E492"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::ex_user_commands::diff_commands: ':diffget', ':diffg', ':3,4diffget', ':diffput', ':diffpu' give DiffGet/DiffPut with the cursor row or rows 2..4; ':diffsplit a.rs' and ':diffs a.rs' give DiffSplit { path: \"a.rs\" }; ':diffsplit' is E471"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::ex_user_commands::zz_zq: ZZ hands the host what ':x' does (WriteQuit { only_if_modified: true }), ZQ what ':q!' does (Quit { force: true }); 'Zx' cancels and changes nothing"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench vim_host still compiles with the new variants answering E492, E99 and 'Not supported yet: :diffsplit'; cargo test -p fe-vim -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: hand capitalised user commands, :diffget, :diffput and :diffsplit to the host, and run ZZ and ZQ as :x and :q!

## Problem

The user's vim-mode requirement (coordinator, 2026-10-12): every new git and diff feature must be reachable from vim
normal mode through ex commands following fugitive and gitsigns (`:Git blame`, `:Gdiffsplit`, `:Gwrite`, `:Gitsigns
stage_hunk`, with visual ranges), vim's own diff commands (`:diffget`, `:diffput`, `:diffsplit`), and `ZZ`/`ZQ` in the
commit box. fe-vim's ex line hands the host only `:w :q :wq :x :e :qa :wa` (`HostCommand`,
`crates/fe-vim/src/ex.rs` :232); anything else is `E492: Not an editor command`. `ZZ` and `ZQ` aren't implemented.

## Change

- `crates/fe-vim/src/ex.rs`:
  - A command name starting with an ASCII uppercase letter (Vim's rule for user commands, `:h user-commands`) that
    isn't one of fe-vim's own parses as `HostCommand::User { name: String, bang: bool, range: Option<Range<u32>>,
    args: String }`: `range` the buffer rows the line's range resolves to (`:'<,'>Gitsigns stage_hunk` gives the
    visual rows; no range given, `None`), `args` the rest of the line trimmed. fe-vim doesn't know which user commands
    exist: the host answers `E492: Not an editor command: <line>` for one it doesn't know.
  - `:diffg[et]` and `:diffpu[t]` with an optional range (default: the cursor's row) parse as `HostCommand::DiffGet {
    rows: Range<u32> }` and `HostCommand::DiffPut { rows }`; `:diffs[plit] {file}` as `HostCommand::DiffSplit { path:
    String }` (`E471: Argument required` without one). Abbreviations as nvim's.
- New feature module `crates/fe-vim/src/quit_keys.rs` (listed in `crates/fe-vim/src/feature.rs`'s `FEATURES`, `mod`
  in `crates/fe-vim/src/lib.rs`): normal-mode `ZZ` runs what `:x` runs and `ZQ` what `:q!` runs (`:h ZZ`), so the host
  sees the same `HostCommand`s; `Z` followed by any other key cancels.
- T522 (inbox) adds `:Replace` as its own `HostCommand` variant; whichever of the two lands second keeps fe-vim's own
  capitalised commands (`:Replace`) matched before the `User` fallback.
- `crates/fe-workbench/src/vim_host.rs`: arms for the new variants so the match stays exhaustive: `User` answers
  E492, `DiffGet`/`DiffPut` answer `E99: Current buffer is not in diff mode`, `DiffSplit` answers `Not supported yet:
  :diffsplit` (the git-commands card replaces these).

## Tests

New file `crates/fe-vim/src/tests/ex_user_commands.rs` (a `mod` line in `crates/fe-vim/src/tests/mod.rs`).
