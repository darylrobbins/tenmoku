---
task_uid: "01m50fnrfswmktqgpqewgfdm7q"
task: "T98"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test scroll_display_oracle::scroll_display_matches_neovim passes: tests/corpus/scroll_display.toml has at least 23 cases, every case name the spec lists among them, each with window_rows 10 and either folds or soft_wrap, recorded by nvim 0.12.4, and fe-vim matches nvim on every case, the view's top included"
    required: true
    manual: false
  - id: "C2"
    text: "With no closed fold and no wrapped row the scroll keys are unchanged: scroll_oracle, jump_fold and every other fe-vim oracle test pass, and re-recording leaves every *.expected.json except scroll_display.expected.json byte-identical"
    required: true
    manual: false
  - id: "C3"
    text: "Scope: nothing under crates/fe-editor or crates/fe-gpui changes, and fe-gpui's vim::scroll_tests pass unchanged"
    required: true
    manual: false
---

# Make vim scrolling display-row exact under closed folds and soft wrap, checked against nvim (T230 review F1) (Mac T447)

Filed on argon from Mac T447 (card ~/coordination/argon-cards/T447.json rev 2; note zi). Mac task uid 01m4pv6e52v56gynyyha480j3b. Base: argon trunk dbc87ea = mac/main fbf7ff3 synced. Recording needs nvim 0.12.4 (pinned in tests/support/mod.rs), which argon has. Mac deps already on the trunk: Mac T230. Title the host commit "... (Mac T447)".

# Make vim scrolling display-row exact under closed folds and soft wrap (T230 review F1)

Follow-up of T230 (review F1, deferred by the lead in T230 rev 3). M7 "Vim". **Clean room**, as every M7
card: no Vim/Neovim source, no hjkl, no Zed, no Helix. Recorded nvim behaviour is the reference.

## Problem

`crates/fe-vim/src/scroll.rs` (`run`, `page`) counts every distance in text rows: its module doc says
"Rows are the text's rows: a fold or a wrapped row doesn't change how far they go." nvim counts the rows
the window shows. Probed on nvim 0.12 (2026-10-15), window 10 rows, 'scroll' 5, 60 rows `line N`:

- closed folds over rows 3-12 and 20-29, cursor and top at row 0: `<C-d>` puts top and cursor on row
  **14** (5 display rows: 1, 2, the fold, 13, 14); fe-vim gives 5, a row inside the fold. `<C-d><C-d>`:
  19. `zb` on row 35: top 17. `zz` on row 35: top 31; then `<C-u>`: top 17, cursor 30. `<C-f>`: top 17.
- every third row 95 chars of `x` (two rows at nvim's 80 columns): `<C-d>` from the top: top and cursor
  4; again: 7. `<C-e><C-e>`: top 2 (`<C-e>` scrolls whole text rows: 'smoothscroll' is off). `zb` on
  row 30: top 23 (ten display rows); `zt<C-u>` on row 30: top and cursor 26.

So nvim's top is always a text row's first display row, and `ScrollRequest::TopRow(row)` can still
express every result. What is missing is fe-vim's arithmetic, not the host contract: `EditorHost`
already gives `viewport()` (top text row, and `rows`, which the GPUI host fills with display rows from
`frame.visible_rows()`) and `top_display_row()` (T580), and `EditorModel::display()` maps text rows to
display rows through folds and soft wrap. `jump::view_rows` already counts this way. fe-gpui's
`EditorView::scroll_to_top_row` already converts the requested text row with `row_for_buffer_row`.

`HeadlessVim::follow_cursor` (`crates/fe-vim/src/headless.rs`), which keeps the oracle's view on the
cursor after each key as nvim's window does, counts text rows too, so a fold or wrap case could not be
checked against nvim even with `scroll.rs` fixed. The oracle harness can't express folds or soft wrap.

## Change

### `crates/fe-vim/src/scroll.rs`

Count in the display rows of `target.model.display()` (after `sync_display`), not text rows:

- The view's first display row is `host.top_display_row()` when it lies on `viewport().top_row`, else
  that row's first display row (as `jump::view_rows` does). The view shows `viewport().rows` display rows.
  'scroll' (half the view's rows) and a counted 'scroll' are display rows.
- `<C-d>`/`<C-u>` move the top and the cursor by 'scroll' display rows, each landing on the text row that
  shows at the display row reached (a closed fold's first row), and `<C-d>` stops where the last display
  row shows at the bottom.
- `<C-e>`/`<C-y>` move the top by `count` shown text rows: a closed fold is one, a wrapped row is one
  (it moves by all its display rows). The cursor moves only to stay in the view, counted in display rows.
- `<C-f>`/`<C-b>` (`page`) count the view and its two-row overlap in display rows.
- `zt`/`zz`/`zb` (and `z<CR>` `z.` `z-`) put the cursor's row first, centred or last in display rows:
  `zb` so its last display row is the view's last.
- The new top is always a text row; ask for it with `ScrollRequest::TopRow` as today. Where the
  display-row count lands differently from this description, the recorded nvim result wins.
- Rewrite the module doc's "Rows are the text's rows" paragraph to say what it counts now.

With no closed fold and no wrapped row in reach every result is the same as today.

### `crates/fe-vim/src/headless.rs`

`HeadlessVim::follow_cursor` applies its existing nvim rules (close above: cursor at top; further:
centred; close below: cursor at bottom; and so on) counted in display rows, so a view with a fold or
a wrapped row stays where nvim's does. `ViewHost` is unchanged.

### Oracle harness (`crates/fe-vim/tests/support/mod.rs`, `crates/fe-vim/tests/support/oracle.lua`)

- `Case` and `Recorded` gain `folds: Vec<(u32, u32)>` (closed folds, first and last text row from 0;
  `serde(default)`, skipped when empty) and `soft_wrap: bool` (`serde(default)`, skipped when false), so
  every existing corpus and expected file parses and re-records unchanged.
- `oracle.lua`, right after setting the buffer's lines: for each fold `{a, b}`, `:{a+1},{b+1}fold`
  (manual folds start closed). It leaves 'wrap' as nvim's default (on); for a `soft_wrap` case it
  asserts the window is 80 columns wide.
- `check`: before `set_window_rows`, `vim.model_mut().fold_rows(a..=b)` for each fold, and for
  `soft_wrap` `set_soft_wrap(true)`, `set_wrap_width(80.)`, then `wrap_pending` until nothing is pending,
  so every row is wrapped as nvim's are.

## Tests (new files)

- `crates/fe-vim/tests/corpus/scroll_display.toml` and its `scroll_display.expected.json`, recorded by
  the ignored `record_corpus_with_neovim`. Every case: a 60-row buffer, `window_rows = 10`, and either
  folds or `soft_wrap`. Fold cases use folds over rows 3-12 and 20-29 (cursor never inside a fold's
  hidden rows). Wrap cases' long rows are runs of `x` with no spaces whose length is not near a multiple
  of 80 (95 and 200 chars), so fe-display's word wrap and nvim's character wrap break them alike.
- `crates/fe-vim/tests/scroll_display_oracle.rs`, shaped as `tests/scroll_oracle.rs`: checks the case
  names below exist and every case matches nvim, including the view's top.

Case names: `ctrl_d_fold`, `ctrl_d_twice_fold`, `ctrl_d_count_fold`, `ctrl_u_fold`, `ctrl_e_fold`,
`ctrl_e_count_fold`, `ctrl_y_fold`, `ctrl_f_fold`, `ctrl_b_fold`, `zt_fold`, `zz_fold`, `zb_fold`,
`visual_ctrl_d_fold`, `ctrl_d_wrap`, `ctrl_d_twice_wrap`, `ctrl_u_wrap`, `ctrl_e_wrap`, `ctrl_e_twice_wrap`,
`ctrl_y_wrap`, `zt_wrap`, `zz_wrap`, `zb_wrap`, `zb_wrap_partial` (the cursor row's rows don't fill the
view exactly).

**Recording needs nvim 0.12.4** (`NVIM` in `tests/support/mod.rs`): neon, argon and helium have it;
the Mac has 0.12.6, which the recorder refuses. Re-recording rewrites every `*.expected.json`; all but
`scroll_display.expected.json` must come out byte-identical.

## Size

About 120 non-test lines in `scroll.rs` and `headless.rs`, 30 in the harness, plus the corpus.

## Out of scope

Any change under `crates/fe-editor` or `crates/fe-gpui`: the `EditorHost` contract and the GPUI host
already carry what is needed. A top partway down a wrapped row (from the mouse wheel) counts from
`top_display_row()`; nvim never has one, so no case covers it. Rows the GPUI frame loop has not wrapped
yet count as one display row. `<C-f>`/`<C-b>` under soft wrap (no recorded cases), 'smoothscroll',
`j`/`k` over closed folds (they still move by text rows), block decorations (`block_map` screen rows).
