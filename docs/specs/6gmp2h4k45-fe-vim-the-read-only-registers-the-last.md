---
task_uid: "01m55cejxrzktmy16gmp2h4k45"
task: "T169"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test readonly_registers_match_neovim (top level, in crates/fe-vim/tests/readonly_registers_oracle.rs) passes on crates/fe-vim/tests/corpus/readonly_registers.toml, at least 14 cases recorded with nvim 0.12.x, among them (asserted by name) dot_after_insert, dot_after_later_delete, dot_after_dot_repeat, dot_after_replace_mode, dot_with_count, dot_in_visual_put, colon_after_substitute, colon_after_failed_command, colon_not_set_by_cancelled_line, colon_not_set_by_search, dot_unset_fails, yank_into_dot_fails, delete_into_colon_fails."
    required: true
    manual: false
  - id: "C2"
    text: "Registers::get('.') and get(':') return the texts charwise and Registers::check still refuses both names, asserted through HeadlessVim in readonly_registers_oracle.rs; the docs of get, read and check say so."
    required: true
    manual: false
---

# fe-vim: the read-only registers . (the last inserted text) and : (the last command line) can be put, as nvim's can, and refuse writes (Mac T836)

## Problem

fe-vim has no `.` or `:` register at Mac main 92c57c57: the reducer's `is_register` takes `" - _ + * /`,
letters and digits, so `".p` and `":p` cancel, and `Registers::get` knows neither. The user approved them
on 2026-10-17 in the "Vim breadth" track. In nvim (`:help quote.`, `:help quote:`) `.` holds the last
inserted text and `:` the last command line run; both are read-only. Probed with `nvim --clean --headless`
0.12.6:

- `x` at (0,0), `ahello<Esc>".p`: `xhellohello`, cursor (0,10).
- `x` at (0,0), `ahi<Esc>0dd".P`: `hi`, cursor (0,1) (the text survives later changes).
- `abc` at (0,0), `:s/b/B/<CR>":p`: `as/b/B/Bc`, cursor (0,6).
- `".yy` does nothing (a write to a read-only register fails).

## Change

- `crates/fe-vim/src/register.rs`: `Registers` keeps the last inserted text and the last command line;
  `get('.')` and `get(':')` return them charwise (`None` before there is one), `read` takes both names, and
  `check` (the names a command may write) still refuses them, so a yank, delete or change naming one
  fails as nvim's does. The docs of `get`, `read` and `check` say so.
- The last inserted text is set where an insert or replace session ends and `LastChange::inserted` is
  set (`crates/fe-vim/src/insert.rs`, near `state.set_last_change`; `.` repeating an insert sets it
  again). The last command line is set where an ex line is noted for `@:` (`crates/fe-vim/src/macros.rs`
  and its caller in `crates/fe-vim/src/commands.rs`), typed or replayed, and not for a cancelled line or a
  search.
- `crates/fe-vim/src/reducer.rs` `is_register`: take `.` and `:` after `"`.

Every reader of `Registers::read` (`p`, `P`, a visual put, `:pu` once it lands) then gets them; check what
nvim does for `".` put with a count and for `v".p`, and match it. Test with the oracle pattern: a new corpus
`crates/fe-vim/tests/corpus/readonly_registers.toml` (header comment as the other corpora; a key that fails
is a case's last key), recorded into `readonly_registers.expected.json` by the ignored
`record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/readonly_registers_oracle.rs` modelled on `tests/line_motions_oracle.rs`. The oracle
doesn't record `.` and `:`: the cases put them into the buffer.

reducer.rs, commands.rs and insert.rs are held by Mac T770, T800, T803 and T805, and commands.rs also by Mac
T838 (`:s`'s confirm): this card depends on T805 and T838, the last of them.
