---
task_uid: "01m4r60x4kjtesvz8x2eqh9m40"
task: "T31"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test element::jump_style_tests::no_backdrop_before_first_char: after g s with no char typed, layout.jump is Some, its backdrop is None and the jump paint log has no backdrop; after g s a the backdrop is the text bounds, painted in theme.background at JUMP_BACKDROP_ALPHA, which is 0.3"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test element::jump_style_tests::backdrop_keeps_text_readable: for the fastedit Dark, Light and Ember palettes, foreground under theme.background at JUMP_BACKDROP_ALPHA has a WCAG contrast of at least 4.5 against background (6.0, 4.59, 6.6)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test element::jump_style_tests::labels_use_theme_slots: with jump_label, jump_label_preview and jump_label_foreground set to three sentinel colours, after g s a every Preview label paints an opaque jump_label_preview box and a FontWeight::BOLD glyph in jump_label_foreground; after g s a b every Live label paints an opaque jump_label box and that glyph, and every Concealed label an opaque jump_label_preview box with no glyph; no label paint uses theme.cursor or theme.background"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test element::jump_style_tests::caret_above_backdrop: in vim normal mode (block caret), after g s a the jump paint log records the block caret after the backdrop; after escape no block caret is painted after a backdrop"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test element::jump_style_tests::live_labels_survive_the_cap: with more than MAX_MATCHES targets visible, a backward jump (g S) lays out every Live label of its first page, and so does a forward jump after space to its second page"
    required: true
    manual: false
  - id: "C6"
    text: "fe-gpui test element::jump_style_tests::slots_from_palette: EditorTheme::from_palette(EditorPalette::fastedit_light(), ...) has jump_label #a8005a, jump_label_preview #00727f, jump_label_foreground #ffffff"
    required: true
    manual: false
  - id: "C7"
    text: "fe-config test jump_label_contrast::labels_readable_in_bundled_themes: in fastedit-dark.toml, fastedit-light.toml and fastedit-ember.toml all three keys are opaque; jump_label_foreground on jump_label is at least 7.0 (7.44, 7.47, 8.16), on jump_label_preview at least 4.5 (10.31, 5.66, 9.93); each box is at least 3.0 against the editor background; jump_label and jump_label_preview differ in hue by at least 60 degrees"
    required: true
    manual: false
  - id: "C8"
    text: "EditorColors::KEYS has 16 entries and editor_palette.rs and ember_theme.rs pass; rg -n 'JUMP_PREVIEW_ALPHA|JUMP_CONCEALED_ALPHA' crates finds nothing; nothing under crates/fe-vim changes; cargo test -p fe-gpui -p fe-config and cargo clippy --workspace --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Jump overlay: no backdrop until the first char, a lighter one after, opaque bold labels in new jump_label theme colours, and the caret above the backdrop (Mac T482)

Filed on helium from Mac T482 (~/coordination/helium-cards/T482.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-m.md). Title, type, complexity, scope, criteria and labels as on the Mac.

# Jump overlay: no backdrop until the first char, a lighter one after, labels in their own theme colours, the caret above the backdrop

The overlay from T368 (commit 41ba5d26) is hard to use (user, 2026-10-12): "Should we really dim before
we enter the letter to jump to? Second, the position markers aren't prominent enough. They're very hard
to see." The review of T368 (T463 F2) also found the block caret painted under the backdrop.

## Problem

`crates/fe-gpui/src/element.rs` (`jump_layout` ~L1596, `paint_jump` ~L1630, consts ~L1586):

- The backdrop (`theme.background` at `JUMP_BACKDROP_ALPHA` 0.6) is laid out as soon as `gs`/`s` starts
  the jump, before any char is typed, so the text the user has to read to pick a target is dimmed. At 0.6
  body text falls to 2.8:1 (fastedit Dark) and 2.2:1 (fastedit Light).
- Labels reuse `theme.cursor` for the box and `theme.background` for the glyph, in the regular weight. A
  preview is the same box at 0.5 alpha, a concealed one at 0.3: over the dimmed text they barely show.
- In vim's normal and visual modes the block caret is painted under the text (~L855), before the
  backdrop, which then dims it.

## Change

### Theme slots (fe-config, fe-gpui)

Three new `[editor]` keys, in `EditorColors` (`crates/fe-config/src/theme.rs`, after `popover_border`,
each with a doc line) and in all three theme files:

| key | role | Dark | Light | Ember |
|---|---|---|---|---|
| `jump_label` | a live label's box | `#ff79c6` | `#a8005a` | `#ff8fa3` |
| `jump_label_preview` | a preview label's box, a concealed label's box | `#5fd7d7` | `#00727f` | `#7fd1c4` |
| `jump_label_foreground` | the glyph on either box | `#16181d` | `#ffffff` | `#1c1814` |

Files: `crates/fe-config/themes/fastedit-dark.toml`, `crates/fe-config/themes/fastedit-light.toml`,
`crates/fe-config/themes/fastedit-ember.toml` (its `ember_theme.rs` test already requires every
`EditorColors::KEYS` entry, so it needs no edit). Pink for live, teal for preview in every theme: different
hues, neither the cursor's blue (Dark, Light) nor Ember's amber cursor. WCAG ratios, glyph on box: live
7.44 / 7.47 / 8.16, preview 10.31 / 5.66 / 9.93 (Dark / Light / Ember); each box against its theme's
background is at least 5.6:1.

`EditorTheme` (`crates/fe-gpui/src/lib.rs`) gains `jump_label`, `jump_label_preview` and
`jump_label_foreground`, filled in `EditorTheme::colored` (`crates/fe-gpui/src/theme.rs`) like the other
13. `crates/fe-config/tests/editor_palette.rs`'s key count goes from 13 to 16.

### Painting (`crates/fe-gpui/src/element.rs`)

- **No backdrop before the first char.** `JumpLayout::backdrop` becomes `Option<Rect>`: `None` while
  `jump_query()` is `Some("")`, the text area once a char is typed (preview and live labels alike).
- **Lighter backdrop.** `JUMP_BACKDROP_ALPHA = 0.3`, still a named const, with a doc line saying why: the
  largest tenth that keeps body text at WCAG AA (4.5:1) in every bundled theme (Dark 6.0, Light 4.59,
  Ember 6.6; 0.4 drops Light to 3.5), so the text stays readable while the labels stand out.
- **Labels.** Fully opaque boxes, glyph in `jump_label_foreground` with `FontWeight::BOLD`:
  - `Live`: `jump_label` box, glyph.
  - `Preview`: `jump_label_preview` box, glyph. Told apart from live by its colour, not by alpha.
  - `Concealed`: `jump_label_preview` box, no glyph, opaque.
  - `JUMP_PREVIEW_ALPHA` and `JUMP_CONCEALED_ALPHA` go. No label paint reads `theme.cursor` or
    `theme.background`.
- **Caret above the backdrop.** With a backdrop painted, block carets are painted after the backdrop and
  labels (at `BLOCK_ALPHA`, as now) instead of before the text. With no backdrop (no jump, or no char
  typed yet) the paint order is unchanged.
- **Live labels survive the cap** (T463 F1). `jump_layout` takes at most `MAX_MATCHES` labels in text
  order, so a backward jump over more than 1,000 visible targets keeps only concealed boxes. Fill the cap
  with `Live` and `Preview` labels first, then `Concealed`, then sort by offset.
- **Paint log for tests.** A `#[cfg(test)]` thread-local in `element.rs`, like `CANDIDATES`, records each
  jump paint in order: the backdrop (bounds, colour), each label (bounds, fill, glyph colour and font
  weight, or no glyph), and each block caret painted after the backdrop. `crates/fe-gpui/src/view.rs`
  doesn't change.

Nothing under `crates/fe-vim` changes.

## Tests

- New file `crates/fe-gpui/src/element/jump_style_tests.rs`, declared from `element.rs` next to
  `mod jump_tests;`. It uses the 40-row editor of `jump_tests.rs`, with its own helper.
- New file `crates/fe-config/tests/jump_label_contrast.rs`: GPUI-free, it parses the three theme files
  through `EditorPalette::fastedit_dark()`, `fastedit_light()` and, for Ember, `ThemeFile::parse` plus
  `EditorPalette::for_theme`, as `ember_theme.rs` does.
- `crates/fe-gpui/src/element/jump_tests.rs`: `labels_at_targets` reads the backdrop as an `Option`. No
  new tests here.

## Out of scope

- Targets the jump would autojump to: they show nothing after `{c1}` today. That's the fe-vim card that
  follows this one.
- A setting for the backdrop, flash-style labels, cross-pane labels.
- The running-app check. Suggested as a human action task once this lands (below).

## Suggested follow-up (human action task, not a dependency)

In the running app with vim on, in fastedit Dark and fastedit Light: `gs` leaves the text undimmed; after
`{c1}` the text dims slightly and the teal preview labels are easy to spot; after `{c2}` the pink live
labels stand out, concealed boxes show, and the block caret is clearly visible throughout.

## Implementation notes

- fe-gpui has no `kiln-core` dependency, and its `Cargo.toml` is out of scope, so
  `backdrop_keeps_text_readable` takes Ember's background and foreground from the `[editor]` table of
  `fastedit-ember.toml` as text, over a copy of fastedit Dark's palette. `jump_label_contrast.rs` in
  fe-config parses Ember properly, through `ThemeFile::parse` and `EditorPalette::for_theme`.
- `live_labels_survive_the_cap` uses its own wider editor (60 rows of 30 `ab`s, 50 in view): the 40-row
  editor of `jump_tests.rs` has about 60 targets in view, too few to reach `MAX_MATCHES`. With the
  cap filled in text order, its backward jump lays out no live label.
- The cap sorts only when there are more labels than `MAX_MATCHES`: a stable sort putting `Concealed`
  last, a truncate, then a sort by offset.
