---
task_uid: "01m55cehsr00q0d627tsehwnv2"
task: "T168"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test substitute_confirm_matches_neovim (top level, in crates/fe-vim/tests/substitute_confirm_oracle.rs) passes on crates/fe-vim/tests/corpus/substitute_confirm.toml, at least 16 cases recorded with nvim 0.12.x, among them (asserted by name) yes_no_yes, all_after_no, quit_after_yes, last_after_no, esc_then_undo, without_g_one_per_row, across_rows, replacement_longer_than_match, replacement_with_line_break, no_match_fails, amp_amp_keeps_confirm, other_keys_ignored, from_a_macro."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test confirm_prompt_is_nvims (top level, same file) passes: through HeadlessVim, while a confirm waits VimState::message is replace with {string} (y/n/a/q/l/^E/^Y)? and the selection is the match, and after the last answer the message is nvim's report."
    required: true
    manual: false
---

# fe-vim: :s with the c flag asks before each match, taking y, n, a, q, l and Esc as nvim's prompt does, all its replacements one undo step, instead of refusing c (Mac T838)

## Problem

At Mac main 92c57c57 fe-vim refuses `:s`'s `c` flag with "the c flag (confirm): not supported yet"
(`flags_and_count` in `crates/fe-vim/src/substitute.rs`). The user approved it on 2026-10-17 in the "Vim
breadth" track. In nvim (`:help :s_c`) each match is shown and the prompt `replace with {string}
(y/n/a/q/l/^E/^Y)?` waits for a key. Probed with `nvim --clean --headless` 0.12.6:

- `a a a`, `:s/a/b/gc<CR>yny`: `b a b`.
- `a a/a`, `:%s/a/b/gc<CR>na`: `a b/b` (`a` replaces this and every later match), cursor (1,0).
- `a a/a`, `:%s/a/b/gc<CR>yq`: `b a/a` (`q` stops), cursor (0,2).
- `a a/a`, `:%s/a/b/gc<CR>nl`: `a b/a` (`l` replaces this one and stops), cursor (0,0).
- `a a/a`, `:%s/a/b/gc<CR>yy<Esc>u`: `a a/a` (`<Esc>` stops; the replacements made are one undo step).

## Change

A confirm session for `:s` with `c` (and `:&`/`&&` keeping it):

- `crates/fe-vim/src/substitute.rs`: with `c`, the run stops before the first match instead of replacing
  it, and leaves a pending confirm in `VimState` (the remaining range, the compiled pattern and
  replacement, the flags, the transaction opened, the count of replacements); the match is selected (the
  caret on its start, as nvim shows it) and `VimState::message` holds nvim's prompt. Answering `y` replaces
  it and goes to the next match (after the replacement, wrapping to later rows of the range, a `g`-less
  `:s` moving to the next row after one), `n` skips it, `a` replaces it and all after it without asking,
  `l` replaces it and ends, `q` and `<Esc>` end. When it ends, the transaction closes (one `u` undoes every
  replacement), the caret goes where nvim's does, and the report message is nvim's.
- `crates/fe-vim/src/state.rs` `VimState::feed`: while a confirm is pending, `y n a q l <Esc>` commit a new
  `vim.substitute_answer {"answer": "y"}` (an args struct in substitute.rs), every other key does nothing
  (nvim waits; `<C-e>`/`<C-y>` scroll in nvim: leave them as nothing here), and the mode stays normal so
  the host needs no change. A macro or `:normal` feeding the answers works the same way.
- `crates/fe-vim/src/commands.rs`: register `vim.substitute_answer`.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/substitute_confirm.toml` (header
comment as the other corpora; an error ends a case), recorded into `substitute_confirm.expected.json` by the
ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/substitute_confirm_oracle.rs` modelled on `tests/ex_lines_oracle.rs`; plus the prompt
text asserted through `HeadlessVim` in the same file.

substitute.rs is also Mac T837's (the print flags, before this), and commands.rs Mac T770's, T800's and
T803's (helium's chain): this card depends on T837 and T803.

## Implementation notes

- **The prompt text is nvim 0.12's, not the one C2 quotes.** nvim 0.12.4 (the oracle's pin) prompts
  `replace with {string}? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)`. That is the only
  "replace with" string in its binary, and its `usr_10.txt` shows it the same way.
  `replace with {string} (y/n/a/q/l/^E/^Y)?` is the older Vim/nvim text. `confirm_prompt_is_nvims` asserts
  0.12's. `{string}` is the replacement as typed with `~` expanded, as nvim's `sub` is after `regtilde`.
- Rows are read as without `c`: each row's matches are found before any of them is replaced, so a
  replacement is never matched again, and `^` doesn't match after a `\r` split
  (`line_breaks_match_the_row_as_read`). Waiting matches are anchors, and the replacements go into one undo
  entry through `Buffer::edit_in`. No transaction stays open between answers.
- The caret follows nvim (all recorded): it stays on the match last asked about after `n`, `q` or `<Esc>`,
  goes to the start of the row a replacement ended on after `y` or `l`, and goes to the first non-blank of
  the last line changed after `a`. The jump mark is set at the first match even if nothing gets replaced.
- `<C-c>` quits as `q` does, because nvim's prompt takes it (`ctrl_c_quits`). `n` beats `c` (no prompt), `&&`
  keeps `c`, and `:&` drops it.
- Scope added by respec: `ex/normal.rs`, because `:normal` keys that run out while it asks end it as `<Esc>`
  does, as in nvim (`normal_running_out_ends_it`). `ex/global.rs`, because `:g` can't wait for answers, so
  its `:s` with `c` is refused with "the c flag (confirm) under :g: not supported yet", changing nothing.
  `lib.rs` for the crate docs, and `src/tests/substitute.rs`, whose test asserted the old refusal of `c`.
- If something else edits the text between answers (another actor, or a host command), the next answer
  ends the asking as `q` does. A `:s` started while another asks ends that one. Leaving normal mode (a host's
  mouse selection, say) drops it.
- Carried T167 F1 is fixed: `:s\&` with `r` takes the last search's pattern, and so does the substitute
  saved for repeats (`last_substitute_pattern_with_r`, followed by `:s`).
