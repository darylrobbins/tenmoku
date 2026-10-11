---
task_uid: "01m4ewve10k2tez70aycd5pyvc"
task: "T226"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::replace::session_overwrite_and_backspace: R commits vim.replace_mode and vim.mode is replace; Rxy\u003cEsc\u003e on \"abcd\" gives \"xycd\" as one undo step with the caret on y; R past the row's end appends; \u003cBS\u003e\u003cBS\u003e after Rxy restores \"abcd\" and a third \u003cBS\u003e stops at the session's start; \u003cCR\u003e splits the row and replaces nothing; 3Rab\u003cEsc\u003e on \"123456789\" gives \"ababab789\"; . repeats the replace; a count of 4294967295 before R returns within the test with O(carets) bookkeeping (as T156's counted insert); VimState::replace_text is public and is what HeadlessVim calls for chars in replace mode; out of scope here (T359): regional-indicator (flag emoji) pairing across counted copies and counted dot-repeat"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test replace_matches_neovim (tests/replace_oracle.rs): tests/corpus/replace.toml has at least 20 cases covering R over a row and past its end, counts, \u003cCR\u003e, \u003cBS\u003e restoring and deleting appended text, multibyte and combining graphemes, . after R and u after it; each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers; every earlier expected file is byte-identical and replace.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C3"
    text: "src/insert.rs changes by at most 60 added or removed lines (git diff --stat), the replace session living in src/replace.rs"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus and unit test passes with its files unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs describe replace mode for hosts"
    required: true
    manual: false
---

# fe-vim: replace mode (R): overwrite typing, backspace restoring what it replaced, counts and dot-repeat

# fe-vim: replace mode (R): overwrite typing, backspace restoring, counts and dot-repeat

Milestone M7 "Vim", second batch (resized), card 09a: the replace-mode third of the old card 9.
Read T156's spec (insert sessions, `.`) and 08b. Design: `docs/decisions/vim-engine.md` §9 item 2
(`vim.mode` `replace`).

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help R`, `:help Replace-mode`.

## File size rule

`src/insert.rs` is about 1,760 lines after T174. The session logic goes in the new
`src/replace.rs`; `insert.rs` gains only the hooks a replace session needs from the insert session
(at most about 60 lines). If more of `insert.rs` must change, block and ask the lead for a split of
`insert.rs` first.

## Change (new `src/replace.rs`, `src/insert.rs`, `src/reducer.rs`, `src/state.rs`, `src/repeat.rs`, `src/headless.rs`)

- `R` commits `vim.replace_mode {"count"}`, opening a session as `i` does (one transaction, `.`
  repeats it); `vim.mode` is `replace`.
- Typed text overwrites graphemes on the row and appends past its end; a line break splits the row
  and replaces nothing. The host types through `VimState::replace_text(model, text)` (and Enter
  through `line_break`); `HeadlessVim` does so for chars in replace mode.
- `<BS>` in replace mode (`VimState::replace_backspace`) moves left and restores the grapheme that
  was replaced there, or deletes what was appended, up to the session's start.
- `<Esc>` repeats the typed text `count - 1` more times (`3Rab<Esc>`), ends the session and returns
  to normal mode, caret one grapheme left.

### Oracle (`tests/support/mod.rs`)

`mode_name` maps nvim's `R` to `replace`. No new recorded fields.

### Corpus

`tests/corpus/replace.toml`, at least 20 cases: `R` over a row, past its end, with a count, with
`<CR>`, `<BS>` restoring and deleting appended text, over multibyte and combining graphemes, `.`
after `R`, and `u` after it.

## Tests (new files)

- `src/tests/replace.rs`, declared in `src/tests/mod.rs`.
- `tests/replace_oracle.rs` (`#[allow(dead_code)] mod support;`) with `replace_matches_neovim`.

## Size

About 350 non-test lines, at most ~60 of them in `insert.rs`.

## Out of scope

The GPUI wiring: replace-mode input and the underline cursor (11). `gR` virtual replace, `r<CR>`.
Any change outside `crates/fe-vim`.

## Round 4 (lead respec, 2026-10-11): fix F10 and F11 here

Review round 3 left C1 unmet because of F10, and unmet required criteria can't be overridden, so this
task fixes F10 and F11 itself before it lands. F12 stays a follow-up. Keep every other change as it is.

- **F10 (replace.rs ~830):** counted replay must stay bounded when a copy produces no net text. On
  `" abc"` at column 2, `4294967295R<CR><BS><Esc>` records `"\n\u0008"`; replaying it on the
  unindented new row yields an empty piece with unchanged state, and the fast path (`piece.len() >
  cut`) never fires, so the loop runs ~4.3e9 times. Fast-forward stable replay cycles with zero net
  growth, and bound shrinking cycles too. Add maximum-count tests for Enter then indentation
  backspace, including a counted dot-repeat, each returning within the test.
- **F11 (replace.rs ~590):** the grapheme-ownership scan stops at every line-break entry, so it won't
  take back generated indentation. On `" abc"` at column 2, `R<CR>` + U+0301 + `<BS><Esc>` must match
  Neovim 0.12.4 (the combining mark joined the last indentation space; both are this session's, so
  `<BS>` removes the whole grapheme). Allow reversal into the live indentation part of a line-break
  entry while keeping the newline; update the retained entry and the replay representation for
  partly removed indentation; add combining-mark cases with counts and dot-repeat, and add them to
  the corpus if the recorder can express them.

## Round 5 (lead respec, 2026-10-11, the user's call): F13 moves to T359

Review round 4 left F13 (medium) not fixed: across counted copies, regional-indicator (flag emoji)
pairing doesn't see the row text before the current copy, in both simulator branches and in counted
dot-repeat. At the user's direction that case is out of scope here and fixed in T359; C1 now excludes
it. Make no code change for F13 in this task: rerun finish as is.

## Implementation notes

- **Shape.** `vim.replace_mode` runs `insert::insert` with `at: before` (so the transaction, the
  undo carets, marks and the indentation ledger are the insert session's), sets the session's
  change to `Change::Replace`, and switches to `Mode::Replace` with a `replace::Replace` record.
  `insert.rs` changes by 35 lines: `Session::origin`/`change` and `place` become `pub(crate)`;
  `end()` takes the replace record's keys for the count's copies (`replace::repeat_typed`) and for
  `.`; `split_session` resumes a split replace session in replace mode.
- **The record (review F1, F2, F5, F6).** One entry per key typed: where what it put in is, as
  offsets followed through every batch of the buffer's edits (a subscription), and what it took
  out. Another origin's batch, an undo or a redo that touches an entry's bytes marks it touched
  for good, even if the bytes come back. A caret's entries are the record whose last entry is
  live and ends just before it, so carets that reorder or merge keep theirs. Entries go in from
  the batch that typed them (a pending note matched by version), so text another origin edits as
  the model catches up afterwards is never claimed. A caret whose overwrite the caret before it
  took is left out before any entry is written, and lands with that caret.
- **`<BS>` (F3, F6)** takes back the last entry, putting back what it overwrote, and widens to the
  whole grapheme before the caret only if the session's own contiguous live entries make up all
  of it (`Re\u0301x<BS><BS>` on `abcdef` gives nvim's `bcdef`; a mark typed onto a base that was
  there before goes alone). With nothing of the session's before the caret it does nothing, so it
  never crosses the session's start. nvim's own stack is blind there; those cases aren't in the
  corpus.
- **Line breaks (F9).** In replace mode `VimState::line_break` is the session's own edit: a line
  break and the row's indentation up to the caret, dropping the blanks after the caret (nvim's
  'autoindent'), which its entry keeps so `<BS>` over the break puts them back. The ledger is
  told the batch is a typed line break, so `Esc` drops indentation left unused. Insert mode keeps
  `EditorModel::newline`, unchanged.
- **Copies and `.` (F4, F8)** type the session's keys again: characters, `\n` for `Enter`, and a
  `\u{8}` after a line break whose indentation `<BS>` took out. `.` types them through the live
  paths (`replace::retype`). The copies are one edit per caret, simulated by `replace::copies`
  with the same rules over the row's rest (indentation that nothing follows goes at the next
  line break or at the end; dropped blanks; `<BS>` taking indentation or, with none, the break).
  Once a copy types what the one before did and leaves the rest as it was or used up, the rest go
  in at once, so a count of 4294967295 costs what a small one does; keys without a line break
  are just repeated. They stop at `MAX_PUT` bytes, as counted insert's do. With `4294967295R`,
  the time goes to the shared step back measuring every grapheme of the 64 MiB row (as for
  `4294967295i`), so C1's test types four-byte characters (2.7 s in debug, not 8.6 s).
- **`Esc` is `vim.replace_normal_mode`** (context `editor && vim.mode == replace`). An existing
  test pins `vim.normal_mode`'s context to `editor && vim.mode == insert`, and C4 keeps earlier
  tests unchanged; the handler is the same `insert::normal_mode`.
- `visual.rs`: `select_visual` does nothing in replace mode, as in insert mode (one line).
- **Round 4 (F10, F11).** nvim's `.` and count copies replay the typed keys literally, `<BS>`
  included (probed on 0.12.4). The record's keys (`Entry::keys`) are still each key left in, plus
  `Entry::then`: keys `<BS>` took back again where typing them elsewhere may leave something.
  That covers characters taken back right after a line break, which mark its indentation used,
  and a combining mark taking a blank with it. It also covers a line break taken out (on a row with
  indentation, `<BS>` takes that instead), and keys taken back at a record's start (a keys-only entry).
  - F11: a `<BS>` whose grapheme starts in a live line break's indentation (a mark typed after
    it joins its last blank) takes the blank and the mark, keeps the line break, shortens its
    entry and records the keys as a bite. `Typing::bite` replays it: the blank goes only where
    the mark joins one.
  - F10: `copies` fast-forwards any settled steady copy (`again`): growing, zero growth, or
    shrinking while there's text to cut (unreachable today: a copy cuts only the indentation the
    copy before ended with, which is in its own piece). `4294967295R<CR><BS>`, its `.` and the mark
    variants return in milliseconds.
  - Each copy keeps a stack of what it typed (`Typed`), so a `<BS>` joins the right line break or
    takes back a character typed before one. `insert.rs` gains `Session::typed_on_row` (8 lines,
    41 changed in all) so a copy that types and takes back characters on the caret's row keeps
    its indentation, as nvim does.
  - Checked by a differential fuzz against nvim 0.12.4 (not committed): four sets of 2,000
    random sessions over rows indented 0–4 blanks, with counts and `.`, two of them weighted to
    `<CR>`, marks and `<BS>`. Mismatches went from 330/296/359/311 to 201/44/234/46. Two cases
    newly differ, both in the smarttab class below (a typed blank, then `<BS>`, in indentation).
    The rest are in the known limits. 46 new corpus cases cover F10, F11, F13 and the replay fixes.
- **Review round 1 of the respec (F13).** A copy's `<BS>` takes the whole grapheme the copy typed
  before the caret (`Typing::grapheme_back`), as the live path does, with the indentation's last
  blank when it joins, putting back only what the last character overwrote. Round 2: the grapheme
  is the last one of the row up to the caret as the copy knows it (the indentation after its line
  break, then what it typed since, then the taken-back characters), segmented whole, so a ZWJ
  sequence goes whole and regional indicators keep their pairs. Text before the copy's own (a copy
  that starts mid-row) isn't context: a grapheme reaching into it takes only the copy's part.
- **Known limits (pre-existing, outside F10/F11; candidates for follow-ups).** Each still differs
  from nvim 0.12.4:
  - *smarttab*: nvim's `<BS>` in indentation goes back to the previous 'shiftwidth' stop
    ('smarttab' is on by default), popping one stack entry per blank. fe-vim takes all of a line
    break's indentation (`"        abc"` col 8, `R<CR><BS>`: nvim keeps 4 blanks) and a typed blank
    alone. Insert mode doesn't model it either.
  - *stack lag*: nvim's `<BS>` over a grapheme the session typed in several keys pops one stack
    entry. The others stay and later `<BS>`s put them back in place of other text (`"abcdef"` col
    1, `Rqé<BS><BS>`: nvim `acdef`, fe-vim `abdef`). fe-vim drops them with their text.
  - *autoindent at the next line break*: nvim takes unused autoindent out when the next `<CR>` is
    typed; fe-vim takes it out at `Esc`. So a `<BS>` joining that line break again differs
    (`"    d f"` col 2, `Rx<CR><CR><BS><BS>`).
  - *blind `<BS>`*: past the session's start, or into a grapheme whose base was there before, nvim
    deletes what was there; fe-vim stops (the spec, review F6). That also shows when `.` types a
    mark onto a row.
