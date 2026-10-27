---
task_uid: "01m5qphzwvkbnkyjqg5xs24sfd"
task: "T236"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-display test layout::scroll_off_tests::caret_keeps_n_rows_from_the_edges: with scroll_off 0, 3 and 8 a caret moved below and above the window scrolls so it sits that many rows from the edge; 3 reproduces today's frames (existing layout tests pass unchanged)"
    required: true
    manual: false
  - id: "C2"
    text: "layout::scroll_off_tests::huge_and_tiny: a value larger than half the window centres the caret and never scrolls past the last row; a window of two rows does not loop or panic"
    required: true
    manual: false
  - id: "C3"
    text: "Integration test ex_set_scrolloff_forms (top level, crates/fe-vim/tests/ex_set_scrolloff.rs): :set so=5, scrolloff=5, so? (message '  scrolloff=5'), so=-1 and so=x error, noso/invso refused, and the fe-vim scroll oracle tests (scroll_oracle, folds scrolling) pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "Integration test editor_scroll_off_setting (top level, crates/fe-config/tests/editor_scroll_off_setting.rs): default 3, range 0 to 100, schema golden passes"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test scroll_off_tests::sticky_margin_follows_the_option: element.rs's sticky-scroll clearance uses the same margin and a settings reload applies it live"
    required: true
    manual: false
---

# Add 'scrolloff': :set so=N and editor.cursor_surrounding_lines set how many rows autoscroll keeps around the caret (default stays 3) (Mac T1166)

## Problem

The user asked on 2026-10-24 for 'scrolloff' as a vim option (`:set so=8`, and a setting): the rows kept between the caret and the window's edge. Vim users set it (LazyVim sets 4, many set 8 or 999 to centre). The lead's survey says it is hard 0; T229/T230 fit fe-vim's own scroll commands to nvim's 0.

## Current state

- The editor's autoscroll keeps the caret three rows from the edges: `let margin = (line_height * 3.).min(height / 3.)` at `crates/fe-display/src/layout.rs:899`, and the same margin again in `crates/fe-gpui/src/element.rs:958` (sticky scroll's `clear_of_caret`). So the editor margin is 3 rows, not 0; 0 is what fe-vim's scroll commands (`<C-d>`, `zt`, `H`/`L` in `crates/fe-vim/src/scroll.rs`, `line_motions.rs`) are fitted to (memory fe-vim-oracle-facts, T229/T230; their oracle tests must stay unchanged).
- `:set` option list and `apply` are in `crates/fe-vim/src/ex/set.rs:41-62`; per-editor model flags like `soft_wrap` in `crates/fe-editor/src/lib.rs:578`; settings reach views in `crates/fe-gpui/src/view.rs:660-672`.

## Change

- Setting `editor.cursor_surrounding_lines` (VS Code's name; integer 0 to 100, default 3 = today's behaviour) in settings.rs, schema and template. `:set scrolloff=N` / `so=N` / `so?` per editor (value clamps to half the window's rows, so `999` centres, as nvim treats it).
- `EditorModel` keeps `scroll_off` (fe-editor lib.rs) and pushes it to `DisplayMap` (display_map.rs); layout.rs:899 and element.rs:958 use `scroll_off * line_height` (still capped at height/3 for the default only; an explicit value caps at (height - one row) / 2).
- fe-vim's scroll commands keep nvim's 0 (documented: they are keyed to the 0 oracle); the option governs caret-driven autoscroll (typing, `j`/`k` past the edge, search jumps, `n`).
- Out of scope: making `<C-d>`/`zt`/`H`/`L` honour a non-zero scrolloff (their own card, with the oracle at scrolloff=N).

## Tests

New `crates/fe-display/src/layout/scroll_off_tests.rs` (`#[path]` line in layout.rs): with 0, 3 and 8, a caret moved past the bottom/top lands that many rows from the edge, a huge value centres, a tiny window clamps. New `crates/fe-vim/tests/ex_set_scrolloff.rs` (`:set so=5`, `so?` message, `so=-1` and `so=x` errors, `no`/`inv` refused as non-bools). New `crates/fe-config/tests/editor_scroll_off_setting.rs`; fe-gpui test in `crates/fe-gpui/src/scroll_off_tests.rs` that the sticky-scroll margin follows it and a reload applies.
