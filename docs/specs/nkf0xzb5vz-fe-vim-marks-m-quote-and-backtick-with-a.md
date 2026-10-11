---
task_uid: "01m4ewvamdhraefjnkf0xzb5vz"
task: "T224"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::marks::marks_as_anchors_and_motions: ma commits vim.set_mark {\"name\":\"a\"}; 'a commits vim.move with a linewise mark motion and `a an exact one; after a row inserted above the mark, 'a lands on the mark's row and `a on its column; after dd over the mark's row, 'a fails with E20 in VimState::message() and moves nothing; d'a is linewise and d`a exclusive; '' after G, /x\u003cCR\u003e and % returns to the position before the jump and '' again goes back; :5\u003cCR\u003e is not a jump, as in nvim 0.12.4 ('' after it alone is E20, and after G:5\u003cCR\u003e '' returns to where G left from); :'a,'bd deletes the rows between the marks; 'a in visual mode moves the visual cursor and keeps the anchor"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test marks_match_neovim (tests/marks_oracle.rs): tests/corpus/marks.toml has at least 25 cases covering ma with 'a, `a, d'a, d`a and y'a, marks in visual mode, '' and `` after G, /, % and :5, a mark on a deleted row (E20, last key), a mark after text inserted above it, '\u003c and `\u003e after visual mode, and :'a,'bd; each matches its nvim 0.12.4 expectation in buffer, cursor, mode, registers and the marks a-z, \u003c and \u003e"
    required: true
    manual: false
  - id: "C3"
    text: "The recorder records the marks a-z that are set as an optional field; every earlier tests/corpus/*.expected.json is byte-identical and marks.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus and unit test passes with its files unchanged; nothing under crates/fe-vim/src/visual/ changes; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs describe marks"
    required: true
    manual: false
---

# fe-vim: marks (m, quote and backtick with a-z, the jump mark, < and >) as motions, after operators, in visual mode and in ex ranges

# fe-vim: marks (m, ' and ` with a-z, '' and the < > marks) as motions and ex ranges

Milestone M7 "Vim", second batch (resized), card 08a: the marks half of the old card 8. Read cards 1,
04b–04c (searches as jumps) and 05a (ex ranges). Design: `docs/decisions/vim-engine.md` §4 (Marks)
and §9 item 10 ("Marks … are `fe_buffer` anchors").

Assumes card 00 (the `visual.rs` split) has landed: marks in a visual mode go through the root
`src/visual.rs` (`move_cursor`, `Mark`), nothing under `src/visual/`.

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help mark-motions`, `:help '`,
`:help ''`, `:help jump-motions`.

## Change (new `src/marks.rs`, `src/motion.rs`, `src/operator.rs`, `src/visual.rs`, `src/ex.rs`, `src/state.rs`)

- `m{a-z}` commits `vim.set_mark {"name"}`: an anchor at the cursor, in `VimState`.
- `'{a-z}` and `` `{a-z} `` are motions (`{"mark": {"name", "exact"}}`): `'` is linewise to the
  mark's row's first non-blank, `` ` `` exclusive charwise to the exact position. Both work after an
  operator (`d'a`, `` y`a ``) and in visual mode.
- `''` and ``` `` ```: the position before the last jump. Jumps are `G`, `gg`, `{count}G`, `%`,
  `/ ? n N * #`, `'x`, `` `x `` (`:help jump-motions`); fe-vim records the position
  before each. `:{line}` is not a jump: nvim 0.12.4 doesn't record one (probed in T224: `:5<CR>''`
  is E20, and after `G:5<CR>`, `''` returns to where `G` left from), so neither does fe-vim. `'<`, `'>`, `` `< ``, `` `> `` use card 1's marks.
- A mark whose row was deleted (`dd` over it) is gone: jumping to it is E20 "Mark not set" in
  `VimState::message()`; a mark on a row that was only edited follows its anchor. The corpus
  decides edge cases.
- 05a's ex ranges accept `'a`–`'z`.
- Marks live in `VimState` (one per editor view).

### Oracle (`tests/support/mod.rs`, `tests/support/oracle.lua`)

`oracle.lua` also records the marks `a`–`z` that are set (card 1 records `<` and `>`), as the same
optional field. No earlier case sets one, so earlier expected files stay byte-identical.

### Corpus

`tests/corpus/marks.toml`, at least 25 cases: `ma` then `'a`, `` `a ``, `d'a`, `` d`a ``, `y'a`, in
visual mode, `''` and ``` `` ``` after `G`, `/`, `%` and `:5`, a mark on a deleted row (E20, last
key), a mark after text inserted above it, `'<` and `` `> `` after visual mode, and `:'a,'bd`.

## Tests (new files)

- `src/tests/marks.rs`, declared in `src/tests/mod.rs`.
- `tests/marks_oracle.rs` (`#[allow(dead_code)] mod support;`) with `marks_match_neovim`.

## Size

About 450 non-test lines (marks and motions ~250, jump recording ~100, ranges, state, oracle ~100).

## Out of scope

Uppercase and numbered marks, `'[ ']`, `'.`, `'^`, `'"`, the jumplist (`<C-o>`, `<C-i>`), the
changelist, `:marks`, `:delmarks`, macros (08b), the GPUI side (11). Any change outside
`crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **Marks are per editor view** (in `VimState`), not per buffer: two splits of one file don't share
  `ma`. Vim shares lowercase marks per buffer.
- **No uppercase (file) marks** yet: they need a buffer identity across tabs.

## Implementation notes

- **`:5` isn't a jump in nvim 0.12.4** (`:help jump-motions` doesn't list `:{N}`; probed: `:5<CR>''` is E20, and `G:5<CR>''` goes back to where `G` left from). fe-vim follows nvim, so the C1 test asserts that, and the corpus records it (`quote_quote_after_colon_5`, `backtick_backtick_after_colon_5_alone`). Rev 1's C1 said `''` after `:5<CR>` returns to the position before it, contradicting C2's "matches nvim"; rev 2 respecced C1 to nvim's behaviour.
- As in nvim, `:d`, a matching `:s` and `&` are jumps too, and a jump that leaves the cursor on the new jump mark (or whose command deletes its row) gives the previous jump mark back if it's still set.
- Marks are a row anchor plus a byte column (as `'<`/`'>` already were), with anchors that detect a deleted row and an exact anchor that a join carries. fe-vim's own linewise deletes and changes, charwise deletes over rows, `O`/`p`/`P`, and typing begun at a row's start adjust marks explicitly, as nvim's do; other edits are read from the text they change. Remaining divergences are listed in lib.rs's known limits.
- Checked with a scratch differential fuzz (about 4,000 random cases, one nvim per case) and a read-only pre-review; the corpus has 70 cases.
