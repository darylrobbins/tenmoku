---
task_uid: "01m5e48zcqsfefj5vkftqze8jz"
task: "T241"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test insert_registers_match_neovim (top level, in crates/fe-vim/tests/insert_registers_oracle.rs) passes on crates/fe-vim/tests/corpus/insert_registers.toml, at least 16 cases recorded with nvim 0.12.x, among them (asserted by name) ctrl_r_named, ctrl_r_unnamed, ctrl_r_linewise, ctrl_r_multirow_autoindent, ctrl_r_ctrl_r_literal, ctrl_r_ctrl_o_linewise_above, ctrl_r_ctrl_p_fixes_indent, ctrl_r_dot, ctrl_r_colon, ctrl_r_slash, ctrl_r_empty_register, ctrl_r_escape_cancels, ctrl_a_last_insert, ctrl_a_nothing_inserted, ctrl_r_dot_repeat."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test insert_register_refusals_type_nothing (top level, same file) passes: through HeadlessVim, \u003cC-r\u003e= and \u003cC-r\u003e followed by a key that names no register leave the text, the caret and insert mode as they were."
    required: true
    manual: false
---

# fe-vim: insert-mode <C-r>{reg} (and <C-r><C-r>, <C-r><C-o>, <C-r><C-p>) inserts a register and <C-a> the last inserted text, as nvim does (Mac T880)

{
  "mode": "inline",
  "body": "## Problem\n\nAt Mac main 6f032b12 insert mode has no `<C-r>{reg}` (insert a register) or `<C-a>` (insert the last\ninserted text): fe-vim drops both (see the insert-key seam card this depends on). Both are in VSCodeVim and\nIdeaVim, and the user approved them in the \"Vim parity\" track on 2026-10-17. nvim 0.12.6 probe: `abc` /\n`def` at (0,0), `yiwjA <C-r>\"<Esc>`: `def abc`, cursor (1,6).\n\n## Change\n\nIn `crates/fe-vim/src/insert/keys.rs` (`VimState::insert_key`), as `:help i_CTRL-R` and `:help i_CTRL-A` say,\nprobing each rule with `nvim --clean`:\n\n- `<C-r>{reg}` types the register's text as if typed (autoindent applies to its line breaks, as\n  `VimState::line_break_in` does); `<C-r><C-r>{reg}` and `<C-r><C-o>{reg}` insert it literally,\n  `<C-r><C-o>` putting a linewise register above the row as `P` does; `<C-r><C-p>{reg}` literally with its\n  indent fixed. Every register fe-vim reads works (`\"` `0`–`9` `a`–`z` `-` `/` `.` `:` `+` `*`); an empty one\n  inserts nothing; `=` (expression) and any non-register key, and `<Esc>` after `<C-r>`, insert nothing and\n  leave insert mode's state as it was.\n- `<C-a>` inserts the `.` register's text as `<C-r>.` does; with nothing inserted yet it fails as nvim's\n  does (E29).\n- `.` repeats and macros replay them as nvim does (probe `A<C-r>a<Esc>j.` after changing `a`).\n\nThe `.` and `:` registers come from the lent T836. Test with the oracle pattern: a new corpus\n`crates/fe-vim/tests/corpus/insert_registers.toml`, recorded into `insert_registers.expected.json` by the\nignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new\n`crates/fe-vim/tests/insert_registers_oracle.rs` modelled on `tests/line_motions_oracle.rs`.",
  "ref": "",
  "ref_blob": ""
}

## Implementation notes

`VimState::insert_key` has no registers, and insert mode's plain-character path never reaches it, so the feature
adds `insert_register_start`, `insert_register_key` and `insert_last_inserted` (in `insert/keys.rs`) with the
pending prefix kept in `VimState::register_key` (`state.rs`). `HeadlessVim::type_key` calls them. Known limit:
fe-gpui doesn't call them yet, so `<C-r>` and `<C-a>` are still dropped in the app; `<C-r><C-p>` fixes the indent
from the primary caret's row only.

Review round 1: F3 (`<C-r><C-p>` keeps the rows' relative indents), F4 (a prefix belongs to its session) and F5
(E35 for an unset `/`) are fixed and recorded. F1 (fe-gpui wiring) is out of scope for this card and stays a known
limit; F2: `.` repeats the text a `<C-r><C-o>`/`<C-r><C-p>` put as a literal insert, which is all the corpus
shows nvim doing.
