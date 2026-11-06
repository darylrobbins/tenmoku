---
task_uid: "01m6rjmjsjq2fdbctb0jfw1vz5"
task: "T1394"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "items_come_before_groups and keymap_chords_order_items_before_groups (crates/fe-vim/tests/which_key_order.rs) pass"
    required: true
    manual: false
  - id: "C2"
    text: "vim::which_key_layout_tests::glyphs_for_special_keys passes: \u003cSpace\u003e shows as ␣, \u003cTab\u003e ⇥, \u003cBar\u003e |, others as in the spec, entry keys unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "vim::which_key_layout_tests::arrows_line_up passes: every arrow has one x"
    required: true
    manual: false
  - id: "C4"
    text: "vim::which_key_layout_tests::a_group_title_is_the_group_colour_and_items_are_not passes"
    required: true
    manual: false
  - id: "C5"
    text: "cargo run -p fe-gpui --example screenshots passes on macOS with the new which_key_popup scene and committed golden"
    required: true
    manual: false
  - id: "C6"
    text: "the existing vim::which_key_tests and vim::which_key_chord_tests pass, updated only where they assert order or \u003cSpace\u003e/\u003cBar\u003e text"
    required: true
    manual: false
---

# Align the which-key popup's keys in a fixed column, show space, tab and bar as glyphs, list items before groups, and add an esc-close footer

## Problem

The user (2026-11-06) compared our which-key popup with LazyVim's: "All the keys are evenly spaced so the arrows
all line up ... different colors for sections vs. items ... a space icon and use | instead of Bar spelled out."

Ours (`popup_layer`, `crates/fe-gpui/src/vim/which_key.rs:560`) draws `key → title` as a flex-wrap of 30-char
cells. Each key is the raw vim notation (`<Space>`, `<Bar>`), so the arrow column starts wherever the key ends.
A group's title is coloured `theme.information` but an item's key and title look alike, and groups sort in with
items by key (`crates/fe-vim/src/which_key.rs` sort at ~line 112, `chord_which_key` sort in the same gpui file).
There is no hint row for how to leave the popup.

## Change

Pure presentation of the existing list; no new data.

1. **Key column.** Every cell's key sits in a fixed-width box (the widest key's glyph width in the shown list, in
   `ch` units, min 1) so every arrow starts at one x. Right-align nothing; left-align the key text.
2. **Key glyphs.** A new pure fn `key_glyph(&str) -> String` shows vim notation as which-key.nvim does:
   `<Space>` `␣`, `<Tab>` `⇥`, `<CR>` `↵`, `<Esc>` `⎋`, `<BS>` `⌫`, `<Bar>` `|`, `<lt>` `<`, `<Del>` `⌦`,
   `<Up>/<Down>/<Left>/<Right>` `↑ ↓ ← →`, `<C-x>`, `<M-x>` and `<D-x>` as written.
   Anything else unchanged. Display only: `WhichKeyEntry::key`, the prefix string and what the keymap matches are
   untouched. The prefix header also uses the glyphs (`<Space>` header shows `␣`).
3. **Colours.** Key in `theme.warning` (as today), arrow `gutter_foreground`, an item's title in `theme.foreground`,
   a group's title in `theme.information` for the whole title (today only the count is). No new colour tokens
   (T1273's screenshot run fails new literals); use existing `EditorTheme` fields only.
4. **Order.** Items (non-groups) first, then groups, each sorted as now (key, case aside, lowercase first).
   Apply in both producers: `VimState::which_key_for` (fe-vim) and `chord_which_key` (fe-gpui). Cells flow in the
   same wrapped columns, so the list reads items then groups.
5. **Footer hint.** A last row in `theme.gutter_foreground`, `esc close`, outside the scrolling entries region so it
   stays visible. LazyVim also shows `⌫ back`; Backspace does not step back here, so it is left out (no Backspace
   handling in this card).

## Scope

- `crates/fe-gpui/src/vim/which_key.rs` (popup_layer, chord sort; move `key_glyph` and layout helpers into the new
  file below when they exceed ~40 lines)
- `crates/fe-gpui/src/vim/which_key_layout.rs` (new: `key_glyph`, column width)
- `crates/fe-gpui/src/vim/which_key_layout_tests.rs` (new; declared next to `which_key_tests` in `vim.rs`: a
  one-line `#[cfg(test)] mod` edit, allowed)
- `crates/fe-gpui/src/vim.rs` (only that mod line)
- `crates/fe-vim/src/which_key.rs` (sort order)
- `crates/fe-vim/tests/which_key_order.rs` (new)
- `crates/fe-gpui/examples/screenshots/scenes.rs`, `crates/fe-gpui/examples/screenshots/golden/which_key_popup.png`
- `docs/design-language.md` (one line on the popup, if it lists popups)

Existing `which_key_tests` / `which_key_chord_tests` that assert entry order or text are updated in place.

## Tests

- `which_key_order::items_come_before_groups` (new file): `<leader>` with maps `ff`, `fr`, `bd`, `bb` and `,`: the list is `,`, then the groups `b`, `f`.
- `which_key_order::keymap_chords_order_items_before_groups`: the `g` chord merges keymap and vim keys; groups last.
- `vim::which_key_layout_tests::glyphs_for_special_keys`: the table above, plus `<C-a>` and `a` unchanged.
- `vim::which_key_layout_tests::arrows_line_up`: with keys `a`, `<Space>`, `<Bar>`, `<C-a>` shown, every
  `which-key-entry-*` arrow has the same x (use the existing `debug_selector` pattern, add `which-key-arrow-{key}`).
- `vim::which_key_layout_tests::a_group_title_is_the_group_colour_and_items_are_not`.
- Screenshot scene `which_key_popup` in `crates/fe-gpui/examples/screenshots` (leader popup with a mix of items,
  groups, `<Space>` and `<Bar>` entries, vim on): extend `Scene` with a setup closure if it has none; golden
  committed; `cargo run -p fe-gpui --example screenshots` passes (macOS compares).

## Implementation notes

- **One shared order.** `WhichKey::sort_entries` (fe-vim) is the one sort both producers call:
  `VimState::which_key_for` and fe-gpui's `chord_which_key`. A group is `WhichKeyEntry::group` (more
  keys go on past it), so a map that is also a prefix (`<leader>t` with `<leader>tt`) lists, and is
  coloured, with the groups. `keymap_chords_order_items_before_groups` builds the `g` chord's merged
  list as `chord_which_key` does (fe-vim can't reach the keymap) and checks `sort_entries` on it.
- **Scope expansion: `crates/fe-vim/tests/which_key.rs`.** Its `which_key_lists_remap_continuations`
  asserted the old interleaved order (`["a", "f", "t", "w"]`) three times; the new order makes it
  `["a", "w", "f", "t"]`. Only those three lines changed.
- **`vim.rs`** gains `mod which_key_layout;` beside the test module line: the new non-test file
  needs a declaration, and `vim.rs` is where `which_key` is declared.
- **Key column width.** `ch` is the font's `ch_advance`; the column is the widest glyph key,
  measured with `TextSystem::layout_width` (fallback fonts included), in whole `ch`. Shaped text
  can come out a hair wider than measured, so the key box is `whitespace_nowrap`.
- **Screenshot hook.** An off-screen window is never the active window, and fe-gpui resolves keys
  held for a map at once outside the active window (`wait_for_remap`), so typing `<Space>` in the
  scene can't leave the leader popup up. `EditorView::show_which_key_for(keys)` (`#[doc(hidden)]`,
  in `which_key.rs`) feeds the keys to vim's state and shows the popup at once; the scene's
  `setup` uses it, and `reset` turns vim and the maps off for the scenes after it.
- Colours are tested through the pure `entry_colors` the popup paints with: GPUI's debug bounds
  carry no colours.
