---
task_uid: "01m4bq04rdd2762ttb9hcq6968"
task: "T173"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-vim test tests::visual::reducer_and_commands: from normal mode v commits vim.visual {\"kind\":\"char\"} and V {\"kind\":\"line\"}; in visual mode d, y, c, \u003e, x and ~ commit vim.operate with target \"selection\" and no Wait, X, D, Y, C, S and R commit the linewise variants, o commits vim.visual_swap, gv commits vim.reselect, \u003cEsc\u003e commits vim.visual_exit and iw commits vim.select {\"text_object\":\"inner_word\"}; 3\u003e in visual mode carries count 3; vim.mode is visual or visual_line and the editor frame has the flag vim.visual, which normal mode never has; vim.visual, vim.visual_exit, vim.visual_swap, vim.reselect and vim.select are registered with contexts that hold only in their modes, and step() still takes no model"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::visual::selection_on_the_model: on \"abc\\nde\\u{301}f\" from (0,0), v gives the model selection 0..1 and vl gives 0..2 not reversed; from (0,2) vh gives 1..3 reversed and HeadlessVim::cursor() is (0,1) with visual_anchor() (0,2); from (1,1) v covers the whole e plus combining mark (bytes 5..8); V covers row 0 from its start to its end without the newline and Vj covers both rows; with two carets, v keeps only the primary; vim.move in visual mode extends whatever its extend arg says"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::visual::operators_marks_and_reselect: vjd on two rows is one transaction and one u restores it; Vjd writes both rows linewise to \" and shifts register 1; vy writes \" and 0 and leaves mode normal with the caret at the selection start; vc ends in insert mode, in the same transaction as the delete; after leaving visual mode by \u003cEsc\u003e, by an operator or by switching away, VimState::mark('\u003c') and mark('\u003e') hold the extent's ends; gv after a line inserted above the old extent reselects the same text; gv with no earlier visual extent does nothing; o swaps cursor and anchor; VimState::select_visual(model, Char, 0, 3) enters charwise visual mode over bytes 0..4 with visual_cursor() 3, and with Line enters linewise mode"
    required: true
    manual: false
  - id: "C4"
    text: "fe-vim test tests::visual::named_arrows_continue_pending_input (T161 review F5): 3\u003cDown\u003e moves three rows, d\u003cDown\u003e deletes two rows as dj does, 2d\u003cRight\u003e deletes two graphemes, and v\u003cRight\u003e extends the selection by one grapheme, each with no Cancel"
    required: true
    manual: false
  - id: "C5"
    text: "fe-vim test visual_matches_neovim (tests/visual_oracle.rs): tests/corpus/visual.toml has at least 40 cases covering v and V with motions and counts, switching kind, o, gv after an edit, \u003cEsc\u003e with the '\u003c and '\u003e marks, each of d x y c s \u003e \u003c ~ u U J r X D Y C S R in both kinds, viw, a repeated iw, vi( and va\", empty rows, multibyte and combining graphemes, $ in visual mode, 3\u003e and . after visual changes in both kinds; each matches its nvim 0.12.4 expectation in buffer, cursor, mode, registers, the visual anchor when the case ends in a visual mode, and the '\u003c and '\u003e marks"
    required: true
    manual: false
  - id: "C6"
    text: "The recorder (oracle.lua and tests/support/mod.rs) records the visual anchor only for cases ending in a visual mode and the '\u003c and '\u003e marks only when set, both optional fields of Outcome; every earlier tests/corpus/*.expected.json is byte-identical and visual.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C7"
    text: "In src/tests/reducer.rs only the v of prefixes_targets_cancel_and_forward's forwarded keys changes, to a key fe-vim still forwards; every earlier corpus test passes with its files unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs describe visual mode and update the list of what later cards add"
    required: true
    manual: false
---

# fe-vim: charwise and linewise visual mode (v, V), operators on the selection, o, gv, the < and > marks, and . after a visual change

# fe-vim: charwise and linewise visual mode (v, V), operators on the selection, o, gv, '< and '>, and . after a visual change

Milestone M7 "Vim", second batch, card 1 of 11: visual (1–3), search (4), ex (5), the command
line (6), clipboard (7), marks and macros (8), replace/increment/folds (9), scrolling (10), and
their GPUI wiring (11). Cards 1–3 of the first fe-vim work (T154, T155, T156) and the GPUI wiring (T161, T162)
are done or queued. Read their specs in `docs/specs/` first, then `docs/decisions/vim-engine.md`:

- §4 (Modes: Visual, VisualLine; `'<`/`'>`)
- §9 item 2 (`vim.mode` values `visual` and `visual_line`), item 4 (the `Selection` target), item 7
  ("Visual selections include the character under the cursor", "Leaving visual mode keeps
  `'<`/`'>`")
- §10 (normal, visual and operator-pending keys go to `step()`; no text input in visual mode)

**Clean room**, as every M7 card (`CLEAN_ROOM.md`): no Vim/Neovim source, no hjkl, no Zed `vim`
crate, no Helix. Behaviour comes from running `nvim --clean` (0.12.4) and `:help visual-mode`,
`:help visual-operators`, `:help visual-repeat`, `:help v_o`, `:help gv`. The corpus is the spec:
when unsure, add a case and record it.

## Change

### Modes and the reducer (`src/state.rs`, `src/reducer.rs`, new `src/visual.rs`)

- `Mode::Visual(VisualKind)` with `VisualKind::{Char, Line}` (card 2 adds `Block`). `vim.mode` is
  `visual` or `visual_line`. A visual mode also stamps the flag `vim.visual` on the editor frame,
  so the GPUI card can bind keys once for every visual kind.
- `step()` stays pure and model-free. It needs to know a visual mode is on (an operator key then
  commits at once instead of waiting for a target); carry that in `Pending` (set by `VimState`
  before stepping) or as a `step` argument, your call, documented.
- New commands on fe-vim's registry:
  - `vim.visual {"kind": "char" | "line"}` from `v` / `V`: enters from normal mode, switches kind
    from the other visual kind, and leaves visual mode when the kind is the current one. A count
    before `v`/`V` from normal mode is out of scope (`:help v_count` reselect rules) and cancels.
  - `vim.visual_exit` from `<Esc>` (and `ctrl-[` through the host's mapping) in a visual mode.
  - `vim.visual_swap` from `o`: the cursor goes to the other end.
  - `vim.reselect` from `gv`: the last visual extent and kind again, from anchors; with no last
    extent it does nothing. `gv` inside visual mode swaps with the previous extent, as nvim does.
  - `vim.select {"text_object": …, "count": …}` from a text object typed in visual mode (`iw`,
    `aw`, `i(` … card 2's twelve and their aliases): extends the selection as nvim does,
    including a repeated `iw`.
- `vim.move` is also in context in the visual modes. There it moves the visual cursor and keeps
  the anchor (whatever `extend` says), with goal columns as in normal mode.
- **Named arrow keys continue pending input** (T161's review F5, never filed as a task): after a
  count or an operator, `<Left> <Right> <Up> <Down>` act as `h l k j` instead of cancelling, in
  normal and visual modes.

### The selection on the model

- The vim cursor and the anchor are byte offsets of grapheme starts, kept by `VimState` as
  `fe_buffer` anchors. The model's selection is what is painted and copied:
  - charwise: from the earlier end to the end of the later end's grapheme (it includes the
    grapheme under the cursor), `reversed` when the cursor is the earlier end;
  - linewise: from the start of the first row to the end of the last row, newline excluded.
- Entering visual mode with several carets keeps only the primary (nvim has one cursor).
- `HeadlessVim::cursor()` reports the vim cursor in visual modes, not the model's selection head.
  Add `HeadlessVim::visual_anchor() -> Option<(u32, usize)>`.
- For hosts: `VimState::visual_cursor()` (the vim cursor's offset, for painting the block cursor)
  and `VimState::select_visual(model, kind, anchor, cursor)` (enter or update a visual mode over a
  range, for mouse drags in card 3), both public and documented.

### Operators on the selection (`src/operator.rs`, `src/edit.rs`)

- In a visual mode `d x y c s > < ~ u U J r{char}` commit with target `"selection"`
  (`Target::Selection`), and `X D Y C S R` with the linewise variants (`:help v_X`, `v_D`, `v_Y`,
  `v_C`, `v_S`, `v_R`). `u`/`U` are new lowercase/uppercase operations over the selection only (not
  `gu`/`gU` in normal mode). A count before `>`/`<` shifts that many times. Registers follow card
  2's rules (a charwise visual delete within a row writes `-`, a multi-row one shifts `1`–`9`).
- Each change is one transaction with the actor's origin; `c`/`s` open an insert session in it,
  as card 3's `c` does. After any operator the mode is normal (insert for `c`).
- Leaving visual mode by any path sets `'<` and `'>` (anchors; the `<` mark is the start of the
  first grapheme, `>` the start of the last; linewise uses column 0 and the row's last byte as nvim
  reports them through `getpos`). They are kept in `VimState` for `gv` and for the ex card's
  `'<,'>` range. Readable via `VimState::mark('<' | '>')`.

### Dot-repeat (`src/repeat.rs`)

`.` after a visual change repeats it on the same amount of text from the cursor
(`:help visual-repeat`): the same number of rows for linewise, and for charwise the same number of
rows, with the last row's count of characters (or the same count of characters within one row).

### Oracle (`tests/support/oracle.lua`, `tests/support/mod.rs`)

- `oracle.lua` records `visual = {row, byte_col}` (`getpos('v')`) only when the case ends in a
  visual mode, and `marks = {"<": [row, col], ">": [row, col]}` only for marks that are set.
  `Outcome` gains both as optional fields (`#[serde(default, skip_serializing_if = …)]`), so every
  earlier `*.expected.json` stays byte-identical after a re-record. `check` compares them.
- `mode_name` maps nvim's `v` and `V`.

### Corpus

`tests/corpus/visual.toml`, at least 40 cases: `v` and `V` with motions and counts, switching
kind, `o`, `gv` after an edit above the old extent, `<Esc>` and the `'<`/`'>` marks, each operator
listed above in both kinds, text objects in visual mode (`viw`, repeated `iw`, `vi(`, `va"`),
selections over empty rows, multibyte and combining graphemes, `$` in visual mode, a count with
`>`, and `.` after visual changes in both kinds. Recorded with the existing ignored recorder.

## Tests (new files)

- `src/tests/visual.rs` (declared in `src/tests/mod.rs`).
- `tests/visual_oracle.rs`, an integration test file declaring `#[allow(dead_code)] mod support;`
  (so the items only `oracle.rs` uses don't fail clippy), with `visual_matches_neovim`.

## Existing tests

`tests::reducer::prefixes_targets_cancel_and_forward` lists `v` among keys forwarded with nothing
pending. Replace `v` there with another key fe-vim still forwards; change nothing else in that file.

## Size

Estimate about 850 non-test lines in `crates/fe-vim/src` (tests and corpus excluded). The budget is
1,000. If it will clearly pass that, block with a split proposal rather than trimming criteria.

## Out of scope

- Blockwise visual (`<C-v>`), `p`/`P` in visual mode, blockwise registers (card 2).
- The GPUI wiring: key bindings, painting, the status bar (card 3).
- `:` from visual mode (the ex card), `gn`, `v_g?`, `v_gq`, `v_=`, `v_!`, select mode, `gv` after
  undo, counts before `v`/`V`, `1v`.
- Any change outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **Visual state lives in `VimState`** and the model's `SelectionSet` mirrors it as an inclusive
  range, so the existing selection painting, `editor.copy` and the status bar's selection count
  work unchanged.
- **One cursor in visual mode.** Several carets collapse to the primary when visual mode starts.
- **`u`/`U` only in visual mode** here; `gu`, `gU`, `g~` with motions are a later card.
