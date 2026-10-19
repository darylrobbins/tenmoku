---
task_uid: "01m55mj5q2k0wwnd5a3zw4edb4"
task: "T153"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test entire_objects (top level, in crates/fe-vim/tests/entire_and_indent_objects.rs) passes: dae, yie and cie with leading and trailing blank rows, an all-blank buffer, and vae and vie selecting linewise."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test indent_objects (top level, same file) passes: dii, dai, daI, 2dii, each on a blank row and at the buffer's end, vii again growing to the next level, and . repeating dii."
    required: true
    manual: false
---

# fe-vim: ie and ae (the whole buffer) and ii, ai and aI (an indent level) text objects, as textobj-entire and vim-indent-object give them (Mac T891)

Filed on argon from Mac T891 (card ~/coordination/argon-cards/zzp/T891.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55mebs5bd1psd134ndw1bkb. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T152 (Mac T890). Title the host commit "... (Mac T891)".

## Problem

At Mac main 6f032b12 fe-vim has no entire-buffer (`ie`/`ae`, textobj-entire) or indent (`ii`/`ai`/`aI`,
vim-indent-object) text objects, both emulated by VSCodeVim and IdeaVim: `die` and `dii` cancel. The user
approved them in the "Vim parity" track on 2026-10-17. The memory notes for M7's breadth cards (T415, T417)
found that a new object also needs an arm in `crates/fe-vim/src/visual/object.rs`, which otherwise grows
every non-word object charwise.

## Change

New feature module `crates/fe-vim/src/feature/objects.rs`, declared in `crates/fe-vim/src/feature.rs` and
listed in `FEATURES`, its `object` hook mapping `e` and `i`/`I` to new `TextObject` variants
(`crates/fe-vim/src/args.rs`) dispatched from `crates/fe-vim/src/text_object.rs`, and an arm at the top of
`visual/object.rs`'s `select_object` routing them to the module's own visual selection. All are linewise.

- `ae`: every row. `ie`: every row but the leading and trailing blank rows (an all-blank buffer: all rows).
- `ii`: the rows around the cursor's row whose indent is at least its (blank rows between them included,
  trailing blank rows not); on a blank row, the next non-blank row's level. `ai`: that plus the row above.
  `aI`: plus the rows above and below. A count takes that many levels out.
- In a visual mode, the object again when the selection already is that level takes the next level out.
- An operator over them, and `.`, work as over any linewise object.

Behaviour reference: textobj-entire's and vim-indent-object's documentation (no source); no nvim oracle.
Tests in a new integration file `crates/fe-vim/tests/entire_and_indent_objects.rs`, including visual mode.
