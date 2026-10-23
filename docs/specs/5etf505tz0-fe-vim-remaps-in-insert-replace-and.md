---
task_uid: "01m5n8xzb6s1v3he5etf505tz0"
task: "T205"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "RemapMode has Insert and CmdLine; VimState::remap_insert(key) returns RemapInsert::Wait or Keys(..) in Insert, Replace and CmdLine mode and RemapInsert is exported from fe_vim"
    required: true
    manual: false
  - id: "C2"
    text: "tests/remap_insert.rs (new, runs with cargo test -p fe-vim --test remap_insert) covers: inoremap jk \u003cEsc\u003e ends insert; an expansion types its text; imap chain vs inoremap; a held prefix then a breaking key types the first key and re-matches the rest; remap_timeout resolves a held prefix to the shorter map or the typed keys; replace mode; cnoremap on the command line; set_remaps while a prefix is held; E223 on an imap cycle"
    required: true
    manual: false
  - id: "C3"
    text: "Cross-mode self-prefix: with nmap ab abx and imap b B, HeadlessVim typing ab in normal mode runs a literally (enters Insert) and the b after it is mapped by imap b B (test asserts the buffer, and the expected text matches what headless nvim does for the same lines)"
    required: true
    manual: false
  - id: "C4"
    text: "A macro recorded while an imap fires holds the typed keys, not the expansion, and replays the same; :normal runs insert-mode maps and :normal! does not; remap_waiting() is true and vim.pending_kind is remap while an insert prefix is held"
    required: true
    manual: false
  - id: "C5"
    text: "Existing fe-vim remap tests (remap_feed, remap_set, remap_repeat, remap_cmd, remaps_oracle) and cargo clippy -p fe-vim pass unchanged; no change under crates/fe-gpui, fe-workbench, fe-config or tests/corpus"
    required: true
    manual: false
---

# fe-vim: remaps in insert, replace and cmdline mode (RemapMode::Insert/CmdLine, VimState::remap_insert), run by HeadlessVim and :normal (T877 A) (Mac T1089)

## Problem

T877 (record `docs/decisions/vim-remaps.md`, card 8) split into A-E; this is A, the
engine. Remaps run in normal, visual and operator-pending mode only. In insert,
replace and cmdline mode the host types the key and only reports it with
`VimState::record_key`, so `remap::stage` (called from `VimState::feed_key`) never
sees it. This card gives fe-vim the engine side and wires the two fe-vim hosts that
type those modes: `HeadlessVim` and `:normal`'s `Runner`. The vimrc and settings do
not load `i`/`c` maps yet (card B), so A's tests build `Remap` records directly, as
`tests/remap_feed.rs` does. No fe-gpui / fe-workbench / fe-config / vimrc change.

## Change

- `crates/fe-vim/src/remap.rs`: `RemapMode` gains `Insert` (`i`, used for Insert and
  Replace mode) and `CmdLine` (`c`). `map_mode` returns them for `Mode::Insert |
  Mode::Replace` and `Mode::CmdLine(_)` (it returns `None` today for these); the
  exhaustive `match mode` in `hides()` gets arms (insert/cmdline hide no built-in).
  New `pub enum RemapInsert { Wait, Keys(Vec<VimKey>) }` and
  `VimState::remap_insert(&mut self, key: VimKey) -> RemapInsert`, which a host asks
  before it types/edits for a key in insert, replace or cmdline mode. Build it on
  `stage`/`run`/`resolve`, so held prefixes (`remap_waiting`, `vim.pending_kind` =
  `remap`), `remap_timeout`, `take_replay` tags (map output never recorded, typed
  keys recorded once), the E223 depth limit, `remap_off` (`:normal!`) and
  `set_remaps` replace all behave as in normal mode. `Wait`: the host types nothing
  yet. `Keys`: what to type now, in order; a broken prefix gives its first key as
  typed and matches the rest again in the mode that key left (record section 4,
  rule 3). The host calls `record_key` itself for each key it types, as today, so
  `remap_insert` must not double-record.
- Cross-mode self-prefix rule (record section 3): with `nmap ab abx` and `imap b B`,
  `ab` typed in normal mode runs `a` literally (enters Insert) and the `b` after it
  is mapped by `imap b B`. Only the first key of a right side that starts with the
  whole left side is exempt.
- `crates/fe-vim/src/state.rs`, `src/lib.rs`: the method; re-export `RemapInsert`
  next to `RemapMode` (`lib.rs` line ~846).
- `crates/fe-vim/src/headless.rs`: `feed_mode_key`'s Insert, Replace and CmdLine arms
  and `feed_key`'s cmdline fast path ask `remap_insert` first and run each returned
  key through the existing `record_key` + `type_key`/`replace_key`/`line_key` (a
  `Wait` returns `Ok`). Keys a map expands come back through `take_replay` as in
  `HeadlessVim::feed`'s loop, so expansion keys carry their tags.
- `crates/fe-vim/src/ex/normal.rs`: `Runner::key`'s Insert/Replace/CmdLine branch
  does the same; `:normal!` (`remap_off`) bypasses it; `held_keys` at the end of a
  run resolves a held insert prefix as a timeout does.
- New tests in a NEW file `crates/fe-vim/tests/remap_insert.rs`, driving
  `HeadlessVim` with `Remap` records and `state_mut().remap_timeout()` as
  `remap_feed.rs` does: inoremap `jk`->`<Esc>`; expansion text; `imap` chain and
  `inoremap` non-chain; held prefix then a breaking key; timeout of a held prefix;
  replace mode; cmdline `cnoremap`; the cross-mode self-prefix case; a macro
  recorded with an imap holds the typed keys, not the expansion; `:normal` vs
  `:normal!`; `set_remaps` while a prefix is held.

## Notes

- fe-vim has no timer: the host calls `remap_timeout()` after `CHORD_TIMEOUT`.
- Before coding, check nvim for `inoremap jk <Esc>` + `ijkx` and
  `nmap ab abx` + `imap b B` + `ab`, so the unit tests state nvim's answer. The
  corpus cases for these land in card B (they need the vimrc parser).
- Do NOT touch `crates/fe-vim/tests/corpus/*`.
