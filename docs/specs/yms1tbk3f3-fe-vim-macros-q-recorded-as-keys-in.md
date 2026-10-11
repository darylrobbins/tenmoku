---
task_uid: "01m4ewvc30jax826yms1tbk3f3"
task: "T225"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::macros::record_and_play: qa commits vim.record and stamps vim.recording a on the editor frame; qaxjq leaves register a holding \"xj\"; qa then A;\u003cEsc\u003ejq leaves \"A;\\x1bj\"; 3@a plays it three times; @@ replays the last played register; qA appends; \"ap puts the macro's text; after \"ayiw on the word dd, @a deletes a row (the register's text is played as keys); a macro whose motion fails stops at that key; a recursive macro (register a holding xj@a, set by a yank) ends when j fails on the last row, and on a 1,000-row buffer it finishes; a replay longer than 1,000,000 keys stops with a message; take_replay() returns the keys to feed and HeadlessVim feeds them"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::macros::bounded_bookkeeping: recording 100,000 keys keeps one register entry (no per-key allocation beyond the register's text); a count of 4294967295 before @a returns within the test, stopped by the replay cap"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test macros_match_neovim (tests/macros_oracle.rs): tests/corpus/macros.toml has at least 25 cases covering recording and playing with counts, @@, qA, insert-mode text with \u003cEsc\u003e and \u003cCR\u003e, a search and an ex command inside a macro, a macro stopping at a failing motion, \"ap of a recorded macro, a yank into a register then @a, . after @a and a recursive macro ending by failure; each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers (the macro's register text included); every earlier expected file is byte-identical and macros.expected.json is the recorder's unedited output"
    required: true
    manual: false
  - id: "C4"
    text: "In src/tests/reducer.rs only the q of prefixes_targets_cancel_and_forward's forwarded keys changes, and in src/tests/commands.rs only the q of move_dispatches_through_the_registry's feed(\"q\") Forwarded assert, each to a key fe-vim still forwards; every earlier corpus and unit test passes with its files unchanged; fe-gpui's vim::tests pass unchanged (Q and K stay forwarded); scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs describe macros and record_key/take_replay for hosts"
    required: true
    manual: false
---

# fe-vim: macros (q, @, @@) recorded as keys in registers, replayed by the host through take_replay(), with a replay cap

# fe-vim: macros (q, @, @@) recorded as keys in registers and replayed by the host

Milestone M7 "Vim", second batch (resized), card 08b: the macros half of the old card 8. Read 04b
(the command-line mode), 05a and 08a. Design: `docs/decisions/vim-engine.md` §4 (Macros: "keys
recorded … stored as register text … `@r` decodes the text and re-feeds it") and §9 item 6 ("`q{r}`
must also store the *keys* as text in register `r`, so that `"rp`, editing and `@r` work. Replay
feeds those keys through the same keymap and reducer path, deterministically").

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help q`, `:help @`, `:help @@`.

## Change (new `src/macros.rs`, `src/key.rs`, `src/register.rs`, `src/reducer.rs`, `src/state.rs`, `src/repeat.rs`, `src/headless.rs`)

- `q{a-z}` commits `vim.record {"register"}`; `q{A-Z}` appends to the lowercase register. While
  recording, `vim.recording` is stamped on the editor frame with the register's name. `q` while
  recording commits `vim.record_stop`.
- **What is recorded.** Every key the host feeds while recording, in every mode: normal, visual,
  operator-pending, insert (the typed chars, `<CR>`, `<BS>`, `<Esc>`) and the command line. The
  host calls `VimState::record_key(VimKey)` for keys that don't pass through `feed` (insert-mode
  text, command-line text); `HeadlessVim` does so for its insert-mode and command-line keys. The
  final `q` isn't recorded.
- **The register's text** is the keys in nvim's own encoding where nvim stores a byte (`<Esc>` as
  `\x1b`, `<CR>` as `\r`, `<Tab>` as `\t`, `<C-x>` as the control byte), and in vim notation
  (`<BS>`, `<Left>`) where nvim stores an internal byte sequence that isn't UTF-8. Charwise.
  `"ap` puts it; editing the register (a yank into `a`) changes what `@a` plays.
- `@{a-z}` (with a count) commits `vim.play {"register", "count"}`; `@@` replays the last played
  register. Playing doesn't feed keys itself: it leaves them in `VimState::take_replay() ->
  Vec<VimKey>` for the host to feed through its own path (`HeadlessVim` feeds them as typed keys;
  card 11 feeds them in the editor). A key that fails (a failed motion, a cancel, a forward) stops
  the rest of the replay, as nvim's error does. A replay that would feed more than 1,000,000 keys in
  all (recursive `@a` inside `a` included) stops there with a message.
- `.` after a macro repeats the macro's last change, as nvim does.

### Oracle

Macro registers are compared through the existing register comparison (text and kind); no new
recorded field.

### Corpus

`tests/corpus/macros.toml`, at least 25 cases: record and play with counts, `@@`, `qA` append, a
macro with insert-mode text and `<Esc>`, with `<CR>` in insert mode, with a search, with an ex
command, a macro that stops at a failing motion, `"ap` of a recorded macro, a yank into a register
then `@a`, `.` after `@a`, and a recursive macro that ends by failing.

## Tests (new files)

- `src/tests/macros.rs`, declared in `src/tests/mod.rs`.
- `tests/macros_oracle.rs` (`#[allow(dead_code)] mod support;`) with `macros_match_neovim`.

## Existing tests

`tests::reducer::prefixes_targets_cancel_and_forward` lists `q` among keys forwarded with nothing
pending; replace it with another key fe-vim still forwards. fe-gpui's
`vim::tests::normal_mode_keys_go_to_fe_vim` types `"QK"`: this card doesn't implement `Q` or `K`.

## Size

About 400 non-test lines (recording and encoding ~200, playing and the cap ~150, state ~50).

## Out of scope

`Q`, `:normal`, `@:`, recording with register `"`, the GPUI recording and replay (11). Any change
outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **Macros are keys, not invocations.** §9.6's "invocation lists for scripting and AI replay" is
  left to a later card; vim compatibility (`"ap`, editing a macro) needs the keys.
- **A replay cap of 1,000,000 keys** stands in for nvim's "until an error or `<C-c>`".
