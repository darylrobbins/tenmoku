---
task_uid: "01m4p2zcqda98nskc9c8vyn0vq"
task: "T424"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests in tests/ex_global.rs: Ex::parse gives :g[lobal][!] and :v[global] with any allowed delimiter and an optional command, and :norm[al][!] {keys}; a nested :g is E147"
    required: true
    manual: false
  - id: "C2"
    text: ":g runs its command on each marked row that still exists with the cursor on it, the whole :g one undo step; :normal runs its keys synchronously through VimState::feed and vim's registry on the cursor's row or on each row of a range (cursor at its start), an unfinished command ending as Esc would and a failing key dropping that row's remaining keys, one undo step"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test ex_global_matches_neovim (tests/ex_global_oracle.rs): tests/corpus/ex_global.toml has at least 40 cases (:g and :v with d, m0, t$, s, j, normal and no command; ranges; other delimiters; :g//d after a search; :normal dw, :%normal A;, :2,3norm! \u003e\u003e, unfinished commands, insert mode left open; u after each); each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes"
    required: true
    manual: false
---

# fe-vim: :g :g! :v over ex commands, and :normal over a range, checked against nvim

# fe-vim: :g :g! :v over ex commands, and :normal over a range

## Problem

`:g/pat/d`, `:v/pat/d`, `:g/pat/normal A;` and `:%norm .` are core nvim editing idioms; fe-vim has
none (`crates/fe-vim/src/ex.rs` ~85–344; gap analysis 2026-10-11). This card depends on the ex line
commands card (same `ex.rs`), so `:g` can run `:m`, `:t`, `:j`, `:y` too.

## Change

### `:g` (`src/ex/global.rs`, new, `mod global;` in `ex.rs`)

- `:[range]g[lobal][!]/{pat}/[cmd]` and `:[range]v[global]/{pat}/[cmd]`: marks every row of the
  range (default all) that matches (`!`/`:v`: doesn't), then runs `cmd` with the cursor on each
  marked row that still exists, in order (`:help :g`). Any single-byte non-alphanumeric delimiter
  but `"`, `|` and `\`; an empty pattern is the last search's; the pattern becomes the last search.
- `cmd` is any fe-vim ex command, default `:p` (which only moves the cursor to the last match).
- The whole `:g` is one undo step; a nested `:g` is E147; no match is E486 (or nvim's `:v`
  message).

### `:normal` (`src/ex/normal.rs`, new, `mod normal;` in `ex.rs`)

- `:[range]norm[al][!] {keys}`: runs `keys` (vim notation, `parse_keys`) on the cursor's row, or on
  each row of the range with the cursor at its start, synchronously inside fe-vim: each key goes
  through `VimState::feed` and vim's own registry (`commands::registry`, with the same
  `VimTarget`), typing in insert and replace mode through the model as `HeadlessVim::feed_key` does
  (fe-vim's runner is its own; `headless.rs` doesn't change). A command left unfinished at the end
  ends as `<Esc>` would; a failing key drops that row's remaining keys. `!` is the same (fe-vim has
  no remaps). `.` and registers behave as typed keys do.
- One undo step for the line. A `:normal` that opens a command line (`:normal :d<CR>`) runs it.

**Clean room**: `nvim --clean` (0.12.4) and `:help :g`, `:help :v`, `:help :normal`; never source,
hjkl, Zed or Helix.

### Corpus

`tests/corpus/ex_global.toml`, at least 40 cases: `:g/x/d`, `:g!/x/d`, `:v/x/d`, `:g/x/m0`
(reversing), `:g/x/t$`, `:g/x/s/a/b/`, `:g/x/j`, `:g/x/normal Ay`, `:g/x/` alone, ranges, other
delimiters, `:g//d` after a search, nested `:g` (last key), `:normal dw`, `:%normal A;`,
`:2,3norm! >>`, `:normal` with an unfinished command, insert mode left open, `u` after each.

## Tests (new files)

- `tests/ex_global.rs`: parsing (`Ex::parse`) of `:g`, `:v` and `:normal` with their errors.
- `tests/ex_global_oracle.rs` (`#[allow(dead_code)] mod support;`) with `ex_global_matches_neovim`.

## Size

About 350 non-test lines.

## Out of scope

`:exe`, `:windo`/`:bufdo`, `|` between commands (`:g/x/d|s/a/b`), the
GPUI check.

## Conflicts

`src/ex.rs` is also in T357's scope (inbox).

## Implementation notes

Probed against `nvim --clean` 0.12.4; where nvim and `:help` disagree, fe-vim follows nvim:

- Delimiters: nvim takes any character but a letter (E146), `"`, `|` and digits included, and
  reads `\/`, `\?` and `\&` as the last search's and substitute's patterns (other `\` is E10); a
  blank before the delimiter is skipped (`:g !x!d` delimits with `!`). fe-vim does the same.
- Nothing marked is a message, not an error, in nvim (`Pattern not found: x`, no E486; `:v`'s
  `Pattern found in every line: x`); fe-vim matches. An error from the command stops the `:g`
  there; a `:s` finding nothing on a row doesn't. The pattern becomes the last search, keeping
  its direction.
- Nested `:g`: as the spec says, E147 at parse and at run time (via `:normal`). nvim gives E147
  only with a range; without one it runs the inner `:g` on the current line. Documented in
  `ex.rs`.
- Which marked rows still exist follows nvim's line marks: deleted, joined into the row above,
  or moved by `:m` (noted by `:m` itself, since fe-vim rewrites rows in place). Followed per edit
  batch through a buffer subscription and a Fenwick tree over the marks, so `:g/^/d` stays
  O(n log n). A join (`:j`, `J`, visual `J`, `.` repeating one) and `:sort` (every row of its
  range, as nvim) note what they did too, since a join's edits read like a delete's (from an
  empty row) and `:sort` rewrites rows in place.
- Cursor and undo: `:g` keeps the goal column (nvim's curswant) on each row; `:normal` over a
  range keeps it only while it's still valid (a change makes it the line's start), and goes by
  line number, clamped to the last. `u`/`<C-r>` go where the first command that changed the
  text noted (nvim's). The jump mark is where the cursor was before the `:g`.
- `:normal` keys are vim notation (`parse_keys`), as the spec says; nvim's `:normal` reads them
  as typed text. Its keys aren't recorded and don't count against a replay under way, but a
  recording or `@` they start stays: this needed `Macros::hold`/`release` in `src/macros.rs`
  (outside the listed scope; the replay bookkeeping is private there). Typed text goes in under
  the `:normal`'s actor. An undo or redo among the keys closes the group's transaction (as
  nvim, it undoes what the line did so far); what follows goes in a new one, the undo step.
  Nesting stops at 100 levels with E192 (nvim's 'maxmapdepth' is 1000; each level runs on the
  thread's stack).
