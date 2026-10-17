# Vim key-to-key remaps: where they are written, stored and run

Status: **decided** by the user, 2026-10-17 (see "Questions for the
user"). Source card T62 (Mac T425). Follows `vim-engine.md` §10, which left
remaps "not designed yet". Clean room: fastedit's code and docs, and nvim's
`:help map-overview`, `:help :noremap`, `:help mapleader`, `:help 'timeoutlen'`
and `:help map-modes`. No code changes.

## Problem

nvim users bring remaps: `nnoremap Y y$`, `nnoremap H ^`,
`nnoremap <leader>w :w<CR>`, `inoremap jk <Esc>`. Without them fastedit's vim
mode can't be made to feel like the user's own. Today there are none:

- `fe-keymap` maps keystrokes (and chords) to commands, by key context. It has
  no notion of "type these other keys instead".
- `fe-vim`'s `VimState::feed(VimKey)` (`state.rs`) runs `step` (`reducer.rs`)
  on the pending input, then the feature hooks (`feature.rs`: `key`, `g`, `z`, `object`)
  for keys the reducer rejects. It sees one key at a time and keeps no
  look-ahead.
- `fe-gpui/src/keymap.rs` matches the keymap first in every mode. A match to
  `vim.key` hands the chord's keys to vim. An unmatched key goes to vim while
  `vim.pending` is set or the mode is normal/visual. A failed chord re-delivers
  its swallowed keys, which is how `jk` in insert mode works today; the chord
  timeout is `kiln_platform::keys::CHORD_TIMEOUT` (1 s, re-exported by
  `fe-gpui/src/keymap.rs`).
- Macros (`macros.rs`) record typed keys as register text; `@` queues them for
  the host to feed back through `feed` or `record_key`.
- `:normal[!]` exists (`ex/normal.rs`): it feeds each key through
  `VimState::feed` and vim's registry on each line. Its parser reads `!` but
  today discards the distinction, since fe-vim has no mappings.
- GPUI's macro runner (the host's replay of `@`): `EditorView::replay_key`
  (`fe-gpui/src/vim.rs`) types a replay's keys into a command line it doesn't
  show, and drops the host command (`:w`) that such a line leaves, with "a
  macro replay doesn't run …".

## Options

### 1. Where remaps are written

**(a) A table in `settings.toml`'s `[vim]` table.**

```toml
[[vim.remap]]
mode = "n"            # n, x, o, i, c (or a list)
from = "H"
to = "^"
recursive = false
```

The table is `[[vim.remap]]`, with `vim.leader`, read by fe-config's
`VimSettings` (`crates/fe-config/src/settings.rs`). It is not in the keymap
file: that file is kiln's (`kiln_core`'s loader, through
`kiln::Config::keymap`), and vim's settings already live in `VimSettings`.

Fits M5's config model: one file, hot reload, "invalid config keeps last
good". Verbose for the common one-liner, and no way to paste an existing
`init.vim`.

**(b) A vimrc subset file**, `~/.config/fastedit/vimrc`: `nnoremap`, `xnoremap`,
`onoremap`, `inoremap`, `cnoremap`, the `map`/`noremap` all-mode variants and
`nmap`-style recursive ones, `unmap`, `let mapleader = …`, and `"` comments.
Anything else is a diagnostic ("not supported"), never run. Pasteable from
nvim, but it's a second config language, with its own parser, hot reload and
diagnostics.

**(c) Both, (b) compiled to (a).** The vimrc parser produces the same
`Remap { mode, from, to, recursive }` records as the TOML table; one engine
runs them. Costs the parser on top of (a), but nothing at run time.

### 2. Modes

nvim's `n`, `x` (Visual only; `v` in `:map` is Visual and Select together),
`o` (operator-pending), `i`, `c`, plus `l`, `t` and Select mode (`s`), which
fe-vim has no mode for: `vnoremap`/`vmap` apply to Visual only and load with
a note that Select isn't there, `snoremap` is a diagnostic. Order of value: `n` (almost every remap),
`i` (`jk`, but already a keymap chord today), `x`, `o`, `c`. `fe-vim`'s modes
map directly: Normal without an operator → `n`, Normal with an operator
pending → `o` (not `n`: `nnoremap H ^` doesn't apply after `d`; `dH` needs
`onoremap H ^`), Visual(_) → `x`, Insert and Replace → `i`, CmdLine → `c`.
Insert and cmdline text is read by the host, not `feed` (`record_key`), so
`i` and `c` remaps need the host-side hook below, not just `feed`.

### 3. Recursive maps

`noremap` right sides are not re-mapped; `map` right sides are, so
`nmap a b` + `nmap b c` makes `a` run `c`. A right side that begins with its
own left side (`nmap a a`, `nmap ab abx`) isn't a loop: nvim doesn't re-map
that first character, so the engine doesn't either. Only the **first
character** of the right side is exempt, and only when the right side starts
with the whole left side; every later key is mapped as usual, in the mode the
earlier keys left. With `nmap ab abx` and `imap b B`, typing `ab` runs `a`
literally (it enters Insert mode), and the `b` after it is then mapped by
`imap b B`, not read literally. The corpus confirms the exact rule (see Testing). A real cycle (`nmap a b`
with `nmap b a`) stops with `E223: recursive mapping`. nvim stops with `E223: recursive
mapping` after `'maxmapdepth'` (default 1000) levels. We'd do the same with a
fixed limit of 100 and a status-bar message. A `<Plug>`-style indirection is
the only common use of recursive maps and fastedit has no plugins; so the
default is non-recursive and recursive is cheap but arguably not needed.

### 4. Multi-key left sides, ambiguity and timeout

`nnoremap gb :ls<CR>` and `nnoremap g <Something>` overlap: with both `gb` and
`gg`-like built-ins, vim waits for more keys when a typed prefix matches a
longer map, and gives up after `'timeoutlen'` (1000 ms), then runs the shorter
meaning. fastedit has two owners of a key sequence:

- the **keymap chord engine**: first, in every mode (§10 step 1), with
  `kiln_platform::keys::CHORD_TIMEOUT` and re-delivery on failure;
- **fe-vim's remap table**: only for keys the keymap didn't take.

Rules, in order:

1. **The keymap wins where it matches** (§10 step 1), but not while a vim
   command is pending: `fe-gpui/src/keymap.rs` bypasses the matcher then and
   the keys go to vim. So a remap's left side is reachable in normal mode
   when the keymap doesn't bind it, and also after an operator, count or
   register. A load diagnostic ("`jk` is bound in the keymap in
   `vim_mode == insert`; the keymap wins there") is raised only when the
   same mode and context do bind that sequence (or a prefix of it) in the
   keymap, so it's per mode and context, not for every key. `inoremap jk
   <Esc>` is the one real conflict, and the keymap chord is the answer.
2. **Remap-prefix ownership.** While fe-vim holds a typed prefix waiting for
   a longer map, it owns the keys: the keymap isn't consulted for them,
   as with a pending vim command (the host already routes there while
   `vim.pending` is set; the remap state sets it). The state is
   published as `vim.pending` so the host and status bar show it.
3. **Exact match and incomplete prefix are separate states.** After each key
   the state is one of: no match (feed the keys as typed), incomplete prefix
   only (wait), exact match only (run it), or exact match that is also a
   prefix of a longer map (wait; this is the ambiguity). On timeout
   (`kiln_platform::keys::CHORD_TIMEOUT` as `'timeoutlen'`; `vim.timeoutlen` can come later) or a
   key that breaks the longer match, the **longest completed mapping runs**
   and the unconsumed suffix is processed again from the start, as new keys
   (so it may match another map, or go to `feed` as typed). With no
   completed mapping, only the **first** buffered key is consumed literally
   (fed as typed, its invocation dispatched), and the remaining buffered keys
   are matched again against the remaps in the mode that key left. With
   `nnoremap jk x` and `nnoremap h l`, typing `jh` runs built-in `j`, then
   the mapped `l`, not `j` and `h`.
4. **Shadowing built-ins is permanent for complete maps.** A complete map
   `nnoremap g x` hides built-in `g` prefixed commands for good, like nvim
   (`gg` then needs a map or a wait); a map that is only a prefix of a built-in
   (`<leader>g`, left side longer than the built-in's) shadows nothing.
   A load note names a left side that hides a built-in.

### 5. Where it runs

**(i) In fe-vim, in front of `step`.** A `RemapState` inside `VimState`, called
by `feed` first; it returns the keys to run (zero while waiting for more,
one or more to feed). This is the only place that sees counts, registers,
operator-pending and `.`, which are all `Pending` state.

**(ii) In the keymap.** Remaps become keymap bindings to a `vim.key` command
with a key list. The keymap is mode-aware already, but it can't see "an
operator is pending" or a count, and its chord engine has no recursion or
`noremap` distinction. It would also make `from` keys that vim's `Pending`
legitimately uses (`2`, `"`, `a` after `d`) unreachable.

**Decision: (i)**, with the keymap still first (rule 1 above).

How (i) meets the rest of fe-vim:

- **Feature hooks.** `feature.rs` now has hooks `key`, `g`, `z` and `object`
  (the char after `i`/`a`), nine features in `FEATURES`, and features that add
  motions, text objects and operators as `Motion`, `TextObject` and `Operator`
  variants in `args.rs`, committed through `Pending::motion`,
  `Pending::with_operator` and `Pending::operate`. All of that is downstream
  of `step`: remaps run before `step`, so a remapped key's output reaches the
  hooks as typed keys, including `gU`-style operators, `i`/`a` objects and
  the jumplist keys. The remap module registers no `Feature` and adds no
  `Motion`, `TextObject` or `Operator` variant; it is a field on `VimState`.
- **Counts and registers.** `3H` with `nnoremap H ^`: the count and
  register are `Pending` state already; the remap matches the *next* key and
  replaces it, so `^` runs with count 3, as in nvim. A digit that starts a
  count is never a remap left side unless `Pending` is empty and the user
  wrote one (`nnoremap 0 ^`; fine, nvim allows it too). A remap left side is
  matched only at the start of a command or where `Pending` waits for a motion
  or operator (so `dH` works); not for the argument of `f`, `t`, `r`, `m`,
  `'`, `` ` ``, `q`, `@`, `"` (nvim reads those arguments literally).
- **Dot-repeat.** `.` repeats the last repeatable change the *expanded* keys
  produced, since fe-vim stores committed changes, not key sequences.
  `nnoremap X dw` then `X` then `.` repeats `dw`. A yank (`nnoremap Y y$`)
  or a save (`nnoremap <leader>w :w<CR>`) makes no repeatable change, so `.`
  keeps repeating the one before it, as in nvim.
- **Macros.** nvim records typed keys, before mapping, and plays them through
  maps. So `macros.record` is called with the key as typed, and a replay's
  keys enter the remap stage like typed ones; `Macros` already distinguishes
  typed from replayed keys. Remap output is not recorded again.
- **`:normal` and `:normal!`.** `ex/normal.rs` feeds keys through `feed`.
  `:normal` should run them through the remap stage and `:normal!` bypass it;
  the card keeps the bang the parser reads and drops, and passes it as a
  `use_remaps: bool` into `feed`'s remap stage. A remap's own expansion inside
  `:normal` is run to the end of its line like the rest of its keys.
- **The expansion runner (first cut, any left-side mode).** `feed` returns
  `Step`s for one key, but an expansion is many keys across modes (`:w<CR>`
  opens the command line, which the host owns). So the host runs an
  expansion: for each key, feed or route it by the current mode, **dispatch
  each resulting invocation before the next key**, then route the next key
  by the mode that left (the command line's input after `:`). This is the
  GPUI macro runner (`replay_key`, which today drops the host command a
  replayed line leaves) extended to run it, and the
  `:normal` runner's per-mode routing, one runner rather than a third. It
  keeps typed-vs-expanded recording separate from which modes' left sides
  are remappable: a remap's keys are never recorded, whatever the mode.
- **Insert and cmdline.** There `feed` isn't called; the host takes the keys
  and calls `record_key`. A second entry point, `VimState::remap_insert(key)`
  → `Vec<VimKey>` or "wait", lets the host's insert path ask first. Only
  worth it if `i`/`c` remaps are in the first cut (see Questions).

### 6. Right sides that aren't keys

- **Keys** (`^`, `y$`, `<Esc>`, `<C-w>`): fed as keys. The notation is
  `fe-vim`'s `key.rs`, extended with `<Space>`, `<leader>`, `<lt>`, `<Bar>`.
  An unknown `<name>` is a load diagnostic.
- **Ex lines**, `:w<CR>`: just keys: `:` opens the command line, the rest is
  typed into it, `<CR>` runs it, through the expansion runner above. No
  special case. A save isn't dot-repeatable, as in nvim.
- **`<Cmd>…<CR>`**: runs the line without changing mode, without moving
  through the command line (no history, no flicker). `<Cmd>` takes an ex line
  as today, and one more form: **`<Cmd>fe:workbench.save<CR>`**, a fastedit
  command ID dispatched like a keymap binding (`Source::Keyboard`). Prefix
  `fe:` keeps it unambiguous with ex commands and is only in remaps, so it
  can't leak into a vimrc that is shared with nvim, where nvim would reject
  it (acceptable: that line would fail in nvim anyway).
- **`<Plug>`, `<expr>`, `<script>`, `<buffer>`, `<silent>`**: `<silent>` and
  `<nowait>` are accepted and ignored; the rest are diagnostics.

### 7. Hot reload and diagnostics

Same as M5: the remap set is part of the loaded config; a file that fails to
parse keeps the last good set, and the problems go to the status bar list that
opens each at its line (T56). Per-line diagnostics for (b): unknown command,
unknown key name, left side bound in the keymap, a cycle that can only loop
(`nmap a b` with `nmap b a`). Reload replaces the table atomically; a
half-typed multi-key remap in flight is dropped (its keys fall through).

### 8. Testing

Corpus cases against nvim, as every M7 card. A new optional field on a case:

```toml
[[case]]
name = "remap_Y"
mapping = ["nnoremap Y y$"]     # new, optional: runs in nvim and fe-vim first
buffer = "abc def"
cursor = [0, 4]
keys = "Yp"
```

The oracle runner feeds the `mapping` lines to headless nvim before the keys
and to fe-vim as a `RemapSet` built from the same lines (one parser under test
on both sides: if the vimrc parser mis-parses, the nvim answer differs). Cases
to record: `Y`, `H`, counts (`3H`), operator-pending (`dH` with `onoremap H ^`, in the preamble), `.` after a
mapped key, a macro recorded and played with a mapping, recursive vs
`noremap`, a self-prefix right side (`nmap a a`; `nmap ab abx` with `imap b B`, where the
expanded `a` enters Insert mode and `b` must still be mapped) a failed prefix with an independently mapped suffix, by mismatch (`jh` above)
and, as a unit test, by timeout (`j` then a pause then `h`), and a genuine
cycle (`a`→`b`→`a`), a prefix remap with its timeout (keys the case types with no delay
resolve as "no timeout"), `:normal` vs `:normal!` once that exists, and
`<leader>`. Timeouts aren't testable against nvim headless; they are unit
tests in fe-vim.

## Recommendation

**(c), staged: ship (a) first, then (b) compiled to it, normal/visual/operator
modes first, `noremap` semantics only at first, run in fe-vim in front of
`step`.**

Reasons: the engine is the same either way and is the real work; the TOML
table (in `settings.toml`'s `[vim]` table, where fe-config's `VimSettings`
already holds vim's settings; the keymap file is kiln's) is the smallest
surface that matches M5's config and diagnostics; the
vimrc parser then adds what nvim users actually want, pasting their lines,
for the price of a small parser that emits the same records. `<leader>`
defaults to `\` as in nvim, settable with `let mapleader` in (b) and
`vim.leader` in `settings.toml`. Recursive `map` is accepted by the parser and supported
by the engine (depth 100), since pasted vimrcs contain `nmap`; a stricter first
cut isn't cheaper once the engine exists.

## Questions for the user

1. **(a) TOML table only, (b) vimrc subset only, or (c) both?** Recommended: (c),
   in that order.
   Decided by the user, 2026-10-17: Both: the TOML table first, then the vimrc subset.
2. **Recursive maps** (`nmap`, `map`): needed, or `noremap` family only and
   `nmap` is a diagnostic? Recommended: supported, depth limit 100.
   Decided by the user, 2026-10-17: Recursive maps (`nmap`, `map`) are supported, depth limit 100.
3. **`<leader>` default**: `\` (nvim) or `<Space>` (a common choice)?
   Recommended: `\`, so a pasted vimrc behaves.
   Decided by the user, 2026-10-17: `<leader>` defaults to backslash `\`.
4. **Modes in the first cut**: normal, visual and operator-pending only
   (recommended), or insert and cmdline too? `inoremap jk <Esc>` already works
   as a keymap chord today.
   Decided by the user, 2026-10-17: The first cut covers normal, visual and operator-pending modes only. Insert and cmdline come later.
5. **Fastedit command IDs on a right side** (`<Cmd>fe:workbench.save<CR>`):
   wanted, or ex lines only?
   Decided by the user, 2026-10-17: Yes: `<Cmd>fe:<command id><CR>` right sides run fastedit commands, in a card after the core engine.
6. **`'timeoutlen'`**: reuse `kiln_platform::keys::CHORD_TIMEOUT` (recommended), or a separate
   `vim.timeoutlen` setting from the start?
   Decided by the user, 2026-10-17: `kiln_platform::keys::CHORD_TIMEOUT` serves as 'timeoutlen'.

## Implementation cards (outline)

All new code is in new files; the only edits to hub files are one-line hooks.
The cards were filed on 2026-10-17 under the label `vim-remaps`; card 8 (insert
and cmdline) waits, as the user scoped the first cut to n/x/o.

1. **Remap engine** (`crates/fe-vim/src/remap.rs`, new; M). `Remap`,
   `RemapSet`, `RemapState` (prefix matching, timeout input, recursion limit,
   modes from `Mode` and `Pending`), key-notation additions in `key.rs` (S,
   `<Space>`/`<leader>`). Hub touch: one field on `VimState` and one call at
   the top of `VimState::feed` in `state.rs`; `feature.rs` (so no `FEATURES` entry, hook or `Motion`/`TextObject`/
   `Operator` variant), `args.rs`, `reducer.rs` and `lib.rs` (beyond a
   `mod remap;`) are untouched. Unit tests for matching,
   ambiguity, timeout, depth.
2. **Config table and reload** (`crates/fe-config`, beside `VimSettings`; the
   `vim.remap` loader, new file; S). Parses `[[vim.remap]]`, hands a
   `RemapSet` to fe-vim, diagnostics into T56's list, keeps last good. Host
   side: `fe-gpui/src/vim.rs` calls the timeout and feeds the produced keys
   (small, S).
3. **Expansion runner** (`crates/fe-gpui/src/vim.rs` or a new
   `vim_expand.rs`; M). Dispatches each invocation of an expansion before
   the next key and routes keys by the resulting mode (`:w<CR>`), extending
   the macro runner past `cmdline`; independent of whether `i`/`c` left sides
   are supported (card 7).
4. **Counts, dot, macros, `:normal`** (`remap.rs`, `ex/normal.rs` plus
   tests; S). The typed-vs-expanded rules in §5; keep the bang the
   `:normal` parser discards and pass it as `use_remaps`.
5. **Corpus and oracle** (`crates/fe-vim/tests/support/` + `tests/corpus/
   remaps.toml`; S). The `mapping` field, recording against nvim.
6. **Vimrc subset** (`crates/fe-vim/src/remap/vimrc.rs`, new; M). Parser to
   `Remap` records, `let mapleader`, `unmap`, diagnostics with line numbers;
   config watch for the file.
7. **`<Cmd>` right sides and `fe:` command IDs** (`remap/cmd.rs`; S), after
   the user's answer to question 5.
8. **Insert and cmdline modes** (`remap.rs` entry point plus host call sites;
   M), only if the user wants them in.
