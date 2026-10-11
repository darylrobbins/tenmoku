---
task_uid: "01m4ewvhgy14acnn34rbspdbhz"
task: "T228"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::folds::z_keys_drive_the_model: on HeadlessVim over an indented buffer (HeadlessHost's indented-block candidates), zc on a row inside a block folds it (EditorModel's display has a fold starting at the header), zo opens it, za toggles it twice, zM folds every candidate, zR unfolds all, zv opens the fold hiding the cursor, 2zo opens two levels, zC and zO act on every level at the row; none of them changes the text or leaves an undo step; vim.fold is registered with context editor \u0026\u0026 vim.mode == normal and mutates_buffer false"
    required: true
    manual: false
  - id: "C2"
    text: "Every earlier corpus and unit test passes with its files unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/folds.rs's module docs describe the fold divergence from nvim; src/lib.rs, src/args.rs, src/reducer.rs, src/commands.rs and src/tests/mod.rs don't change"
    required: true
    manual: false
---

# fe-vim: za zc zo zA zC zO zR zM zv on the editor's folds, through EditorModel's fold API

# fe-vim: za zc zo zA zC zO zR zM zv on the editor's folds

Milestone M7 "Vim", second batch (resized), card 09c: the folds third of the old card 9. Read 09b
(T227) for context; this card no longer waits for it.
Design: `docs/decisions/vim-engine.md` §9 and §10.

**Clean room** as every M7 card: `:help fold-commands` for what each key does; never source, hjkl,
Zed or Helix.

## What exists

- Folds: `EditorModel::fold_rows`, `unfold_rows`, `fold`, `unfold`, `fold_all`, `unfold_all`
  (fe-editor), fold state in fe-display's `DisplayMap`, and candidates from
  `EditorHost::fold_range_at` / `fold_ranges` (syntax in the app, indented blocks in
  `HeadlessHost`). There is no toggle command (the gutter's toggle is GPUI-only,
  `EditorView::toggle_fold_at`).
- fastedit's folds start at the header row (the row that opens the block); Vim's
  `foldmethod=indent` folds start on the first indented row. So folds are **not** checked against
  nvim; unit tests check them against fastedit's own fold model.

## Where it registers (after the feature-module refactor this task depends on)

Everything lives in `src/folds.rs`, the stub the refactor declared and wired in (`pub mod folds`,
`folds::FEATURE` in `feature::FEATURES`):

- `FEATURE.z` takes `a c o A C O R M v` after `z` (giving `folds::FEATURE` a `z` hook is what makes
  the reducer open its `z` prefix); every other key after `z` returns `None`, since T229's scroll
  module takes `z`, `t`, `b`, `<CR>`, `.` and `-` there. The count is the hook's `Pending::count()`.
- `FEATURE.register` registers `vim.fold`; its args type is `pub` in `fe_vim::folds`.
- `src/folds.rs`'s module docs describe the keys and the fold divergence from nvim (below).

`src/lib.rs`, `args.rs`, `reducer.rs`, `commands.rs` and `src/tests/mod.rs` don't change.

## Change

`za zc zo zA zC zO zR zM zv` commit `vim.fold {"action"}` (context `editor && vim.mode == normal`,
`mutates_buffer` false) and run through `EditorModel`'s fold API with `EditorHost::fold_range_at` /
`fold_ranges`: `zo` opens the fold at the cursor's row, `zc` closes the innermost candidate
containing the row (the fold or the block around it), `za` toggles, `zO`/`zC`/`zA` act on every
level at the row, `zR` opens all, `zM` closes every candidate, `zv` opens folds hiding the cursor. A
count on `zo`/`zc` acts that many levels. They don't edit text and leave no undo step.

## Tests (new files)

`src/tests/folds.rs` (the refactor's stub, already declared in `src/tests/mod.rs`).

## Size

About 150 non-test lines.

## Out of scope

`zf`/`zd`/`zE` (manual folds), `zj`/`zk`, `[z`/`]z`, fold options, the GPUI check (11). Any change
outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **Folds stay fastedit's** (syntax folds starting at the header row), and `z` keys drive them; they
  aren't checked against nvim because nvim's fold model differs. The alternative, emulating
  `foldmethod=indent`, would give two fold models in one editor.
