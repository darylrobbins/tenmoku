---
task_uid: "01m55mhye0xswvd6frhavdxqeq"
task: "T155"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test insert_indent_keys_match_neovim (top level, in crates/fe-vim/tests/insert_indent_keys_oracle.rs) passes on crates/fe-vim/tests/corpus/insert_indent_keys.toml, at least 12 cases recorded with nvim 0.12.x, among them (asserted by name) ctrl_t_indents, ctrl_t_twice, ctrl_d_outdents, ctrl_d_to_previous_stop, ctrl_d_no_indent, zero_ctrl_d, caret_ctrl_d, ctrl_t_dot_repeat, ctrl_t_count_insert, ctrl_d_replace_mode."
    required: true
    manual: false
---

# fe-vim: insert-mode <C-t> and <C-d> (and 0<C-d>, ^<C-d>) indent and outdent the row by a shift width, as nvim does (Mac T881)

## Problem

At Mac main 6f032b12 insert mode's `<C-t>` and `<C-d>` (add or remove one 'shiftwidth' of the row's indent,
`:help i_CTRL-T`) do nothing in fe-vim. The user approved them in the "Vim parity" track on 2026-10-17.
nvim 0.12.6 probe (sw=4, et): `foo` at (0,0), `ia<C-t>b<Esc>`: `    abfoo`, cursor (0,5).

## Change

In `crates/fe-vim/src/insert/keys.rs`: `<C-t>` indents the row by one shift width and `<C-d>` takes one off
(to the previous multiple, as nvim's 'shiftround'-free `<C-d>` does: probe), at every caret, the caret
staying on the same text, using the editor's indent style and tab size as `>>` does
(`EditorModel::indent_style`, `tab_size`). `0<C-d>` takes all of the row's indent and `^<C-d>` takes it for
this row only (the `0` or `^` just typed is removed), as `:help i_0_CTRL-D` says. Both go in the session and
are replayed by `.` and a count as nvim's are (probe). Replace mode too (probe what nvim's do there).

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/insert_indent_keys.toml`, recorded
into `insert_indent_keys.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a
new `crates/fe-vim/tests/insert_indent_keys_oracle.rs` modelled on `tests/line_motions_oracle.rs`.

## Implementation notes

- There is no `src/insert/keys.rs` at the branch point; it is new, a submodule of `insert.rs`.
- `HeadlessVim` takes `<C-t>` and `<C-d>` in insert and replace mode through `VimState::indent_in` and `indent_out`; a host calls the same two. `<C-d>` after a typed `0` or `^` (insert mode) is `0<C-d>` / `^<C-d>`.
- Each key goes in the session's typed text as a private-use mark (U+E000..U+E003), so `LastChange::inserted` holds it and `.` and a count type it again. Replace-mode `.` and counts don't repeat the keys (the corpus doesn't cover it).
- The gpui host's key routing is not changed here.
- Out of scope here (the task's scope is fe-vim only): wiring the keys through the gpui host's key routing and macro recording, and repeating them with `.` or a count in replace mode.
- A caret inside the indent moves by what the shift changed, as nvim's does (`ctrl_d_caret_inside_indent`). `0`/`^` count only if typed in this session. A literal U+E000..U+E004 in typed text is escaped in `LastChange::inserted`.
- `0`/`^` count as typed only if the session's last batch put them just before the caret, so any key typed since (an indent key among them) disarms them, per caret.
- Replace mode records each indent key at its place in the keys typed (`Replace::keys`), so a count and `.` type them again (`repeat_replace`, `retype`).
- Reaching the desktop input needs files outside the task's scope: fe-gpui's `src/vim.rs` (`TEXT_INPUT`, `record_typing`, the insert/replace key routing) and `src/commands.rs`, its keymap bindings for `ctrl-t`/`ctrl-d` in insert and replace mode, and fe-vim's `src/ex/normal.rs` (`:normal`'s insert keys). The handlers they would call are `VimState::indent_in` and `indent_out`.
- Desktop: `ctrl-t` and `ctrl-d` in insert and replace mode are `vim.key` in `fastedit.toml` (off macOS, like vim's other ctrl keys); the editor's key listener feeds them to `VimState::indent_in`/`indent_out` (`EditorView::indent_typing`), macro recording keeps them as `<C-t>`/`<C-d>`, replay and `:normal` (`ex/normal.rs`) take them. `restore_indent` runs after every typed line break, in replace mode too.
- Known gap (F8, a follow-up card): the position of each indent key in the typed text is a byte offset taken when it was typed, so a later `<BS>` over text typed before it makes `.` and counts replay the keys in the wrong place. nvim repro: from `        foo` at the first `f` (8 spaces), `ia<C-t><BS> <Esc>0.` gives 18 leading spaces in nvim; fe-vim gives 16. Fixing it needs an ordered input history of the session's keys instead of offsets.
