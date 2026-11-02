---
task_uid: "01m6c9t45q6asy0je7r7jhbw00"
task: "T267"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test subword_objects_select (crates/fe-vim/tests/subword_objects.rs) passes: diS, ciS, yiS and daS on fooBarBaz, HTTPServer, snake_case_name and 'a fooBar b', and 2iS and 2aS, take the spans the card's rules give"
    required: true
    manual: false
  - id: "C2"
    text: "Test subword_objects_in_visual passes: viS selects the subword under the caret and iS again extends the selection by the next subword, charwise; vaS likewise"
    required: true
    manual: false
  - id: "C3"
    text: "Test subword_objects_need_no_setting passes: iS and aS work with set_subword_motions off and on, and . after ciS repeats at the caret"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# fe-vim: iS and aS select the camelCase, PascalCase, snake_case or digit-run part under the caret, with counts and growing in visual mode, as nvim-various-textobjs' subword object does (Mac T1293)

## Problem

Part of the subword feature the user approved on 2026-11-01 ("include text objects if cheap"). With the subword
motions in place (depends-on card), a subword still can't be operated on as an object: `ciw` on `fooBarBaz`
takes the whole identifier.

Key choice: `iS` and `aS`, as nvim-various-textobjs names its subword object. They are free: neither vim, nvim
nor any fe-vim feature takes `S` after `i`/`a` (`crates/fe-vim/src/feature/objects.rs` takes `e i I`,
`argument.rs` `a`, `definition.rs` `f c`). They're always on, needing no setting, unlike `w b e ge`.

## Change

- New `TextObject::InnerSubword` and `ASubword` (`crates/fe-vim/src/args.rs`, serde `inner_subword`,
  `a_subword`), mapped from `S` after `i`/`a` in the reducer's object table (`crates/fe-vim/src/reducer.rs`,
  beside `'w' => (InnerWord, AWord)`), selected in `crates/fe-vim/src/text_object.rs` by the subword boundaries
  of `crates/fe-vim/src/feature/subword.rs`, passing the same granularity to `words` that the motions card
  introduced.
- `iS`: the subword under the caret; on a `_` run inside a keyword run, that run; on blanks, the blanks (as `iw`).
  `aS`: the subword with the `_` run after it, or if none, the one before it; a subword ending its keyword run
  takes the blanks after it (or before, if none after), as `aw` does. A count takes that many subwords
  (`_` runs and blanks counting as `iw` counts them for `iS`).
- Visual mode: an arm in `crates/fe-vim/src/visual/object.rs` (`select_object`), so `viS` selects the subword
  and typing `iS` again extends the selection by the next subword, as `iw` extends by words, charwise.
- `.` after `ciS`/`daS` repeats at the caret.

Tests: new `crates/fe-vim/tests/subword_objects.rs`. No nvim oracle (nvim has no subword object).
