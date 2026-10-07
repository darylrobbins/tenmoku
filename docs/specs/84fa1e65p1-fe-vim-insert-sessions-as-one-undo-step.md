---
task_uid: "01m49xn1a4a1b7rx84fa1e65p1"
task: "T156"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::insert::session_is_one_transaction: HeadlessVim ciwXY\u003cEsc\u003e then u restores the original text in one undo step; 3ix\u003cEsc\u003e inserts xxx and one u removes all three; oA\u003cCR\u003eB\u003cEsc\u003e (o, then two typed lines) is one undo step; an edit with Origin::Ai made through the buffer during a session gets its own transaction and is still present after the user's u"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::insert::insert_mode_routing_and_context: after i, vim.mode is insert on the stamped editor frame and HeadlessVim no longer calls step(); vim.normal_mode is registered with context editor \u0026\u0026 vim.mode == insert, and dispatching it in normal mode gives NotInContext; \u003cEsc\u003e moves the caret one grapheme left except at a line's start; the inserted text recorded for . equals the text between the session's start and the caret when it ended; cw on a word leaves the space after it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::repeat::last_change_rules: d3w\"ayy. deletes three more words (a yank doesn't replace the last change); . after only motions and yanks changes nothing; 3. after d2w deletes three words; . after ciwXY\u003cEsc\u003e replaces the word under the caret with XY as one transaction; \"1p then u. puts register 2; . replays with the repeating invocation's actor as the edit origin"
    required: true
    manual: false
  - id: "C4"
    text: "fe-vim test tests::undo::carets_after_undo_and_redo: on the buffer \"fn foo(a, b) {\\n    let x = foo(a);\\n    x + b\\n}\\nfoo end\", ddu\u003cC-r\u003e from (0,4) ends at (0,4), dwu\u003cC-r\u003e from (1,8) ends at (1,8) and x$u from (2,4) ends at (2,4); 2u undoes two changes; u with nothing to undo returns Effect::Nothing; the recorded pre-change (row, column) pairs never cover more than 1,000 transactions, and a recorded row past the end is clamped to the last row"
    required: true
    manual: false
  - id: "C5"
    text: "fe-vim test insert_matches_neovim (tests/oracle.rs): tests/corpus/insert.toml has at least 20 cases covering i a I A o O with counts, c over motions and text objects, cw on a word and on white space, cc C s S on indented lines, \u003cCR\u003e and \u003cBS\u003e in a session and \u003cEsc\u003e at a line's start; each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers"
    required: true
    manual: false
  - id: "C6"
    text: "fe-vim test edits_match_neovim (tests/oracle.rs): tests/corpus/edits.toml has at least 10 cases covering J with counts, before a ) line, with an empty line and after trailing white space, r with counts and with too few graphemes, ~ with counts and at a line's end, and u and \u003cC-r\u003e with counts; each matches its expectation in buffer, cursor, mode and registers"
    required: true
    manual: false
  - id: "C7"
    text: "fe-vim test repeat_matches_neovim (tests/oracle.rs): tests/corpus/repeat.toml has at least 10 cases including d3w\"ayy., ciwXY\u003cEsc\u003ej0w., x., 3. after d2w, . after A;\u003cEsc\u003e and \"1pu.; each matches its expectation in buffer, cursor, mode and registers"
    required: true
    manual: false
  - id: "C8"
    text: "fe-vim test spot_check_matches_neovim (tests/oracle.rs): tests/corpus/spot_check.toml holds rows 1-21, 24 and 25 of docs/decisions/vim-engine.md §6's spot-check table on §6's buffer with the cursor at (0,4), and each matches its expectation in buffer, cursor, mode and registers; fe-vim test corpus_has_at_least_150_cases counts at least 150 cases across tests/corpus/*.toml, all with expectations recorded by nvim 0.12.4 (the new *.expected.json files are the recorder's unedited output; manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C9"
    text: "vim.insert, vim.join, vim.replace_char, vim.toggle_case, vim.undo, vim.redo and vim.repeat are registered with context editor \u0026\u0026 vim.mode == normal and mutates_buffer true; every earlier corpus test still passes with its files unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted, and in crates/fe-gpui/src/vim/tests.rs only the input of normal_mode_keys_go_to_fe_vim, changed to text fe-vim still forwards in normal mode, e.g. \"QK\", since a is now append); no unsafe in crates/fe-vim; src/lib.rs's crate docs list what later cards add (visual modes, macros, marks, search, ex, and the GPUI wiring of vim-engine.md §10)"
    required: true
    manual: false
  - id: "C10"
    text: "fe-vim test tests::insert::counted_insert_bookkeeping_is_per_caret (review F7): a counted insert's repetition bookkeeping holds one compact descriptor per caret, not a String or vector entry per repetition; the test shows the bookkeeping entry count is the same for 5ix\u003cEsc\u003e and 1000000ix\u003cEsc\u003e on one caret, and the resulting text is still the typed text repeated count times, as one undo step"
    required: true
    manual: false
  - id: "C11"
    text: "fe-vim test tests::insert::newline_provenance_before_sync (review F8): the newline's starting version is registered before newline() is called, without holding the ledger lock across that call; with A then Enter on an indented line containing U+3000, a context stamp through a cloned VimState during newline()'s display synchronization still classifies the newline's indentation as generated, so Esc drops it and dot-repeat doesn't record it"
    required: true
    manual: false
---

# fe-vim: insert sessions as one undo step, c C s S i a I A o O, J r ~, undo and redo, and dot-repeat

# fe-vim: insert sessions as one undo step, c C s S i a I A o O, J r ~, undo and redo, and dot-repeat

Milestone M7 "Vim", card 3 of 3 (see card 1's table). Read card 1's and card 2's specs first. The
design is `docs/decisions/vim-engine.md`:

- §1 (outline: "An insert session is one `fe_buffer` transaction, started by the operator that
  opened it", "Dot-repeat of the last mutating invocation plus its inserted text")
- §4 (the `ciw`/`.`/`u` traces, and the `d3w"ayy.` divergence)
- §6 (spot check)
- §9 items 5, 8 and 11
- §10: in insert mode, keys propagate as text; `Esc` and `ctrl-[` return to normal mode

The exit criterion of the first `fe-vim` work: at least 150 corpus cases green against nvim
0.12, the §6 spot-check commands among them.

**Clean room** as card 1: no Vim/Neovim source, no hjkl, no Zed `vim` crate, no Helix.

## Change

### Insert mode (`src/insert.rs`, `src/state.rs`, `src/commands.rs`)

- `Mode::Insert`. `vim.mode` is `insert` (§9.2), stamped as card 1 describes.
- **Entering it.** `vim.insert { at, count }` (`InsertArgs`), committed by `i a I A o O`. `at` is
  one of:
  - `before`, `after`
  - `first_non_blank` (`I`), `line_end` (`A`)
  - `line_below`, `line_above` (`o`, `O`): they open a line that copies the current line's
    indentation, as nvim's default `autoindent` does
- `c` as an operator (card 2 answered it with an error):
  - it deletes like `d` (registers included)
  - then it enters insert mode in the same transaction
  - `cw`/`cW` on a non-blank act as `ce`/`cE` (`:help cw`)
  - `cc` and `S` keep the line's indentation
  - `C` is `c$`, `s` is `cl` and `S` is `cc`, all reducer aliases
- **The session (§9.8).** The command that enters insert mode opens one `fe_buffer`
  transaction:
  - It calls `model.buffer_mut().start_transaction(origin)` with the actor's origin, and
    records the session's start as an anchor.
  - Text typed in insert mode arrives as ordinary editor input (§10 step 3: in the real app
    the editor's text input calls `EditorModel::insert`). It joins that transaction because
    its origin matches. Edits from other origins get their own transactions (fe-buffer's
    rule).
  - `vim.normal_mode` (context `editor && vim.mode == insert`; the default keymap will bind
    `Esc` and `ctrl-[` to it in the GPUI wiring card) does four things:
    - repeats the typed text `count - 1` more times (`3ix<Esc>` gives `xxx`; `3o` opens
      three lines)
    - ends the transaction
    - moves the caret one grapheme left unless it's at the line start
    - returns to normal mode
- **`HeadlessVim` in insert mode** stands in for the editor's text input and the keymap:
  - a `Char` calls `EditorModel::insert`
  - `<CR>` calls `EditorModel::newline` (it copies indentation, like `autoindent`)
  - `<BS>` calls `EditorModel::backspace`
  - `<Esc>` dispatches `vim.normal_mode`
  - any other key is `FeedError::Forwarded`
  - `step()` is never called in insert mode
- **The inserted text** recorded for `.` is the text between the session's start anchor and the
  primary caret when the session ends, or empty if the caret is before the start.

### `J`, `r`, `~` (`src/edit.rs`)

- `J` is `vim.join { count }`. It joins `max(count, 2)` lines:
  - the joined line's leading white space goes, and one space is put in, as `:help J`
    describes (none before `)`, none when the joined line is empty, none after trailing white
    space)
  - the caret goes to the join point
  - on the last line it does nothing
- `r{char}` is `vim.replace_char { char, count }`, the reducer gaining a `replace` pending
  kind. It replaces `count` graphemes with the char, and does nothing if fewer than `count`
  remain on the line. The caret ends on the last replaced grapheme. `r<CR>` cancels in this
  card.
- `~` is `vim.toggle_case { count }`. It toggles the case of `count` graphemes on the line and
  moves past them, stopping at the last grapheme.
- Each of these is one transaction with the actor's origin.

### Undo and redo (`src/history.rs`)

- `u` and `<C-r>` commit `vim.undo { count }` and `vim.redo { count }` (§10: in normal mode,
  vim wins ctrl keys). They call `EditorModel::undo`/`redo` `count` times, stopping early when
  there's nothing left.
- **Caret.** For every transaction fe-vim produces, `VimState` records the carets just before
  it as (row, byte column) pairs, keeping the last 1,000 transactions.
  - Undo and redo restore the pairs recorded for the transaction they reverted or reapplied,
    clamped: the row to the last row, the column to that row's last grapheme start.
  - Pairs, not anchors: an anchor inside text that `dd` deleted collapses, and nvim's redo
    goes back to the old row and column.
  - If nothing is recorded, they keep `EditorModel`'s placement, clamped to normal-mode rules.
  - Known limit (put in the crate docs): another origin's edits between a change and its
    undo can shift the rows.
- nvim agrees with "restore the carets before the change" for undo and redo alike. A probe on
  §6's buffer: `ddu<C-r>` from (0,4) ends at (0,4); `dwu<C-r>` from (1,8) ends at (1,8);
  `x$u` from (2,4) ends at (2,4); `ciwzz<Esc>0u<C-r>` from (1,8) ends at (1,8).

### Dot-repeat (`src/repeat.rs`, §9.5)

- `VimState` keeps the last change: the last committed invocation of `vim.operate` (not a
  yank, §4), `vim.put`, `vim.insert`, `vim.join`, `vim.replace_char` or `vim.toggle_case`,
  plus the inserted text if it opened an insert session. Moves, yanks, undo and redo don't
  replace it.
- `.` is `vim.repeat { count }`. It replays the last change, and the inserted text, as one
  transaction with the repeating actor's origin. A new count replaces the original's.
- Repeating a put from numbered register `N` (1–8) uses `N + 1`, as `"1pu.u.` relies on.
- With no last change, it does nothing.

### Corpus (recorded with card 1's recorder and nvim 0.12.4)

- `tests/corpus/insert.toml`, with at least 20 cases:
  - `i a I A o O`, with counts
  - `c` over motions and text objects, `cw` on a word and on white space
  - `cc`, `C`, `s`, `S` on indented lines
  - `<CR>` and `<BS>` inside a session
  - `<Esc>` at the line start
- `tests/corpus/edits.toml`, with at least 10 cases: `J` (counts, a `)` line, an empty line,
  trailing white space), `r` (counts, too few graphemes), `~` (counts, line end), and `u`/`<C-r>`
  with counts.
- `tests/corpus/repeat.toml`, with at least 10 cases. They include `d3w"ayy.` (§4), `ciwXY<Esc>`
  then `j0w.` (§4's trace), `x.`, `3.` after `d2w`, `.` after `A;<Esc>`, and `"1pu.`. No case
  starts with `.` (card 1's loader).
- `tests/corpus/spot_check.toml`: §6's table rows 1–21, 24 and 25, on §6's buffer
  `"fn foo(a, b) {\n    let x = foo(a);\n    x + b\n}\nfoo end"` with the cursor at (0, 4), under
  the oracle's options. Rows 22 (`/foo<CR>n`) and 23 (`*`) are search, a later card.

### Tests (new files)

- `src/tests/insert.rs`, `src/tests/repeat.rs` and `src/tests/undo.rs`.
- In `tests/oracle.rs`: `insert_matches_neovim`, `edits_match_neovim`,
  `repeat_matches_neovim`, `spot_check_matches_neovim`, and `corpus_has_at_least_150_cases`
  (it counts across every `tests/corpus/*.toml`).

## Out of scope

- Visual modes, macros, marks, search and ex (later M7 cards, §1).
- Splitting the session on cursor keys inside insert mode (§9.8's `<C-g>u` rule). In the real
  app those keys are macOS editor commands (§10), so the GPUI wiring card ends the session
  when a non-insert command runs.
- The GPUI wiring (§10) and the `vim.enabled` setting.
- Any change outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **The last change lives in `VimState`,** not in the `fe-command` registry as §9.5 suggests.
  The registry isn't changed until a second user of repeat appears.
- **The inserted text is read from the buffer** (start anchor to caret), not from keys. In the
  real app, typed text never passes through fe-vim (§10 step 3).
- **Caret restore after undo/redo** comes from fe-vim's own record of carets before each
  transaction, so `fe-buffer` and `fe-editor` don't change.

## Implementation notes

**Where nvim 0.12.4 and the spec's wording differ, the code follows nvim.** Each rule below is
a recorded corpus case or was checked against isolated nvim runs.

- **Undo and redo carets** come from fe-vim's record, as decided, but the record holds where
  each change *began*, which is where nvim puts the caret back. That is "the carets just
  before it" for `x`, `dw`, `dd`, `J`, `r`, `~`, `p`, `o` and `O`, and for the four probes in
  the spec. It differs from them in these cases:
  - after an operator that moved the caret first: `X`, `db`, `d0` and `dF` start at the
    range's start;
  - `i`, `a`, `I`, `A` and `c` start where typing starts;
  - a one-row `cc` or `S` starts at the first non-blank, or at the caret if it's before;
  - a linewise operator starts on its first row at the caret's column, clamped to that row
    as it was.

  `edits.toml` records `Xu`, `dbu<C-r>`, `Ix<Esc>u`, `ccx<Esc>u`, `>ku<C-r>` and others.
- **`<Esc>` drops indentation nothing was typed after.** This is nvim's 'autoindent': `o<Esc>`,
  `cc<Esc>` and `A<CR><Esc>` on indented rows leave empty rows (`insert.toml`). `o` and `O`
  open rows indented with spaces, as 'expandtab' does. `cc` keeps the row's own indentation.
  A count's copies made by `o` and `O` take the indentation of the row before them, as
  replaying the keys would. As in nvim, typing deleted since still counts as typing on a row,
  while a backspace or a line break doesn't, and the count's copies follow the same rule
  (`ox<BS><Esc>`, `o<BS><Esc>`, `o<CR><Esc>`, `2A<CR>x<BS><Esc>` and others in `insert.toml`).
- **A session is one region per caret.** Each region is the text between a start anchor
  (text typed at it goes after it) and an end anchor (text typed at it goes before it). Each
  caret's count copies its own region's text.
- **The session follows its own indentation through every edit** (`Indents` in
  `src/insert.rs`). It holds the bytes it made as indentation: what `o` and `O` opened a row
  with, what a linewise `c` kept, and what a line break typed in insert mode copied. Only the
  host knows which edits are typed line breaks, so it types `Enter` in insert mode through
  `VimState::line_break` (public; `HeadlessVim`'s `<CR>` does, and the GPUI wiring will). That
  catches the model up, notes the buffer's version with the session, and only then calls
  `EditorModel::newline`, holding no lock across it, so its batch is the one from that
  version whatever measuring text edits around it, and a context stamped through a clone
  while `newline` measures text (which follows the batch) already knows it's a line break.
  If it typed nothing (no caret), the note is taken back, so no later batch is taken for it.
  Any other input, `EditorModel::newline` called directly included, is typed text, whatever it
  holds, so a paste of a line break and blanks keeps its blanks. Catching the model up
  after an edit measures text, which may edit the buffer through another model, so the
  positions an edit of the session's leaves (carets, regions, pieces, the count's copies) are
  taken in the snapshot right after it, and the session subscribes there. It subscribes
  to the buffer's events and maps those pieces through each batch in
  order, whoever made it: bytes an edit deleted drop out, and text inserted inside splits a
  piece without joining it. Each piece notes whether the session typed on its row since it was
  made, and whether another origin deleted some of it or wrote inside it. At `<Esc>`, a piece
  goes only if it's still there, nothing was typed on its row, no other origin changed it, and
  its row holds nothing but white space; it never reaches past its row. The events are folded
  in whenever the context is stamped (the host does for every key), so they don't pile up
  until `<Esc>`. Clones of the state share the record, and a session ends once: ending it
  again through a clone returns that clone to normal mode with the session's outcome (its
  last change for `.` and its carets record), without editing again. So:
  - Blanks that were already there, or that another origin wrote, never go, even when an
    edit joins them to the session's row (another origin deleting the line break after an
    opened row leaves the blanks of the row it joined).
  - Typing at one caret, or on another row, doesn't keep a different row's indentation.
  - Where the caret ended doesn't matter.
  - A count's copies hold the indentation their text held, and type their first row on the
    row each starts on, as typing them again would (`2A <CR><Esc>`). The rows `o` and `O`
    open for them are indented only if the row they repeat was typed on, or holds more than
    blanks.
  - A count's copies go in without the indentation `<Esc>` would drop from them wherever
    the copies alone decide that (every row but the last copy's last, which runs on into the
    text after them). So the session notes one entry per caret, plus the indentation left on
    that last row, however large the count, and `<Esc>` has nothing more to remove from them
    than from one copy.
- **The text `.` keeps** follows the spec's rule (start anchor to the primary caret, empty if
  the caret is before the start). It's read after the indentation cleanup and before the
  count's copies. `.` types it again as typed text, never as indentation, and when the session
  typed on the row it began on, even if that typing was deleted since, `.` keeps that row's
  indentation too (`ox<BS><Esc>j.` in `repeat.toml`).
- **Measuring text while the model catches up** may edit the buffer through another model, or
  stamp the context (or even end the session) through a clone of the state. So the session's
  record is locked only between catch-ups, never across one. `Esc` notes that the session is
  ending, so it ends once, and registers the count's copies before they go in, so a stamp that
  follows their batch first still knows what they are. `J`, `r`, `~`, `>`, `<`, `d` and `p`
  land their carets through the snapshot right after their own edit, as the insert paths do.
  fe-vim's tests can't build a `CharWidth`, so a test-only hook (`operator::MEASURING`) stands
  in for it wherever fe-vim's own catch-ups end.
- **A caret another origin's edit left inside a grapheme**, or past a row's last one, is on
  that grapheme, as nvim clamps its cursor. `a`, `x`, `cw` and `p` act on all of it, as nvim
  does from such a cursor. `i` and `P` put text before it; nvim, whose cursor gets there only
  through its API, puts it at the byte. So `tests::insert::insert_mode_routing_and_context`
  now starts its whole-grapheme `<Esc>` case on the `x` after `é`, not inside `é`.
- **A session's carets record** is held apart from the bounded 1,000 until its transaction
  closes, and kept only if the transaction changed something, `.`'s included. So an empty
  session never pushes out a real record.
- **What `.` records**, beyond the spec's list:
  - `d`, `>` and `<` count once their target resolved, even over nothing (`x` on an empty
    row). A failed motion doesn't count.
  - A put counts even from an empty register.
  - A `J` that joins nothing doesn't count, but a count of 3 or more on the last row does. It
    moves the caret to the row's start, as nvim does, and `.` then repeats the count clamped
    to the rows that were there.
  - A `.` whose replay fails leaves the stored count alone.
- **`C`, `c$` and `ciw` on an empty row write an empty register.** `s`, `cl`, `c0` and every
  `d` don't.
- **`J`'s space** depends on the row joined last, without its indentation, ending in white
  space. So a row of blanks doesn't suppress the space, and joining an empty row puts none
  in.
- **`r<Tab>`** replaces with spaces to as many tab stops, as nvim does with 'expandtab'
  (`r_tab`, `r_tab_count`).
- **A failed `vim.operate` or `vim.put`** (an unwritable register, a put over 64 MiB) leaves
  `.`'s last change as it was.
- **`~`** toggles each grapheme's first character by nvim's own table. That is `src/case.txt`,
  written by the ignored `record_case_table_with_neovim` (`tests/support/case.lua`), as card 1
  did for `src/chars.txt`. Rust's case mappings differ for about 140 code points (`ß`, `İ`,
  title case, newer Unicode).

**Files beyond the spec's list**, all in `crates/fe-vim`:
- `src/case.txt` and `tests/support/case.lua`, the `~` table and its recorder.
- `src/tests/edits.rs`, for carets inside another caret's `J`, `r` or `~`.
- `src/tests/reducer.rs` and `src/tests/operators.rs`, where two card-2 assertions pinned
  behaviour this card replaces. `3i` cancelled and `i` was forwarded; `c` returned an error.

**Known limits** are in the crate docs (`src/lib.rs`). Briefly:
- `fe_buffer` drops empty transactions, so a change that changed nothing (such as `~` on
  punctuation) leaves no undo step, unlike nvim.
- Typed `<CR>` and `<BS>` are fe-editor's, not 'autoindent' and 'smarttab'.
- A session's typing is its origin's edits, so a split view typing with the same origin joins
  it, and another origin's undo or redo closes its transaction (fe-buffer's rules).
- fe-buffer keeps one open transaction per buffer, so another origin's vim change through
  another view that opens its own (an insert session, `c` or `.`) nests into a session's
  (fe-buffer asserts against it in debug builds); fe-vim can't see the open transaction's
  origin to refuse it.
- `.` and a count retype the session's text as the buffer holds it, not its keys.
- `"{reg}.` ignores the register.
- After undoing a `c` over several rows, the caret's column is approximate.
