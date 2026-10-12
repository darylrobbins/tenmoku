---
task_uid: "01m4p2ytcj641cn2sh2nz2e7cv"
task: "T415"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests tests::prose::*: } and { go to the count-th next or previous empty row (else the text end or start), ) and ( to the count-th next or previous sentence start (a sentence ending at . ! or ? then any ) ] or quote chars then a space, tab or row end; empty rows and the text ends are boundaries); all four are exclusive jumps that set the jump mark, compose with d c y and the visual modes, and fail with nothing changed past the text ends"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test prose_matches_neovim (tests/prose_oracle.rs): tests/corpus/prose.toml has at least 40 cases covering { } ( ) with counts and at the text ends, d} and d{ becoming linewise, c) and y(, ip ap is as with counts in d c y, vip and vap repeated, dot-repeat of dap and the jump mark after }; each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C3"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes; nothing outside src/prose.rs, src/tests/prose.rs, tests/prose_oracle.rs and tests/corpus/prose.* changes, except: src/tests/feature_stubs.rs (only removing T415's 8 prose variants, paragraph_forward … a_sentence, from the move/select loop of new_variants_change_nothing, their round_trips check kept), src/visual/object.rs (only an arm in select_object sending ip ap is as to prose::select_visual) and src/marks.rs (only a prose::lands arm in jump for { } ( )); Regatta's own Spec/Respec commits to docs/specs/sh2nz2e7cv-fe-vim-paragraph-and-sentence-motions.md are exempt (the lane must not edit that file itself)"
    required: true
    manual: false
---

# fe-vim: { } ( ) paragraph and sentence motions and ip ap is as objects, checked against nvim

# fe-vim: { } ( ) paragraph and sentence motions and ip ap is as objects

## Problem

fe-vim has no paragraph or sentence motions or objects (`reducer.rs` `motion()` and
`text_object()`; gap analysis 2026-10-11, M7 feature breadth). `{`, `}`, `dap`, `cis` are among the
most used Vim keys. The seam card this depends on declared the variants (`Motion::Paragraph*`,
`Sentence*`, `TextObject::InnerParagraph … ASentence`) and the empty `src/prose.rs` they delegate to.

## Change

Everything lives in `src/prose.rs` (the seam's stub, `prose::FEATURE` already in `FEATURES`):

- `FEATURE.key` takes `{ } ( )` in normal, visual and operator-pending mode and commits the motion
  through `Pending::motion` (count, register and operator as any motion).
- `FEATURE.object` maps `p` and `s` (after `i`/`a`) to the prose objects.
- `prose::motion`: `}`/`{` go to the `count`th next/previous paragraph boundary (an empty row, or
  the text's last/first char); `)`/`(` to the `count`th next/previous sentence start. A sentence
  ends at `.`, `!` or `?` followed by any number of `)`, `]`, `"`, `'` and then a space, tab or row
  end; paragraph boundaries are sentence boundaries too (`:help sentence`, `:help paragraph`, nvim's
  default 'cpoptions' without `J`).
- `prose::class`/`is_jump`/`writes_numbered`: exclusive, jumps, and writing register `1`, as
  `:help exclusive`, `:help jump-motions` and `:help quote_number` say; the
  exclusive-becomes-linewise rule (`:help exclusive-linewise`) applies through the existing operator
  path.
- `prose::select`: `ip ap is as` with counts, as nvim selects them, including white-space-only
  paragraphs and `ap`/`as` taking trailing (or, at the end, leading) blank rows or blanks; in visual
  mode a repeated `ip`/`ap` grows the selection as nvim's does (through the existing `vim.select`
  path).
- `src/prose.rs`'s module docs describe the keys.

**Clean room**: `nvim --clean` (0.12.4) and `:help {`, `:help (`, `:help sentence`,
`:help paragraph`, `:help ip`, `:help as`; never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/prose.toml`, at least 40 cases: `{ } ( )` with counts, at and past the text's ends,
over blank and white-space-only rows, `d}`, `d{` (linewise by the exclusive rule), `c)`, `y(`,
`ip ap is as` with counts in `d c y`, `vip`/`vap` repeated, `.` after `dap`, `''` after `}`. A key
that fails is a case's last key (nvim drops the typeahead after an error).

## Tests (new files)

- `src/tests/prose.rs` (the seam's stub, already declared).
- `tests/prose_oracle.rs` (`#[allow(dead_code)] mod support;`) with `prose_matches_neovim`.
- Record with `cargo nextest run -p fe-vim --run-ignored only -E 'test(=record_corpus_with_neovim)'`.

## Size

About 300 non-test lines.

## Out of scope

nroff macros in 'paragraphs' and 'sections' (the corpus has no row starting with `.`), `[[ ]]`
sections, the GPUI check. Any change outside `src/prose.rs` and its tests and corpus.
