---
task_uid: "01m4r0dm96pr5cs0vhrb3hntgd"
task: "T41"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests tests::jumplist::*: every jump that sets the jump mark (and m') also adds where it left from (at most 100 entries; when ctrl-o or ctrl-i runs, an earlier entry on a later one's row goes, then, not yet gone back, the newest entry if it's on the cursor's row); ctrl-o goes count entries back (the first from the newest end adding the current position and setting the jump mark there) and ctrl-i or tab forward, to the entry's exact position; entries follow edits as marks do, and one whose row was deleted goes to the row that took its place, as nvim's does, or past the last row (E19); past either end fails with nothing changed; a :g adds one entry, from where it began"
    required: true
    manual: false
  - id: "C2"
    text: "vim.jump_older and vim.jump_newer {count} are registered from src/jumplist.rs (args pub in fe_vim::jumplist, context editor \u0026\u0026 vim.mode == normal, mutates_buffer false); in visual and operator-pending modes ctrl-o, ctrl-i and tab are not taken"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test jumplist_matches_neovim (tests/jumplist_oracle.rs): tests/corpus/jumplist.toml has at least 30 cases (chains of G, gg, /, % and mark jumps, counts, two jumps from one row, dd deleting an entry's row, rows put in above an entry, the jump mark mixed in, past both ends as the last key); each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes; nothing outside src/jumplist.rs, src/marks.rs, its test file, oracle test and corpus changes, except src/lib.rs (pub mod jumplist, for C2, and its Known limits line) and src/ex/global.rs (a :g's one jumplist entry), and the spec's Implementation notes"
    required: true
    manual: false
---

# fe-vim: the jumplist, with ctrl-o and ctrl-i (tab) back and forward through jumps, checked against nvim (Mac T419)

Filed on argon from Mac T419 (card ~/coordination/argon-cards/T419.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12-argon-i.md). Mac task uid 01m4p2z39fpferst75krv180cn. Its dependencies (Mac T414) are already on argon main (synced to mac/main 6d07f26), so they are not recorded here. Built on argon main synced to mac/main f5bee8c. Title the host commit "... (Mac T419)".

# fe-vim: the jumplist, with ctrl-o and ctrl-i (tab)

## Problem

fe-vim keeps only the jump mark `''` (`marks.rs` `Marks::jumped`); "the jumplist (`<C-o>`,
`<C-i>`) … aren't supported yet" (`src/lib.rs` Known limits ~575; gap analysis 2026-10-11, distinct
from the `gs`/`gS` jump cards T366–T369). `<C-o>` is how nvim users go back after `gg`, `G`, `/`,
`%` or a mark jump. The seam card this depends on declared the empty `src/jumplist.rs` (in
`FEATURES`) and made `oracle.lua` clear the jumplist before each case.

## Change

- **The list** (`src/marks.rs`, beside the jump mark, per `VimState` as marks are): every jump
  that sets `''` (`Marks::jumped`: `gg G % / ? n N * #`, mark jumps, `:{N}`, and any motion whose
  `is_jump()` is true) also adds where it left from, as a `Place` so it follows edits as marks do.
  At most 100 entries; an earlier entry on the same row as the new one is removed, as nvim does
  with its default 'jumpoptions'. An entry whose row is deleted is dropped when it would be used.
- **The keys** (`src/jumplist.rs`): `FEATURE.key` takes `<C-o>`, `<C-i>` and `<Tab>` in normal
  mode with nothing but a count pending, and commits `vim.jump_older` / `vim.jump_newer`
  `{"count"}` (args `pub` in `fe_vim::jumplist`; registered by `FEATURE.register`, context
  `editor && vim.mode == normal`, `mutates_buffer` false). `<C-o>` goes `count` entries back (the
  first one from the newest end adds the current position, so `<C-i>` returns), `<C-i>` forward;
  each goes to the entry's exact position and sets `''`. Past either end it fails with nothing
  changed. In a visual or operator-pending mode the keys aren't taken (as today).
- `src/jumplist.rs`'s module docs describe the keys and the list.

**Clean room**: `nvim --clean` (0.12.4) and `:help jumplist`, `:help CTRL-O`,
`:help 'jumpoptions'`; never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/jumplist.toml`, at least 30 cases: chains of `G gg /pat<CR> % 'a` then `<C-o>` and
`<C-i>`/`<Tab>`, counts, jumps from the same row twice, `<C-o>` after `dd` deleted an entry's row,
after text inserted above an entry, `''` mixed in, and past both ends (the failing key last).

## Tests (new files)

- `src/tests/jumplist.rs` (the seam's stub).
- `tests/jumplist_oracle.rs` (`#[allow(dead_code)] mod support;`) with `jumplist_matches_neovim`.

## Size

About 250 non-test lines.

## Out of scope

Jumps across files (the host's), `:jumps`, `:clearjumps`, the changelist (`g;` `g,`), routing `tab`
and `ctrl-o` to vim in the app's keymap (the default keymaps bind `tab` to `editor.tab` in
`editor`: a GPUI card, with T230/T231), the GPUI check. Any change outside the files in scope.

## Implementation notes

Probed against `nvim --clean` 0.12.4 (black box, `getjumplist()` and cursor
positions), nvim differs from this spec's wording in places; fe-vim follows
nvim, and every case in `tests/corpus/jumplist.toml` matches it:

- **A deleted row's entry isn't skipped.** It goes to the row that took its
  place at the same column (`dd`, `:d`, `cc`'s other rows), or past the last
  row when no row did (`dG`, `dd` on the last row), where going to it fails
  with E19, moves nothing and is passed over by the next `<C-o>`. Corpus:
  `dd_entry_row*`, `ex_d_entry_row`, `dd_last_row_entry`,
  `dG_entry_past_last_row`, `cc_over_entry_rows`.
- **Same-row removal is lazy.** A jump just appends; when `<C-o>`/`<C-i>`
  runs, an earlier entry on a later one's row goes first, and then (before
  any going back) the newest entry if it's on the cursor's row, even when
  the key then fails (`tab_drops_newest_on_cursor_row`,
  `failed_back_drops_newest_on_cursor_row`). Entries past the last row
  count as one row, the one after it (`entries_past_last_row_*`). So the
  100-entry limit counts
  duplicates (`oldest_dropped_by_alternating`), and the entry the first
  `<C-o>` adds removes an older one on its row only from the next use
  (`from_the_row_of_an_older_entry`).
- **Only the first `<C-o>` after a jump sets `''`** (where it adds the
  cursor's position); later `<C-o>`s and `<C-i>`/`<Tab>` leave it alone.
  Landing where it left, `''` goes back as for any jump (`marks::settle`).
  A first `<C-o>` whose count fits the list as it was adds its entry and
  steps onto the newest one for good, even if the count then reaches past
  the oldest (the oldest dropped at 100 entries, or nothing added in a
  `:g`); one past it from the start changes nothing
  (`hundred_entries_*past_oldest_then_back`,
  `ctrl_o_in_global_normal_*too_far*`).
- `m'` and `` m` `` add an entry (`:help jumplist`), as do `:s`, `:d`, `:>`,
  `{count}zt` and the like (everything that calls `jump_here`); `:{N}` and
  `ma` don't. A `:g` adds one entry, where it began, before its commands
  run (so the entry follows them: `global_from_deleted_last_row`,
  `global_keeps_entry_column`), and their own jumps add none
  (`globals_add_one_entry_each`): `Marks::add_jump` and `Marks::keep_jumps`
  around the command loop in `src/ex/global.rs`, which sets `''` after it
  with `Marks::set_jump` (adding nothing) where it called `jumped` twice.
  A `<C-o>` its `:normal` runs from past the newest entry adds nothing
  either, but counts back as if it had (`ctrl_o_in_global_normal*`; those
  cases feed nvim a literal ctrl-o as `oracle_keys`, as fe-vim's `:normal`
  reads `<C-o>` as notation).
- Outside the listed files: `src/lib.rs` makes `jumplist` a `pub mod` (C2's
  `fe_vim::jumplist::JumpArgs`) and drops the jumplist from Known limits;
  `src/ex/global.rs` gets the calls above.
- A charwise cut over rows sends entries on the rows between with the last
  row's text (joined to the first), as marks on the last row go; checked for
  the last row (`cut_joins_entry_row*`), not the rows between.
