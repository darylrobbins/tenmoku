---
task_uid: "01m55mhy73pa1z1vqnqvzy49gs"
task: "T153"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test insert_keys_match_neovim (top level, in crates/fe-vim/tests/insert_keys_oracle.rs) passes on crates/fe-vim/tests/corpus/insert_keys.toml, at least 16 cases recorded with nvim 0.12.x, among them (asserted by name) ctrl_w_word, ctrl_w_punctuation, ctrl_w_stops_once_at_insert_start, ctrl_w_joins_rows, ctrl_w_in_indent, ctrl_u_typed_text, ctrl_u_nothing_typed, ctrl_u_twice, ctrl_w_dot_repeat, ctrl_u_count_insert, ctrl_w_replace_mode, ctrl_w_in_macro."
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::insert_tests::insert_ctrl_keys_tests::ctrl_w_and_ctrl_u_reach_vim_in_insert_mode (new crates/fe-gpui/src/vim/insert_ctrl_keys_tests.rs, declared in crates/fe-gpui/src/vim/insert_tests.rs) passes: in a real EditorView with vim on, ctrl-w and ctrl-u in insert and replace mode edit as the corpus's cases do, a macro recorded with them replays them, and ctrl-w h in normal mode still focuses the pane to the left."
    required: true
    manual: false
  - id: "C3"
    text: "The integration test insert_ctrl_keys_are_vims (top level, in crates/fe-keymap/tests/vim_insert_keys.rs) passes: off macOS ctrl-w, ctrl-u, ctrl-r, ctrl-t, ctrl-d, ctrl-o, ctrl-n, ctrl-p and ctrl-a resolve to vim.key in an editor in vim insert and replace mode, and to what they resolved to before in vim normal mode and with vim off; on macOS none of them is bound in those modes."
    required: true
    manual: false
---

# fe-vim: insert-mode <C-w> and <C-u> delete the word and the typed text before the caret as nvim does, with ctrl keys in insert and replace mode reaching vim in both hosts (Mac T879)

## Problem

At Mac main 6f032b12 vim's insert and replace modes take only typed characters, `<CR>`/`<C-j>`, `<BS>`,
`<Esc>` and the arrows. Every other key is dropped: `HeadlessVim::type_key` and `replace_key`
(`crates/fe-vim/src/headless.rs`) return `FeedError::Forwarded`, fe-gpui's `EditorView::feed_vim`
(`crates/fe-gpui/src/vim.rs`) returns false whenever `accepts_text()`, and `replay_key` there returns false
for them, so a macro holding one stops. Off macOS several of nvim's insert keys run workbench commands
instead (`ctrl-w` closes the tab, `ctrl-o` opens a file, `ctrl-n` a new file, `ctrl-p` quick open, `ctrl-a`
select all). The user approved the "Vim parity" track on 2026-10-17 (close the gaps against VSCodeVim and
IdeaVim, nvim 0.12 as the reference). Probed with `nvim --clean --headless` 0.12.6 (`backspace` is
`indent,eol,start`):

- `foo bar` at (0,3), `a xy<C-u><Esc>`: `foo bar` (`<C-u>` takes only what this insert typed on the row).
- `foo bar` at (0,3), `a<C-u><Esc>`: `bar`, cursor (0,0) (nothing typed: everything before the caret).
- `  foo` / `bar` at (1,0), `i<C-w><Esc>`: `  foobar`, cursor (0,4) (at a row's start it joins the rows).
- `foo bar` at (0,7), `A baz<C-w><C-w><Esc>`: `foo bar`: the first `<C-w>` takes `baz`, the second stops
  once at the insert's start (`:help i_CTRL-W`, `'backspace'` "stop once").

## Change

This card is the insert-key seam the other insert-key cards build on, plus `<C-w>` and `<C-u>`.

- New `crates/fe-vim/src/insert/keys.rs`, declared in `crates/fe-vim/src/insert.rs` (`mod keys;`, as
  `block_rows` is): `VimState::insert_key(&self, model, host, key) -> bool`, called in insert and replace
  mode for a key the host doesn't type itself; it edits as the open session's origin, inside its
  transaction, at every caret, and returns whether vim took the key. Keys this card doesn't implement return
  false (dropped, as today).
- `<C-w>` deletes the word before the caret and `<C-u>` the text typed on the row (or, with none, all before
  the caret), each stopping once at where the insert began and joining rows at a row's start, as
  `:help i_CTRL-W` / `i_CTRL-U` and the probes say. In replace mode they move back as nvim's do (probe).
- `.` and an insert's count replay them as nvim does (it replays the keys: probe `A<C-w>x<Esc>j.` and
  `3ia b<C-w>c<Esc>`); the session's record keeps them in order, as replace mode's record keeps `<BS>`
  (`crates/fe-vim/src/repeat.rs`).
- Hosts: `HeadlessVim::type_key`/`replace_key` pass any other key to `insert_key` before forwarding.
  `crates/fe-gpui/src/vim.rs`: in insert and replace mode a ctrl keystroke the keymap leaves (and `vim.key`)
  goes to `insert_key`, recorded with `record_key`, and `replay_key` replays it.
- `crates/fe-keymap/keymaps/fastedit.toml`: off macOS, `ctrl-w ctrl-u ctrl-r ctrl-t ctrl-d ctrl-o ctrl-n
  ctrl-p ctrl-a` run `vim.key` in `editor && (vim.mode == insert || vim.mode == replace)`, later than the
  workbench's bindings so they win there (the later insert-key cards implement them; until then they do
  nothing in those modes, as in nvim they're never the workbench's). Keep T523's ctrl-r rule intact.
  Update `crates/fe-gpui/src/command_tests.rs`'s binding count.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/insert_keys.toml` (header comment as
the other corpora; a key that fails is a case's last key), recorded into `insert_keys.expected.json` by the
ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new `crates/fe-vim/tests/insert_keys_oracle.rs`
modelled on `tests/line_motions_oracle.rs`. The fe-gpui test goes in a new
`crates/fe-gpui/src/vim/insert_ctrl_keys_tests.rs`, declared in the existing
`crates/fe-gpui/src/vim/insert_tests.rs` by `#[path = "insert_ctrl_keys_tests.rs"] mod insert_ctrl_keys_tests;`.
The keymap test is a new `crates/fe-keymap/tests/vim_insert_keys.rs` modelled on `tests/vim_window_keys.rs`.

insert.rs and repeat.rs are changed by the lent T800, T803 and T836 (T836 last), headless.rs by T802, and
fastedit.toml by T523: this card depends on T836, T802 and T523.

## Implementation notes

- The insert-key cards that build on this seam (T155 `<C-t>`/`<C-d>`, T156 `<C-o>`, T157 `<C-n>`/`<C-p>`)
  landed first, each with its own branch in the hosts. `VimState::insert_key` takes `<C-w>` and `<C-u>`,
  and both hosts pass it any other key in insert and replace mode after their own branches; it returns
  false for the rest, which stay dropped. `VimState::takes_insert_key` lets fe-gpui refuse the edit in a
  read-only view without swallowing keys vim doesn't take.
- `.` and a count replay the keys: insert mode notes each in the session's ledger as a mark, as the indent
  keys are noted (`keys::Key::Delete`, private-use `\u{e006}`/`\u{e007}`), now with what the primary caret
  had typed both before and after the key, so the history's diff starts from what the key left. A count's
  copies stop once at the region's start, as nvim's copies (typed in the same insert) do: `3ia b<C-u>c`
  gives `fcoo`.
- Replace mode goes back caret by caret, each with `replace_backspace` until it reaches where its key's
  deletion starts (by progress, so a `<BS>` that takes a line break's whole indentation doesn't overshoot);
  where `<BS>` takes nothing (before where typing began) the caret only steps back. At a row's start the
  caret goes to the end of the row above and the line break stays, even one typed in the session, as
  nvim's does (unlike `<BS>`, which joins). A key that moved a caret is noted as a mark, so `.` and a
  count's copies go back again where they type it; the copies put back what they overwrote from their own
  stacks, as their `<BS>`s do. Within typed text replace's own key record, which notes each `<BS>`, is
  enough.
- A count's copies follow carets that a deletion or going back brings together, in insert mode
  (`follow_ranks`) and in replace mode with what each overwrote (`follow_stacks`). Replace-mode copies of a
  session that only moved (no keys typed) still go back at every caret.
- The keymap's six earlier ctrl-t/ctrl-d/ctrl-o insert/replace bindings became one block of nine keys in
  `editor && (vim.mode == insert || vim.mode == replace)`. command_tests.rs's count is of the macOS keymap,
  where none of them is bound, so it stays 280. `vim_window_keys.rs` and `day_one_bindings.rs` asserted
  ctrl-w and ctrl-r did nothing vim's in insert mode off macOS, and fe-workbench's `default_keymaps.rs` that
  ctrl-a selected all there; they now expect `vim.key`.
