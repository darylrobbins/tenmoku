---
task_uid: "01m5qpj06skr91h49vt06g57x2"
task: "T239"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test flash_one_char_labels_both_directions (top level, crates/fe-vim/tests/jump_flash.rs): with jump_style flash, gs then one char shows labels on every match in the view before and after the caret, nearest first, and a label jumps (counted, as an operator target, visual extension, . and macro replay)"
    required: true
    manual: false
  - id: "C2"
    text: "flash_single_match_and_no_match: one match jumps at once, none fails without changing anything or setting the jump mark; more matches than labels page with Space/BS"
    required: true
    manual: false
  - id: "C3"
    text: "flash_leap_style_unchanged: with the default leap style the existing jump_spec and jump_oracle tests pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test vim::jump_flash_tests::overlay_after_one_char: the overlay shows after {c1}; a settings reload of vim.jump_style switches the style live"
    required: true
    manual: false
  - id: "C5"
    text: "Integration test vim_jump_style_setting (top level, crates/fe-config/tests/vim_jump_style_setting.rs): default leap, accepts flash, a bad value is a located schema error, schema golden passes"
    required: true
    manual: false
---

# fe-vim: vim.jump_style = flash makes gs one char then labels on every visible match, both directions (Mac T659)

## Problem

The user filed this on 2026-10-13 ("a future one letter flash jump") and re-listed it on 2026-10-24 in the vim quality-of-life track. `gs`/`gS` (T366 to T368, T580) are leap-style: two chars, then a label. flash.nvim users type one char and every visible match gets a label at once.

## Current state

`JumpPrefix::then` (`crates/fe-vim/src/jump.rs:160`) waits for `{c1}{c2}`; the labelled session (autojump, `label::label`, paging, `jump_overlay`, macros recording the resolved jump) is in the same module and `jump/label.rs`; `set_jump_on_s` (:676) is the `s`/`S` opt-in (`vim.jump_on_s`, `crates/fe-config/src/settings.rs:240`, applied by `set_vim_jump_on_s`, `crates/fe-gpui/src/vim.rs:488`). The overlay is painted by the editor (T368).

## Change (the open questions answered; the lead may override any of them)

- Setting `vim.jump_style` = `"leap"` (default, today) or `"flash"`. In flash style `gs` (and `s` with `jump_on_s`) wait for ONE char, then show labels at once on every match of it in the view, both directions, the nearest first; one match jumps at once (no autojump to the nearest otherwise, flash's behaviour: you always see labels); a label jumps, as in the leap session (counted motion semantics, operator targets, visual extension, `.`, macros recording the resolved jump). `gS` in flash style is the same as `gs` except backward matches only (flash's `S` is treesitter; fastedit keeps the leap direction pair).
- Reuse the session, `label::label`, paging and `jump_overlay` unchanged; `JumpPrefix` gains the one-char form; `<CR>` as `{c1}` repeats the last jump. Label alphabet and paging beyond the alphabet are the leap ones.
- `f`/`t`/`F`/`T` flash labels (flash's char mode) are a later card.

## Tests

New `crates/fe-vim/tests/jump_flash.rs` through HeadlessVim: one char shows labels for matches before and after the caret, a label jumps, `d` + trigger + char + label, visual extension, count, `.`, macro replay, paging past the alphabet, no match fails, leap style unchanged (existing jump_spec tests pass). New `crates/fe-gpui/src/vim/jump_flash_tests.rs` (declared in vim.rs): overlay after one char, setting reload switches style live. New `crates/fe-config/tests/vim_jump_style_setting.rs`.

## Implementation notes

- The motion: `JumpChars` (`crates/fe-vim/src/args.rs`) is now `{ first, second: Option<char> }`, serialized as a one- or two-char string, so `{"jump": {"chars": "a", "forward": false}}` is the flash motion. Matching one char (`label.rs` `on_row`) takes each grapheme `{c1}` would as leap's first char, and a typed space an empty row's start too. The pre-existing unit test asserting `"a"` fails to parse was updated (`src/tests/jump.rs`).
- `vim.jump` gains `both` (flash `gs` without a count). Its targets come from `label::around`: nearest first by rows apart, then bytes apart from the caret's column, after the caret first on a tie. Each session target carries its own direction, so a label commits `{count}gs{c1}` after the caret or `{count}gS{c1}` before it; `.`, registers, marks and a recording see that counted motion. `label::flash` labels: one target jumps at once (after an operator too), more get `FULL` labels with leap's paging, never an autojump.
- The style is read when `{c1}` comes (`jump::key`), since the `g` feature that starts `gs` has no state; `JumpPrefix.one` then lets `then` break on one char for the counted path. `set_jump_style` while labels wait leaves them as they started.
- Known limit: a recording made in one style replays in the style current at replay (`2gSa` in leap waits for a second char).
- Carried finding T253 F2 (U+F8FF provenance marker): fixed in `crates/fe-gpui/src/vim.rs`. A replay's marker is read in `replay_key` before the `c` maps, and while keys are held for a longer map the replay loop resolves them as a timeout does first (`marker_after_held`), so no map's left side takes it. A map's right-side U+F8FF is text: the marker is a marker only when the next tag isn't an expansion (`VimState::remap_expansion_next`, a small addition in `state.rs` and `remap.rs`). A line recorded whole writes a typed U+F8FF after a marker (`text_keys`). Tests are in `crates/fe-gpui/src/vim/remap_line_tests.rs`.
- Scope beyond the card: `crates/fe-vim/src/args.rs`, `macros.rs`, `remap.rs`, `src/tests/jump.rs`, `crates/fe-config/src/lib.rs` (re-export), `crates/fe-gpui/src/vim/remap_line_tests.rs`.
