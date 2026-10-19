---
task_uid: "01m59bvk90p44hexkan09ebk5x"
task: "T221"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test changelist_matches_neovim (top level, in crates/fe-vim/tests/changelist_oracle.rs) passes on crates/fe-vim/tests/corpus/changelist.toml, at least 16 cases recorded with nvim 0.12.x, among them (asserted by name) g_semicolon_last_change, g_semicolon_count, g_comma_back, close_changes_merge, insert_session_position, quote_dot, backtick_dot, d_quote_dot, changes_follow_deleted_rows, at_oldest_fails, at_newest_fails, empty_list_fails, visual_g_semicolon."
    required: true
    manual: false
---

# fe-vim: g; and g, go through the changelist, and '. and `. go to the last change, as nvim does (Mac T905)

## Problem

At Mac main 6f032b12 fe-vim has no changelist: `g;` and `g,` cancel, and the `.` mark (`'.`, `` `. ``) is
unknown (`crates/fe-vim/src/lib.rs` Known limits: "`'.` … the changelist … aren't supported yet"). VSCodeVim
and IdeaVim have both. The user approved them in the "Vim parity" track on 2026-10-17.

## Change

- The changelist lives with the marks, as the jumplist does (`crates/fe-vim/src/marks.rs`): each change
  (every undoable change a vim command or an insert session makes) records where it was, following edits as
  the jumplist's entries do; nvim's rules for merging a change close to the previous one, the 100-entry cap
  and where an insert session's position is taken, by probing.
- `'.` and `` `. `` are the latest change's position as marks (linewise and exact), as motions and operator
  targets.
- New feature module `crates/fe-vim/src/feature/changelist.rs`, declared in `crates/fe-vim/src/feature.rs`
  and listed in `FEATURES`, its `g` hook taking `;` and `,` in normal and visual mode: `vim.change_older` /
  `vim.change_newer {"count"}`, moving through the list as `:help g;` says, failing E662/E663 at its ends and
  E664 when it's empty, changing nothing.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/changelist.toml`, recorded into
`changelist.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/changelist_oracle.rs` modelled on `tests/jumplist_oracle.rs`.

## Implementation notes

- `'.` and `` `. `` were already marks (`Changes` in `marks.rs`, with `marks_special.toml` covering them); the
  changelist is kept beside them: each of vim's batches that sets `'.` makes it the newest entry, by nvim's
  `changed_common` rule (a new transaction adds one unless the newest is on its row within 79 columns; once
  one is added, the rest of the transaction takes the newest's place; undo and redo start nothing new).
  Entries follow edits as jumplist entries do, moved by `Marks::displace` and `Marks::cutting` too.
- nvim's `g;` and `g,` in a visual mode beep (`checkclearopq`), staying in the mode, so `vim.change_older`
  and `vim.change_newer` are taken there but fail with nothing changed, rather than moving as the spec
  supposed; after an operator they aren't taken (the operator is cancelled, as nvim's is). Neither is a jump.
- The oracle's load (`nvim_buf_set_lines`) is itself a change, so nvim's list always starts with an entry at
  (0, 0). The cases make their first change on row 0 near column 0, which takes that entry's place, and the
  two empty-list cases feed nvim `<Esc>` (`oracle_keys`), which changes nothing as E664 doesn't.
- Known limits, in `feature/changelist.rs`: a macro's run is one undoable change in nvim (one entry), but each
  of its changes in fe-vim; `:s` leaves `'.` at its first match where nvim's is at column 0 of the row
  (a pre-existing `'.` gap; fixing it needs a `Note` from `substitute.rs`, outside this card's scope).
- `crates/fe-vim/src/lib.rs` Known limits still lists `'.` and the changelist as unsupported; it's outside
  this card's scope, so a later card should drop them.
