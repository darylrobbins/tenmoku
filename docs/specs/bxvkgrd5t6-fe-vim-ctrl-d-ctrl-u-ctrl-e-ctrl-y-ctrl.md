---
task_uid: "01m4ewvjpqcyy4cwbxvkgrd5t6"
task: "T229"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::scroll::default_viewport_methods: EditorHost has viewport() and scroll(ScrollRequest) with default bodies; HeadlessHost's viewport() is None and its scroll() does nothing; HeadlessHost and every existing fe-editor test are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::scroll::keys_move_view_and_cursor: on a 60-row buffer with HeadlessVim set_window_rows(10), \u003cC-d\u003e from row 0 moves top_row() to 5 and the cursor to row 5; 3\u003cC-d\u003e scrolls 3 and a following \u003cC-d\u003e scrolls 3 again; \u003cC-u\u003e at the top does nothing; \u003cC-e\u003e at top 0 with the cursor on row 0 moves top to 1 and the cursor to row 1; zt, zz and zb put the cursor's row at the top, centre and bottom without moving the cursor; z\u003cCR\u003e also moves to the first non-blank; in visual mode \u003cC-d\u003e moves the visual cursor; with viewport() None (a bare HeadlessHost through VimTarget) \u003cC-d\u003e acts on a view of page_rows() + 2 rows from row 0"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test scroll_matches_neovim (tests/scroll_oracle.rs): tests/corpus/scroll.toml has at least 25 cases with window_rows = 10 on a 60-row buffer covering \u003cC-d\u003e and \u003cC-u\u003e with and without counts (the count persisting), the buffer's ends, \u003cC-e\u003e and \u003cC-y\u003e pushing the cursor, \u003cC-f\u003e and \u003cC-b\u003e, zz zt zb z\u003cCR\u003e z. z-, near the top and bottom rows, and \u003cC-d\u003e in visual mode; each matches its nvim 0.12.4 expectation in buffer, cursor, mode, registers and top row; earlier expected files are byte-identical and scroll.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus and unit test passes with its files unchanged, with one exception: in src/tests/folds.rs (tests::folds::z_keys_drive_the_model) the assertion run(\"zz\") == Step::Cancel becomes run(\"zQ\") == Step::Cancel and nothing else in that file changes (T228 used zz as a z key no feature takes; zz now scrolls, and zQ is no Vim command); neither src/visual.rs nor anything under crates/fe-vim/src/visual/ changes, nor src/lib.rs, src/args.rs, src/reducer.rs, src/commands.rs or src/tests/mod.rs; fe-editor's lib.rs and src/tests/ don't change; scripts/check-no-gpui.sh prints ok: fe-vim and fe-editor; no Cargo.toml or Cargo.lock change; no file outside the scope changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim or crates/fe-editor"
    required: true
    manual: false
---

# fe-vim: ctrl-d ctrl-u ctrl-e ctrl-y ctrl-f ctrl-b and zz zt zb through a viewport seam on EditorHost, checked against nvim with a fixed window

# fe-vim: ctrl-d ctrl-u ctrl-e ctrl-y ctrl-f ctrl-b and zz zt zb through a viewport seam on EditorHost

Milestone M7 "Vim", second batch (resized), card 10a: the fe-editor and fe-vim half of the old card
10; 10b applies it in the editor view. Read T161's spec and 04b, 09c (T228, which takes the other
`z` keys; this card no longer waits for it). Design:
`docs/decisions/vim-engine.md` §10 ("Ctrl keys in normal and visual mode: vim wins. `ctrl-r`,
`ctrl-d`, `ctrl-u` … do what Vim does there"); §6 notes hjkl marks scroll commands "not
oracle-comparable" — here they are, with a fixed window height.

Assumes card 00 (the `visual.rs` split) has landed. Visual-mode scrolling moves the visual cursor
through the existing `visual::move_cursor` (a `vim.move`-shaped `MoveArgs`, `extend` set): neither
`src/visual.rs` nor anything under `src/visual/` changes.

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help scrolling`, `:help CTRL-D`,
`:help 'scroll'`, `:help z.`, `:help zt`.

## What exists (missing infrastructure)

- Scroll state is GPUI-only (`EditorView.scroll`, `autoscroll` in `crates/fe-gpui/src/view.rs`);
  `page_rows()` is the visible rows minus 2.
- `EditorHost` (fe-editor) offers only `page_rows`. Nothing GPUI-free can read or set the viewport.

## Change

### The seam (`crates/fe-editor/src/commands.rs`)

`EditorHost` gains two **default** methods, so `HeadlessHost` and every existing impl compile
unchanged:

- `fn viewport(&self) -> Option<Viewport>`: the first visible row and the number of whole visible
  rows (`None`: unknown, the default).
- `fn scroll(&mut self, request: ScrollRequest)`: `ScrollRequest::TopRow(u32)` (the default does
  nothing).

`Viewport` and `ScrollRequest` are public in `fe_editor::commands`; fe-editor's `lib.rs` and
`src/tests/mod.rs` (held by LSP cards T145 and T147) don't change: the defaults are tested from
fe-vim's `tests/scroll.rs`.

### The keys (`crates/fe-vim/src/scroll.rs`)

They live in `src/scroll.rs`, the stub the feature-module refactor (which this task depends on)
declared and wired in (`pub mod scroll`, `scroll::FEATURE` in `feature::FEATURES`):

- `FEATURE.key` takes `<C-d> <C-u> <C-e> <C-y> <C-f> <C-b>` in normal and visual mode (with a
  count, no operator);
- `FEATURE.z` takes `z`, `t`, `b`, `<CR>`, `.` and `-` after `z`, and returns `None` for every
  other key there (T228's folds module takes `a c o A C O R M v`);
- `FEATURE.register` registers `vim.scroll`; its args type is `pub` in `fe_vim::scroll`;
- the 'scroll' value a count sets is kept on `VimState` (`src/state.rs`).

`src/lib.rs`, `args.rs`, `reducer.rs`, `commands.rs` and `src/tests/mod.rs` don't change;
`src/scroll.rs`'s module docs describe the keys for hosts.

- `<C-d>`/`<C-u>` scroll by 'scroll' rows (half the window, rounded down; a count sets it for later
  uses, as nvim does) and move the cursor by as many; `<C-e>`/`<C-y>` scroll one row (count rows)
  and move the cursor only if it would leave the view; `<C-f>`/`<C-b>` scroll a page as nvim does
  (two rows of overlap); `zz`, `zt`, `zb` put the cursor's row at the centre, top or bottom without
  moving it, and `z<CR>`, `z.`, `z-` also go to the first non-blank.
- They commit `vim.scroll {"action", "count"}`. With `viewport()` `None` they act as if the view were
  `page_rows() + 2` rows from row 0.
- In visual mode they move the visual cursor.
- **`HeadlessVim`** keeps a virtual viewport (top row, rows; default 0 and 22) through a small
  wrapper host around its `HeadlessHost` at dispatch, so tests can read `HeadlessVim::top_row()`
  and set `set_window_rows(n)`. Its public API otherwise doesn't change.

### The oracle

A corpus case may set `window_rows = N` (an optional field). For those, `oracle.lua` sets the window
to N rows before the keys and records `top` (`line('w0') - 1`), and `support` sets `HeadlessVim`'s
window rows and compares `top`. Cases without it record nothing new, so earlier expected files stay
byte-identical. 'scrolloff' is nvim's default (0).

### Corpus

`tests/corpus/scroll.toml`, at least 25 cases with `window_rows = 10` on a 60-row buffer: `<C-d>`
and `<C-u>` with and without a count (and the count persisting), at the buffer's ends, `<C-e>` and
`<C-y>` pushing the cursor, `<C-f>` and `<C-b>`, `zz`, `zt`, `zb`, `z<CR>`, `z.`, `z-`, near the top
and bottom rows, and `<C-d>` in visual mode.

## Tests (new files)

- `crates/fe-vim/src/tests/scroll.rs` (the refactor's stub, already declared in
  `src/tests/mod.rs`), including the default methods' test.
- `crates/fe-vim/tests/scroll_oracle.rs` (`#[allow(dead_code)] mod support;`).

## Size

About 400 non-test lines (seam ~40, keys ~250, headless viewport and oracle ~110).

## Out of scope

The editor view (10b); `<C-o>`/`<C-i>`, `H M L`, `zh zl zs ze`, smooth scrolling, 'scrolloff' as a
setting.

## Decided (lead defaults; the user may revisit)

- **A viewport seam on `EditorHost`**, not a scroll command in fe-editor's registry: the plain editor
  gains no new commands, and vim reads the view the way it reads `page_rows` today.
