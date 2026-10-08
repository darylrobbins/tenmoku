---
task_uid: "01m4bq05t9rtgyzcr0c0xp6w8x"
task: "T174"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-vim test tests::visual_block::block_on_the_model: \u003cC-v\u003e and \u003cC-q\u003e commit vim.visual {\"kind\":\"block\"} and vim.mode is visual_block with the flag vim.visual; on \"abcd\\nab\\n\\tx\\nabcd\" from (0,1), \u003cC-v\u003e3jl gives one selection per row covering display columns 1 to 2 (none on a row too short, and the tab row's selection is what nvim's d removes there, per the corpus); after $ every row's selection reaches its end until h or l; o and O move the cursor to the opposite and the same-row corner"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::visual_block::operators_registers_and_put: a block d is one transaction, writes \" as a blockwise register of width 2 and one u restores the text; yanking the block and pasting with p on a shorter last row pads with spaces and adds rows past the buffer's end as nvim does; I then XY\u003cEsc\u003e inserts XY on every row of the block in one transaction and . repeats it; $A then ;\u003cEsc\u003e appends ; to every row; in charwise visual mode p replaces the selection and leaves the replaced text in \" while P leaves \" unchanged; \"+y of a block writes its rows joined by \\n to the host clipboard"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test visual_block_matches_neovim (tests/visual_block_oracle.rs): tests/corpus/visual_block.toml has at least 35 cases covering \u003cC-v\u003e with j k l h w $, o and O, each of d x y c s \u003e \u003c ~ u U r J D X Y C S R over a block, I and A with short and empty rows, $A, a block over tabs and over CJK wide characters, a blockwise yank pasted with p and P onto short rows and past the last row, . after I and after d, and p and P in charwise, linewise and blockwise visual mode with charwise, linewise and blockwise registers; each matches its nvim 0.12.4 expectation in buffer, cursor, mode, registers (with the blockwise width), the visual anchor and the \u003c and \u003e marks"
    required: true
    manual: false
  - id: "C4"
    text: "The recorder records a blockwise register's width as an optional field; every earlier tests/corpus/*.expected.json is byte-identical and visual_block.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C5"
    text: "Every earlier corpus test and fe-vim unit test passes with its files unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs describe block mode Exception: in crates/fe-vim/src/tests/visual.rs, \"p\" is removed from the list of keys charwise visual mode forwards (the run_visual(\"p\") == Step::Forward assertion, about line 152), because C2/C3 now make p and P in visual mode commit vim.visual_put; no other line of that file changes."
    required: true
    manual: false
---

# fe-vim: blockwise visual mode (ctrl-v), block insert and append, blockwise registers, and p and P in every visual mode

# fe-vim: blockwise visual mode (ctrl-v), block insert and append, blockwise registers, and p and P in every visual mode

Milestone M7 "Vim", second batch, card 2 of 11. Read card 1's spec (charwise and linewise visual
mode) first; this card adds the third kind on the same model. Design: `docs/decisions/vim-engine.md`
§9 item 2 (`visual_block`), item 7 ("`visual_block` is a display-column rectangle, rendered as a
`SelectionSet` (with `$` extending each row)"), item 9 (display columns, never char indices).

**Clean room** as every M7 card: behaviour from `nvim --clean` (0.12.4) and `:help blockwise-operators`,
`:help v_b_I`, `:help v_b_A`, `:help v_b_c`, `:help v_p`, `:help blockwise-register`; never from
source, hjkl, Zed or Helix.

Depends on T171 (review follow-ups of T156), which holds `src/insert.rs`.

## Change

### Block mode (`src/visual.rs`, `src/state.rs`, `src/reducer.rs`)

- `VisualKind::Block`; `vim.mode` is `visual_block`. `<C-v>` commits `vim.visual {"kind":"block"}`
  with card 1's enter / switch / leave rules; `<C-q>` is the same key, as in nvim.
- The block is the rectangle of display columns between the anchor and the cursor, over the rows
  between them. On the model it is one selection per row, covering the graphemes that overlap the
  rectangle (a row shorter than the left edge gets no selection; a wide char or tab half inside the
  block counts as nvim's operators count it, whichever the corpus shows). After `$`, every row's
  selection runs to its end, until a horizontal motion.
- `O` (in block mode) moves the cursor to the other corner on the same row; `o` to the opposite
  corner.

### Block operators (`src/operator.rs`, `src/edit.rs`, `src/insert.rs`)

- `d x y c s > < ~ u U r{char} J` over the block, with nvim's block rules: `d` and `y` write a
  **blockwise** register; `>`/`<` shift from the block's left column; `c` and `s` delete the block
  and type the inserted text on every row of it when the session ends; `r` replaces each grapheme in
  the block.
- `I` and `A` (`:help v_b_I`, `v_b_A`): an insert session at the block's left (or right) edge on its
  first row; on `<Esc>` the typed text (if it holds no line break) is inserted on every other row of
  the block, `A` padding short rows with spaces and `$A` appending at each row's end. One
  transaction; `.` repeats it.
- `D X Y C S R` in block mode follow `:help v_b_D`, `v_b_C` and nvim's recorded result for each
  (`D` and `C` reach each row's end; record the others and follow nvim).

### Blockwise registers (`src/register.rs`)

- `Register` gains a blockwise kind with its width (nvim's `getregtype()` `^V{width}`), alongside
  `linewise`. `p`/`P` of a blockwise register paste it as a block from the cursor's display column,
  padding short rows and adding rows past the end, as nvim does.
- `"+`/`"*` writes of a block put its rows joined by `\n` on the host clipboard; reading the
  clipboard never yields a block.

### `p` and `P` in visual mode (all three kinds)

- `p` replaces the selection with the register (count times) and puts the replaced text in the
  unnamed register (and `"-`/`"1` as a delete would); `P` replaces it and leaves the registers as
  they were (`:help v_P`). Charwise into linewise and linewise into charwise follow nvim.

### Oracle

- `oracle.lua` records a blockwise register's width (`getregtype()` `\22{width}`) as an optional
  `block_width` field of the register, and `mode_name` maps `\22` to `visual_block`. Older expected
  files stay byte-identical (no earlier case has a blockwise register).

### Corpus

`tests/corpus/visual_block.toml`, at least 35 cases: `<C-v>` with `j k l h w $`, `o` and `O`, each
operator above, `I` and `A` with short rows and empty rows, `$A`, `c` with typed text, a block over
tabs and over wide characters (CJK), a blockwise yank pasted with `p` and `P` (also past the
buffer's last row and onto short rows), `.` after `I` and after `d`, and `p`/`P` in charwise,
linewise and blockwise visual mode with charwise, linewise and blockwise registers.

## Tests (new files)

- `src/tests/visual_block.rs`, declared in
  `src/tests/mod.rs`.
- `tests/visual_block_oracle.rs` (`#[allow(dead_code)] mod support;`) with
  `visual_block_matches_neovim`.

## Size

Estimate about 900 non-test lines; the budget is 1,000. Block it with a split proposal (for
example `p`/`P` in visual mode out) rather than trimming criteria.

## Out of scope

- The GPUI wiring (card 3), `gv` across undo, `v_b_<`'s `shiftround`, `g<C-a>`, `:help v_b_I`'s
  behaviour with a line break in the typed text beyond what the corpus records (record one case and
  follow nvim).
- Any change outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **A block is one selection per row** on the model, so painting, copy (`cmd-c` joins rows with
  `\n`) and the multi-cursor code see ordinary selections.
- **Block paste from the system clipboard isn't possible**: the clipboard holds plain text.
