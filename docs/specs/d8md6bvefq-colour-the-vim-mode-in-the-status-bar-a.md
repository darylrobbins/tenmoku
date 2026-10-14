---
task_uid: "01m4shkj01n9vv6xd8md6bvefq"
task: "T600"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test vim_mode_colors: fastedit-dark, -light and -ember each set vim_mode_normal, _insert, _visual, _replace, _command and _foreground in their [editor] table, all opaque, the five mode colours distinct, and vim_mode_foreground has WCAG contrast \u003e= 4.5:1 on each mode colour"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim_mode_status_tests: with vim on, the vim_mode item's fill is (vim_mode_normal, vim_mode_foreground) in normal mode, after d (operator pending) and while a jump is pending; vim_mode_insert after i; vim_mode_visual after v, V and ctrl-v; vim_mode_replace after R; labels unchanged; no vim_mode item with vim off"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test vim_line mode_segment_tests: with :, / and ? open, the vim-line-mode segment draws left of the prefix in the editor theme's vim_mode_command and vim_mode_foreground, and typing, enter and escape behave as before"
    required: true
    manual: false
  - id: "C4"
    text: "status_bar_vim_tests, status_bar_visual_tests, editor_palette, jump_label_contrast and change_colors pass; cargo test -p fe-config -p fe-gpui -p fe-workbench passes; clippy -D warnings clean; no screenshot golden and no crates/fe-keymap file changes"
    required: true
    manual: false
---

# Colour the vim mode in the status bar: a filled segment per mode, and a COMMAND segment on vim's command line, from new editor theme colours

## Problem

The user (2026-10-13): "Colored vim modes. … Let's make it a fancier status bar than just pure text."

With vim on, the mode shows as plain text on the left of the status bar (`NORMAL`, `INSERT`, `REPLACE`, `VISUAL`,
`VISUAL LINE`, `VISUAL BLOCK`, `JUMP ab`): `crates/fe-gpui/src/pane_item.rs`, `status_items` (`status::VIM_MODE`,
`vim_mode_label`, `jump_label`). While vim's command line is open the line replaces the bar
(`crates/fe-workbench/src/vim_line.rs`, `render_line`: prefix `:` `/` `?` then the input), so no mode shows at all.
The framework card this depends on lets an item carry a `StatusFill { background, foreground }`, drawn as a solid
lualine/airline-style segment.

Mode colours are fastedit's, not kiln's: they belong with the editor palette, a theme's `[editor]` table
(`crates/fe-config/src/theme.rs` `EditorColors`, the bundled `crates/fe-config/themes/fastedit-*.toml`, carried to
the view as `fe_gpui::EditorTheme` in `crates/fe-gpui/src/lib.rs` and `crates/fe-gpui/src/theme.rs`), as the jump
label and git change colours are.

## Change

- **Palette slots.** Add to `EditorColors` (doc comment each), `EditorTheme` and its `set_colors` mapping:
  `vim_mode_normal`, `vim_mode_insert`, `vim_mode_visual` (all three visual modes), `vim_mode_replace`,
  `vim_mode_command` (the command line), and `vim_mode_foreground` (the label on every mode's segment). A theme
  that leaves them out gets its appearance's bundled values, as for any `[editor]` key.
- **Values**, after a `# The vim mode's segment in the status bar.` comment in each theme file:

  | slot | fastedit-dark | fastedit-light | fastedit-ember |
  |---|---|---|---|
  | vim_mode_normal | `#61afef` | `#2f6fc7` | `#7fa6b8` |
  | vim_mode_insert | `#98c379` | `#2e7d32` | `#a3b56f` |
  | vim_mode_visual | `#c678dd` | `#9a2fa0` | `#c49bd1` |
  | vim_mode_replace | `#e06c75` | `#c42b3a` | `#e0705f` |
  | vim_mode_command | `#e5c07b` | `#8a5800` | `#e0a84e` |
  | vim_mode_foreground | `#16181d` | `#ffffff` | `#1c1814` |

  (Contrast of foreground on each: dark 5.6–10.3, light 5.0–6.4, ember 5.6–8.3.)
- **The mode item** (`pane_item.rs`): the `VIM_MODE` item gets `.fill(StatusFill { background, foreground })`
  from the editor's `EditorTheme`: `normal` and `operator_pending` (still labelled `NORMAL`, as
  `status_bar_vim_tests::shows_the_vim_mode` requires) and a pending jump (`JUMP …`) use `vim_mode_normal`;
  `insert` uses `vim_mode_insert`; `visual`, `visual_line`, `visual_block` use `vim_mode_visual`; `replace` uses
  `vim_mode_replace`. Labels don't change. With vim off there is still no mode item.
- **The command line** (`vim_line.rs`, `render_line`): a `COMMAND` segment first, at the bar's left edge, filled
  `vim_mode_command` / `vim_mode_foreground` from the open line's editor's theme
  (`EditorView::theme`), styled as kiln-ui draws a filled status entry, with debug selector `vim-line-mode`; then
  the prefix and input as today, with the same 8 px gap before the prefix. It shows for `:`, `/` and `?` alike.
  Keys and focus don't change.

Nothing here takes or changes a key. The workbench screenshot scene runs with vim off, so its golden doesn't
change.

## Tests

- New `crates/fe-config/tests/vim_mode_colors.rs` (contrast helpers as in `tests/jump_label_contrast.rs`).
- New `crates/fe-gpui/src/vim_mode_status_tests.rs`, declared `#[cfg(test)] #[path]` from `pane_item.rs`.
- New `crates/fe-workbench/src/vim_line/mode_segment_tests.rs`, declared from `vim_line.rs` beside
  `history_tests`.
- `crates/fe-config/tests/editor_palette.rs` may need the new keys where it lists `[editor]` keys.

## Implementation notes

- `crates/fe-config/tests/indent_guide_colors.rs` (outside the scope list) also pins `EditorColors::KEYS.len()`;
  it moves from 21 to 27 with the six new keys, as in `editor_palette.rs`.
- The command line's prefix is wrapped in a `vim-line-prefix` debug selector so the test can measure the 8 px gap.
  The segment's drawn colours are recorded under `cfg(test)` (`StatusBar::line_mode_colors`), as the bar records its
  drawn items.
