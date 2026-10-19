---
task_uid: "01m55a6x3pax0nnbq3sndmknyh"
task: "T802"
rev: 5
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test line_end_motions_match_neovim (top level, in crates/fe-vim/tests/line_end_motions_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/line_end_motions.toml (at least 24, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into line_end_motions.expected.json) matches nvim's buffer, cursor, mode, registers, visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, every case the spec's Problem lists (g_ with counts, under d and in visual mode, failing past the last row; g$ on a short row, then j; g$ and 2g$ and gm on a soft-wrapped 209-column row from col 0 and col 100; gm on a short row), and each of g_ g$ gm in charwise visual mode."
    required: true
    manual: false
  - id: "C3"
    text: "EditorHost::view_columns exists with a None default; HeadlessVim reports 0..80 and the GPUI host reports the editor element's visible columns (shown by a unit test of the GPUI host's column computation, or by code review where GPUI can't be constructed headless). The running-app check of g$ and gm is the separate human card, not this task's."
    required: true
    manual: false
---

# fe-vim: g_ goes to the row's last non-blank, and g$ and gm to the end and middle of what the view shows of the row, as nvim does

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track", naming the
motions `g_`, `g$` and `gm`. At Mac main 92c57c57 `line_motions.rs`'s `g` hook takes only `e E j k`, so
each cancels. Probed with `nvim --clean --headless` 0.12.6 (80 columns, 'wrap' on, the oracle's
settings):

- `g_` on `foo bar   ` lands on col 6 (the last non-blank); `2g_` on `foo bar  \nx  \nyy` on (1,0);
  `d2g_` there leaves `yy`; `dg_` from col 0 on `foo bar  ` leaves `  `; `d3g_` on one row fails;
  `vg_d` leaves `  \nx  \nyy`.
- `g$` on a row shorter than the window lands on its last character but doesn't aim at row ends:
  `g$j` on `abc\ndefgh` lands on (1,2) where `$j` lands on (1,4). On a 209-column row wrapped at 80
  columns, `g$` from col 0 lands on 79, from col 100 on 159, and `2g$` from col 0 on 159.
- `gm` goes half the window's width (40 columns) into the display row, or as far as the row goes: col 40
  from col 0 on the long row, col 120 from col 100, col 14 on `foo bar baz qux`.

`g$` and `gm` depend on what the view shows. fe-vim has the display rows (`model.display()`, as `gj gk`
use through `line_motions::with_view`) but no view width: `EditorHost::viewport()` holds only `top_row`
and `rows`.

## Change

- `crates/fe-editor/src/commands.rs`: a new `EditorHost` method with a default, `fn view_columns(&self)
  -> Option<std::ops::Range<u32>>`, the display columns of an unwrapped row the view shows (from its
  horizontal scroll to its right edge); `None` if unknown, which fe-vim reads as `0..80`, nvim's headless
  window. `crates/fe-vim/src/headless.rs`'s host returns `Some(0..80)`.
- `crates/fe-gpui/src/commands.rs` and `crates/fe-gpui/src/view.rs`: the GPUI host reports it as it
  reports `viewport()` (the same thread-local pattern), from the editor element's text width in the
  font's column advance and its horizontal scroll.
- `crates/fe-vim/src/args.rs`: `Motion::LastNonBlank` (`g_`, inclusive), `Motion::DisplayLineEnd`
  (`g$`, inclusive) and `Motion::DisplayMiddle` (`gm`, exclusive), with `line_motions.rs`'s `g` hook
  committing them and `line_motions::motion` moving them; `crates/fe-vim/src/motion.rs`'s arm that
  delegates to `line_motions::motion` lists them. `g_` moves `count - 1` rows down to that row's last
  non-blank (on an all-blank row, as nvim does) and fails past the last row; `g$` goes to the last
  character of the display row `count - 1` display rows down (soft wrap on), or of the row within
  `view_columns` (soft wrap off), keeping that column as the goal; `gm` goes `width / 2` columns into
  the caret's display row (soft wrap on) or from the view's left column (off), clamped to the row's last
  character. Follow nvim wherever the oracle shows a class or count rule differs from this summary.

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/line_end_motions.toml` (the
probed cases above, the long-row ones with `soft_wrap = true`; each in charwise visual mode too; a failing
key last) recorded into `line_end_motions.expected.json` by the ignored `record_corpus_with_neovim` test
in `tests/oracle.rs`, and a new `crates/fe-vim/tests/line_end_motions_oracle.rs` modelled on
`tests/line_motions_oracle.rs`. Check `g$` and `gm` once in the running app on macOS (a narrow window, a
long unwrapped row scrolled right, and soft wrap on) and say what you saw in the decision file.

motion.rs is held by T594 and args.rs/motion.rs by Mac T798 (bracket motions) and Mac T801 (`g*`), so this card depends on
them. `crates/fe-vim/src/lib.rs`'s docs are out of scope; the lead files one docs chore after these
cards land.
