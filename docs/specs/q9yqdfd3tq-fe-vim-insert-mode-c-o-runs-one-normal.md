---
task_uid: "01m55mhyqpvwxx9cq9yqdfd3tq"
task: "T156"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test insert_one_command_matches_neovim (top level, in crates/fe-vim/tests/insert_one_command_oracle.rs) passes on crates/fe-vim/tests/corpus/insert_one_command.toml, at least 14 cases recorded with nvim 0.12.x, among them (asserted by name) ctrl_o_motion, ctrl_o_at_row_end, ctrl_o_delete, ctrl_o_count, ctrl_o_register_put, ctrl_o_visual_returns, ctrl_o_command_line_returns, ctrl_o_escape_returns, ctrl_o_open_row, ctrl_o_dot_after, ctrl_o_undo."
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::insert_tests::insert_ctrl_keys_tests::ctrl_o_runs_one_command_with_the_block_cursor passes: in a real EditorView, after ctrl-o the caret paints as a block and the key context has vim.mode normal and vim.insert_pending, and after the command the caret is a bar again in insert mode."
    required: true
    manual: false
---

# fe-vim: insert-mode <C-o> runs one normal-mode command and returns to insert, as nvim does (Mac T882)

## Problem

At Mac main 6f032b12 insert mode's `<C-o>` (run one normal-mode command, then go back to insert,
`:help i_CTRL-O`) does nothing in fe-vim. The user approved it in the "Vim parity" track on 2026-10-17.
nvim 0.12.6 probe: `abc def` at (0,0), `A<C-o>0X<Esc>`: `Xabc def`, cursor (0,0); nvim's mode is `niI`
in between.

## Change

- `crates/fe-vim/src/insert/keys.rs`: `<C-o>` in insert mode splits the session as the cursor keys do
  (`VimState::split_session`), leaves the caret where nvim does (it doesn't step back; at the row's end it
  stays past the last character: probe), and enters normal mode with a one-command flag on `VimState`
  (`crates/fe-vim/src/state.rs`).
- The next complete normal-mode command runs, then the session resumes in insert mode at the caret. A
  command that opens a visual mode or the command line returns to insert once that ends (probe `<C-o>vjd`
  and `<C-o>:s/a/b/<CR>`); `<Esc>` with nothing pending returns at once; a command that itself enters insert
  mode (`<C-o>o`) just continues there. Count and register prefixes belong to the one command.
- `vim.mode` is `normal` meanwhile (so normal-mode bindings apply), plus a new context flag
  `vim.insert_pending`; fe-gpui's caret is the block cursor then (`crates/fe-gpui/src/vim.rs`).
- `.` after the session repeats what nvim's does (probe `ia<C-o>xb<Esc>` then `.`); `u` undoes the parts as
  nvim's does (probe).
- `HeadlessVim` (`crates/fe-vim/src/headless.rs`) routes keys by the mode as it does now, so it needs only
  the return to insert after the command (`crates/fe-vim/src/commands.rs`, where commands finish).

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/insert_one_command.toml`, recorded
into `insert_one_command.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a
new `crates/fe-vim/tests/insert_one_command_oracle.rs` modelled on `tests/line_motions_oracle.rs` (nvim's
`niI` mode is recorded as `normal`: map it in the new test if `support::check` doesn't). The fe-gpui test
goes in the `insert_ctrl_keys_tests.rs` the seam card added.

## Implementation notes

- `VimState::one_command` (`crates/fe-vim/src/insert/keys.rs`) ends the session as a split ends it
  (a block session as `Esc` ends it, typing on its other rows, as nvim's does) and keeps what resumes in
  `VimState::one_command`'s `OneCommand`. A caret past its row's end steps onto the last grapheme, keeping
  its column as the goal (so `A<C-o>j` keeps nvim's `curswant`); on return it goes back past it if it's on
  that row's last grapheme, or if the goal is past it (`$`), as nvim's `edit()` does on restart.
- The return is `fe_vim::dispatch` (`commands.rs`), which both hosts run vim's commands through: after
  the command, if nothing is pending and the mode is normal, the session resumes. A key that ends the
  command without committing one (`<Esc>` with nothing pending, a cancelled operator, a failed key) makes
  `VimState::feed` commit `vim.resume_insert`. A command that enters insert or replace mode goes on there.
- `<C-o>@a` plays the register, then enters insert mode again with `i` (`R`/`gR` from replace mode)
  queued after the register's keys, as nvim's `put_reedit_in_typebuf` does. A map's keys after its first
  command go in as typed text, as nvim's do.
- `.` inside the command repeats the change before the session (nvim's `old_redobuff`); once the session
  resumes, `.` has the text typed before `<C-o>` again unless the command made a change.
- A charwise put under `<C-o>` leaves the caret just past the text, as nvim's `do_put` does for
  `restart_edit`.
- The corpus has 54 cases. `.` with nothing to repeat isn't recordable: the recorder doesn't reset nvim's
  redo buffer between cases.
- Carried findings fixed: T98 F2 (`@:` settles the marks around each run of its count); T155 F8 (the
  indent keys' history keeps the `<BS>`s that took back text typed before a key, so `.` and a count type
  the keys in order), F9 (`^<C-d>`'s restored indent is the line break's: `Esc` drops it unused, and in
  replace mode it joins the line break's entry, so `<BS>` takes it out and typing after it continues the
  same record; the count's copies restore it after each line break too) and F10 (the count's copies
  charge what they put in and all the indentation their keys write against `MAX_PUT`, and stop before
  the next copy would exceed it; plain ASCII runs in replace mode read only what's after the caret, so
  `100000R<C-t>x<Esc>` stops in seconds).
- In replace mode `<BS>` over a line break's indentation of spaces and tabs now takes one shift width
  at a time, as nvim's 'smarttab' does (it took all of it), for any line break: recorded in
  `replace_break_and_gi.toml`. Other white space still goes whole, as before.
