---
task_uid: "01m55mhyvjnh0xt8hkty5mna8j"
task: "T157"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test insert_completion_matches_neovim (top level, in crates/fe-vim/tests/insert_completion_oracle.rs) passes on crates/fe-vim/tests/corpus/insert_completion.toml, at least 16 cases recorded with nvim 0.12.x, among them (asserted by name) ctrl_n_first_match, ctrl_n_cycles, ctrl_n_wraps_to_typed, ctrl_p_backward, ctrl_p_cycles, ctrl_n_no_match_fails, ctrl_n_no_prefix, ctrl_e_restores, ctrl_y_accepts, typing_accepts, ctrl_n_dedupes, ctrl_n_unicode_word, ctrl_n_dot_repeat."
    required: true
    manual: false
  - id: "C2"
    text: "The integration tests completion_state_follows_cycling and insert_completion_large_buffer (top level, in crates/fe-vim/tests/insert_completion_oracle.rs) pass: VimState::completion() gives the candidates in nvim's order, the selected one following \u003cC-n\u003e and \u003cC-p\u003e, and the typed text's range, and is None once completion ends; a \u003cC-n\u003e with no match on a 200,000-row buffer ends within the command deadline, changing nothing. Bounding a single keyword or candidate of pathological length (copy and replace steps) is out of scope here and is a follow-up card."
    required: true
    manual: false
---

# fe-vim: insert-mode <C-n> and <C-p> complete the keyword before the caret from the buffer's words, cycling as nvim does (Mac T883)

## Problem

At Mac main 6f032b12 insert mode's `<C-n>` and `<C-p>` (keyword completion, `:help i_CTRL-N`) do nothing
in fe-vim, and off macOS ctrl-n and ctrl-p run the workbench's new file and quick open. The user approved
them in the "Vim parity" track on 2026-10-17. nvim 0.12.6 probes (`completeopt=menu,popup`,
`complete=.,w,b,u,t`), buffer `foo food fool` / `` at (1,0): `if<C-n><Esc>` gives row 1 `foo`;
`if<C-n><C-n><Esc>` `food`; `if<C-p><Esc>` `fool` (nvim reports "match 1 of 4": the typed `f` itself
counts as the original).

## Change

In fe-vim, headless; showing a menu is the follow-up card.

- New `crates/fe-vim/src/insert/complete.rs`, declared in `crates/fe-vim/src/insert/keys.rs`: `<C-n>` takes
  the keyword before the caret (word characters as `w` classifies them) and inserts the first keyword in the
  buffer that starts with it and differs, searching forward from the caret and wrapping (`<C-p>` backward),
  each distinct word once, in nvim's order. Further `<C-n>`/`<C-p>` cycle through them, and past either end
  back to the typed text, as nvim does. `<C-e>` goes back to the typed text and ends completion, `<C-y>`
  keeps the match and ends it, and any other key keeps the match and is then handled as usual. With no
  keyword before the caret it completes whole keywords (probe). No match fails as nvim's does.
- Only the current buffer is searched; nvim also scans other windows' and loaded buffers (`complete`'s
  `w,b,u`) and tags: a documented divergence in the module docs.
- `.` repeats the inserted text as nvim does (probe); macros replay the keys.
- `VimState::completion() -> Option<KeywordCompletion>` (the candidates, the selected one, and the typed
  text's range) for a host to show.
- The scan is bounded: one `<C-n>` on a 200,000-row buffer must answer within the command deadline
  (`search::Deadline`), listing candidates lazily as cycling reaches them.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/insert_completion.toml`, recorded into
`insert_completion.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/insert_completion_oracle.rs` modelled on `tests/line_motions_oracle.rs`, which also holds
the large-buffer test.

## Implementation notes

- nvim's search reads the buffer with the typed keyword taken out (it's deleted before the scan), from one
  character before where the keyword started. So the word the caret is in is never a candidate, but text after
  the caret can be (`x\na|ab` + `<C-n>` gives `abab`), and a word equal to the keyword elsewhere is one when the
  keyword is two bytes or more (nvim's pattern is `\<fo`; for one byte it's `\<f\k`, for none `\<\k\k`).
  `complete.rs` follows that, recorded in the corpus (`ctrl_n_text_after_caret`, `ctrl_n_keyword_equal_word`,
  `ctrl_n_skips_own_word`).
- A `<C-n>` with no match leaves the message `Pattern not found` but doesn't fail the key: nvim's beep doesn't
  flush typeahead or stop a macro (`ctrl_n_no_match_fails`, `ctrl_n_no_match_in_macro`).
- Only the text after the keyword changes, at every caret, inside the insert session, so `.`, a count and undo
  need nothing new: nvim's redo holds the same text (`ins_compl_fixRedoBufForLeader`).
- `KeywordCompletion` lists candidates in nvim's menu order (reversed for `<C-p>`, as `complete_info()` gives
  them) and says whether the scan has read every row (`listed_all`). `VimState::completion(&model)` takes the
  model, so it's `None` as soon as the text or the carets change outside completion; any key but its own ends it
  through `VimState::record_key`, and so does the session ending or splitting (`insert::end`).
  `complete_cancel` and `complete_accept` return false outside completion, so `<C-e>` and `<C-y>` stay free for
  nvim's other meanings.
- The search is a state machine whose steps each read at most a 4 KiB window or so (borrowed from the rope's
  chunk when one holds it): walking back over the keyword, probing for its first 256 bytes in the text, or with
  no keyword finding word starts, measuring a word (checking the rest of the keyword as it goes) and taking it.
  Each key takes 64 steps at least, then looks at the clock every 64, so a long row, a long word or a long
  keyword can't hold it past the limit. Words are deduplicated by length and two 64-bit hashes taken while
  measuring, so no step hashes or compares a whole word. An interrupted key keeps the scan, even with nothing
  found, and the next one goes on. The keyword's start is an anchor, so `typed` follows edits at carets before
  the primary one, and validity compares whole selections, not just heads.
- Only fe-vim and its headless host route the keys; binding them in fe-gpui (where ctrl-n and ctrl-p are the
  workbench's off macOS) goes with the menu card.
- Carried T156 F3: `0` and `^` after `<C-o>` (as motions or under an operator) clear the end-of-row return, as
  nvim's `nv_beginline` clears `ins_at_eol`; three one-character-row cases added to `insert_one_command.toml`.
- Every window the search reads starts and ends at a grapheme boundary (`GraphemeCursor` over pieces of the rope),
  and the classes before and at a place are of whole graphemes, so a grapheme longer than a window or the 64 bytes
  read around a place is classed as nvim classes a character with its composing ones (`ctrl_n_after_long_grapheme`
  and the other `long_grapheme` cases; `long_graphemes_stay_whole` for ones longer than a window). With no keyword,
  nvim doesn't then find the word right after such a grapheme; this does, a documented divergence.
- Rev 2 of the spec puts bounding the copy and replace of a keyword or candidate of pathological length out of
  scope (a follow-up card): those steps still copy the whole word.
- With no keyword, nvim's `\<\k\k` searches go on from the end of the match before ('cpoptions' has `c`), so
  a word starting inside a match isn't a candidate (`a日本語` offers only `a`). `chain` follows that forward;
  searching back, whether a match covers a window's start depends on the run of one-grapheme words before it, so
  a resumable walk (`Walk`) finds the run's start a window at a time and notes the parity at the later windows'
  starts it passes, which keeps it linear. Recorded as the `no_prefix_hidden_word`, `no_prefix_alternating`,
  `no_prefix_after_match` and `no_prefix_cjk_first` cases.
