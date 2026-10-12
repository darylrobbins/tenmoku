---
task_uid: "01m4r0dmexy7m0q7e8ba4hsjck"
task: "T42"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests tests::marks_special::*: after a yank, put, delete, change, insert, shift or visual operator the [ and ] marks are the first and last char of the text it yanked, put or changed (rows for a linewise one, as nvim records); the . mark is where the last change was; the ^ mark where insert mode was last left; m[ and m] set them; mA to mZ set marks jumped to as a to z are (the behaviour as built at candidate f804b20). Deferred to a follow-up card, not required here: review findings F18 (mark freshness via the repeat counter), F21 (short-row block-yank endpoints), F22 (corrected . mark only on the closing outcome), F23 (counted inserts with an empty final segment), F24 (characterwise deletion of complete rows read as linewise), and any later nvim-parity findings in the same areas."
    required: true
    manual: false
  - id: "C2"
    text: "The [ ] and . marks come from the edit batches Marks already follows (vim's own actor), so only src/operator.rs (a yank) and src/insert.rs (leaving insert mode) set marks directly; marks::settable and marks::jumpable accept the new names"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test marks_special_matches_neovim (tests/marks_special_oracle.rs): tests/corpus/marks_special.toml has at least 35 cases (the [ and ] marks after yiw, yy, p, P, dw, dd, cw, \u003e\u003e, x and a visual d, linewise, the . mark after a change and after undo, the ^ mark after i and A, mA then its quote and backtick jumps and a d to it, rows put in above); each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes"
    required: true
    manual: false
---

# fe-vim: the [ ] . ^ marks and uppercase marks A-Z, checked against nvim (Mac T420)

Filed on argon from Mac T420 (card ~/coordination/argon-cards/T420.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12-argon-i.md). Mac task uid 01m4p2z5cx01cmd34s6e61prae. Its dependencies (Mac T414) are already on argon main (synced to mac/main 6d07f26), so they are not recorded here. Depends on argon T41 (Mac T419). Built on argon main synced to mac/main f5bee8c. Title the host commit "... (Mac T420)".

# fe-vim: '[ '] '. '^ marks and uppercase marks A-Z

## Problem

"Marks: uppercase and numbered marks, `'[ ']`, `'.`, `'^`, `'"` … aren't supported yet"
(`src/lib.rs` Known limits ~575; gap analysis 2026-10-11). `` `[ `` and `` `] `` (the last changed
or yanked text) and `` `. `` are common in nvim workflows and in plugins' expectations. The seam
card made the reducer's `m` and `'`/`` ` `` prefixes ask `marks::settable` / `marks::jumpable`
and made `oracle.lua` clear `A-Z0-9` marks before each case. This card depends on the jumplist card,
which also changes `src/marks.rs`.

## Change

In `src/marks.rs`, with the two places that edit no text or end a session:

- `marks::settable` takes `a-z A-Z ' \` [ ]` and `marks::jumpable` also `. ^ < >`.
- **`'[` and `']`** (`` `[ `] ``): the first and last char of the text last changed, put, or
  yanked, as nvim sets them for each command (`:help '[`), including a linewise change (first and
  last row, columns as nvim records). Changes, puts and deletes come from the edit batches `Marks`
  already follows (`Feed`), filtered to vim's own actor, so the change sites don't change; a yank
  edits nothing, so `src/operator.rs`'s yank sets them directly. `m[` and `m]` set them.
- **`'.`**: where the last change was made (`:help '.`), from the same batches.
- **`'^`**: where insert mode was last left (`:help '^`), set by `src/insert.rs` when a session
  ends.
- **`mA`–`mZ`**: set and jumped to as `a–z` are, kept per `VimState` (this view) like the others.
- `src/marks.rs`'s module docs describe the new marks; `src/lib.rs`'s Known limits are left for a
  docs pass (lib.rs is not in scope).

**Clean room**: `nvim --clean` (0.12.4) and `:help '[`, `:help '.`, `:help '^`, `:help mark-motions`;
never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/marks_special.toml`, at least 35 cases: `` `[ `` and `` `] `` after `yiw`, `yy`, `p`,
`P`, `dw`, `dd`, `cw…<Esc>`, `>>`, `x`, a visual `d`; `'[` / `']` linewise; `` `. `` after a change
and after undo; `` `^ `` after `i…<Esc>` and `A…<Esc>`; `mA` / `'A` / `` `A `` and `d'A`; marks
after rows put in above. An unset mark's jump is a case's last key.

## Tests (new files)

- `src/tests/marks_special.rs` (the seam's stub).
- `tests/marks_special_oracle.rs` (`#[allow(dead_code)] mod support;`) with
  `marks_special_matches_neovim`.

## Size

About 250 non-test lines.

## Out of scope

`'"`, numbered marks `'0`–`'9`, file marks across buffers (an uppercase mark jumping to another
file is the host's), `:marks`, `:delmarks`, the changelist, the new marks in ex ranges (`ex.rs`),
the GPUI check. Any change outside the files in scope.

## Implementation notes

- `'[`, `']` and `'.` live in `marks::Changes`, read from the same `Feed` batches `Marks` follows.
  A batch counts as vim's own in three cases:
  - fe-vim noted carets for its transaction (`history::Carets`, which nearly every vim edit
    records; undo and redo batches carry the transaction they undo);
  - its transaction is an insert session's (`Marks::own`), whose carets record is only kept once
    the session ends;
  - it was typed in that session.
  This replaces filtering by origin: an agent's vim command sets the marks, and nothing else does,
  whoever made it. Batches wait in `Changes::pending` until a caller that has the vim state reads
  them (`marks::catch_up`). `commands::follow`, the common hook every editing command runs first, now
  calls `marks::follow`. That catches up, follows, and starts the feed, so the first command of all
  (`r`, `~`, `J`, ...) is seen.
- A batch says where the text changed but not which command changed it. Where its shape can't
  tell, the command that made it notes what it knows, keyed by the version the batch made
  (`marks::Note`):
  - a linewise delete's begin column (the caret's, or the visual start nvim uses) — whole-text
    deletes count as rows only with this note;
  - a shift's rows and begin column (`operator.rs`, and block shifts in `visual/block.rs`);
  - a linewise put's rows, and the row `o` or `O` opened (`insert.rs`);
  - a join (`edit.rs`).
  Everything else is read by shape: whole rows taken out, whole rows put in, anything else, and
  undo/redo, whose batches in one transaction span all the rows they change. Puts and shifts leave
  `']` at a fixed column. Only a linewise yank's `']` follows its row's end.
- Directly set, because nothing is edited: a yank (`marks::yanked`, from `operator.rs`, and from
  `visual/block.rs` for a block, at the graphemes under its display columns), and a shift that
  changed nothing (`marks::shifted`). Leaving insert mode (`marks::left_insert`, from `insert.rs`)
  sets these:
  - `'[` where the session began: a `Place` noted when the session opened, which neither typing at
    its row's start nor `<BS>` before it moves;
  - `']` and `'^`; over a block, once the other rows are typed, from an anchor, with `']` at the
    end of the last row's text for `I` and `A`;
  - `'.` at the start of the last copy for a count, or on the first other row of a block.
  The outcome goes into the shared `Closed` record. A clone ending the same session adopts it only
  when it has made no newer change.
- `'^` lives with `a`–`z`. `A`–`Z` are kept as jumplist `Entry`s: nvim keeps a file mark when its
  row is deleted, on the row that takes its place. A mark past the last row gives E19, where an
  unset one gives E20. When a spot is newer than a stale model's snapshot (another view edited the
  shared buffer), it is kept as it is rather than dropped.
- Outside the scope list, beyond the calls above (`commands.rs`, `edit.rs`, `visual/block.rs`): two
  earlier assertions said the old mark names were the only ones, which C1 contradicts. I changed
  just those two, in `src/tests/feature_stubs.rs` (the seam's "mark names are today's" check now
  lists the new names) and `src/tests/marks.rs` ("`m` takes no other names", now checked with `m^`
  instead of `mA`). Every earlier corpus expected file is byte-identical after re-recording;
  `marks_special.toml` has 128 cases.
- Not matched, and not in the corpus:
  - `gJ`, which fe-vim doesn't have;
  - ex commands that delete rows (`:d`, `:%d`) don't note them, so deleting all the text there
    reads as charwise.
