---
task_uid: "01m55matsbaa0scn3sa0tg73vz"
task: "T186"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "remap_feed_matches_where_nvim_does (top level, crates/fe-vim/tests/remap_feed.rs, through HeadlessVim): nnoremap H ^ runs for H, 3H and \"aH (a count or register before it), not for dH; onoremap H ^ makes dH delete to the first non-blank; xnoremap applies in visual modes only; the argument of f t F T r m ' ` q @ \" and the second key of g z Z [ ] are read unmapped"
    required: true
    manual: false
  - id: "C2"
    text: "remap_feed_resolves_prefixes: with nnoremap jk x and nnoremap h l, jh runs built-in j then l; an exact match that is also a prefix waits; remap_timeout() runs the longest complete map and re-matches the rest, or with none feeds only the first held key as typed; while waiting, remap_waiting() is true and the stamped context has vim.pending with vim.pending_kind = remap"
    required: true
    manual: false
  - id: "C3"
    text: "remap_feed_recursion: a noremap's keys aren't re-mapped, a map's are, a to starting with its own from doesn't re-map that first key, and nmap a b + nmap b a fails with E223: recursive mapping (failed() true, the expansion's queued keys dropped)"
    required: true
    manual: false
  - id: "C4"
    text: "remap_feed_set_replaced: set_remaps swaps the set atomically and keys held for a longer match go on as typed; a key of an expansion that fails drops the rest of the expansion"
    required: true
    manual: false
  - id: "C5"
    text: "macros.rs, headless.rs, reducer.rs, feature.rs, args.rs, commands.rs and lib.rs are untouched, and every existing fe-vim test passes unchanged"
    required: true
    manual: false
---

# fe-vim: VimState::feed runs n, x and o remaps before the reducer: prefixes wait, the longest map runs on a mismatch or timeout, and recursion stops at E223 (Mac T869)

## Problem

The remap set card gives fe-vim a `RemapSet` and its `lookup`, but no key is remapped yet.
`docs/decisions/vim-remaps.md` (decided 2026-10-17) §4–§5 put the engine in fe-vim, in front
of `step`. Today `VimState::feed` (`crates/fe-vim/src/state.rs`) runs, in order:
`jump::label_key`; the `q` that ends a recording; `macros.command_start`/`record`;
`jump::key`; `Pending::command_line`; then `step`. It sees one key at a time.

All three hosts already feed back, key by key and by mode, the keys
`VimState::take_replay` hands out. They push them in front of the keys they still hold, and
on a failure they drop the rest and call `stop_replay`. The three are `HeadlessVim::feed`
(`headless.rs`), `:normal`'s `Runner` (`ex/normal.rs`) and `EditorView::replay_vim`
(fe-gpui). A remap's expansion needs exactly that: its keys run in order, each in the mode
the one before it left (`:w<CR>` opens the command line, which then takes `w`).

This is the stateful half of the record's card 1, made headless.

## Change

In `crates/fe-vim/src/remap.rs` (`RemapState`) and `state.rs` (a field, a hook and the stamp
only). `macros.rs` (T836 holds it), `headless.rs` (T802), `reducer.rs`, `feature.rs`,
`args.rs`, `commands.rs` and `lib.rs` stay untouched.

- `VimState::set_remaps(&mut self, set: Arc<RemapSet>)` replaces the set atomically. Keys
  held for a longer match are dropped back into the queue as typed, unmapped (§7).
  `VimState::remaps() -> &RemapSet`.
- **Where a key is matched.** `feed` passes a key through the remap stage right after
  `jump::label_key`, before the `q` check (a map of `q` wins, as in nvim). The stage applies
  only when all of these hold:
  - the mode is Normal or Visual;
  - nothing is pending, or only a count, a completed `"x`, or an operator (that is mode
    `o`; Normal with nothing but a count or register is `n`; Visual is `x`);
  - the key isn't the argument of `f t F T r m ' \` q @ "`, nor the second key of `g z Z [ ]`
    (nvim reads those unmapped);
  - the key isn't from a non-recursive expansion.
- **States** (§4 rule 3). Each fed key ends in one of four states. With no match, it goes on
  as typed. An incomplete prefix waits. An exact match that is not also a prefix runs. An
  exact match that is also a prefix waits. While a prefix waits, `feed` returns
  `Step::Wait(pending)` with `pending` unchanged, and `stamp` sets `vim.pending` with
  `vim.pending_kind = remap`, so the host bypasses its keymap. `mode_name` is unchanged.
- **Resolving.** Running a match queues its `to` keys, which the hosts take through
  `take_replay`. A key that breaks the longer match runs the longest complete map, then
  re-matches the unconsumed keys from the start. With no complete map, only the first held
  key goes on as typed, and the rest are re-matched in the mode it leaves.
  `VimState::remap_timeout(&mut self)` resolves the same way; the host calls it after
  `CHORD_TIMEOUT` with no key. `VimState::remap_waiting() -> bool`.
- **Queued keys are tagged.** Each key handed out records whether it is an expansion's, and
  whether it may be re-mapped. The tags are pushed in front, in the order the hosts push the
  keys, so `macros.count()` still counts only a replay's keys. An expansion's key isn't
  recorded. Its `q` acts as typed, not as a replay's ignored `q`.
- **Recursion** (§3). A `recursive` map's keys are matched again. When `to` starts with
  `from`, its first key is exempt. After 100 expansions with no key in between that wasn't
  an expansion's, the remap stage fails: message `E223: recursive mapping`,
  `failed() == true`, and the queued expansion keys are dropped.
- A key of an expansion that fails drops the rest of that expansion, through the hosts'
  existing failure path (nvim flushes the rest of a mapping on an error).

Tests go in the new top-level `crates/fe-vim/tests/remap_feed.rs`, through `HeadlessVim`
(`state_mut().set_remaps(...)`, `feed`, `state_mut().remap_timeout()`).

## Implementation notes

- `RemapState` keeps the keys held for a longer match, the keys it resolved that the host
  hasn't taken, and a tag for each key the host took (`take_replay` hands out a `@`'s keys,
  then the map's). `feed` and `record_key` pop the tag of each key they read: a `@`'s key is
  counted by `macros.count()` as before, a held key going on was counted when first read, and
  an expansion's key is neither counted nor recorded. With no remaps every tag is "typed",
  so nothing changes.
- Typed keys are recorded once, when they're used: a held key when its map runs (the left
  side, as typed), or when it goes on to the reducer.
- A key fed while resolved keys wait to be taken (after `remap_timeout` or `set_remaps`, when
  the host hasn't called `take_replay` yet) is queued after them and `feed` returns
  `Step::Wait`, so the order holds whichever the host does first.
- A `0` while a count is being typed goes on the count, as nvim's `no_zero_mapping`.
- Carried T185 F2: `hides_builtin` names `gs`, `gS` and `gs{c1}`/`gS{c1}` in all three
  modes; a four-key `gs{c1}{c2}` map hides nothing.
- Review round 1: a key handed out keeps its origin through recording (only a key typed by
  the user is recorded, whatever the macros' last count said), the `:s` confirm answer and a
  jump's recording rewrite (an expansion's keys are read with the macros set aside, as
  `:normal` does). A held first key that isn't the user's goes back through the host's
  queue, so its failure stops the map. The keys handed out since a typed key are capped at
  `REPLAY_CAP`, as a `@` that a map plays again restarts the macros' own count.
- Known limit, for card 4: `:normal`'s runner (`ex/normal.rs`) neither sets the tags aside as
  it does the macros (`Macros::hold`) nor resolves a map still held when its keys end.
