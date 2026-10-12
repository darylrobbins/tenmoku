---
task_uid: "01m4r8jvh52db5rfc111mnfsmx"
task: "T34"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "EditorColors::KEYS has three more entries than at the task base (19 after T482's 16): change_added, change_modified, change_deleted; fe-config tests editor_palette and ember_theme pass, with editor_palette.rs changing only its key count"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test change_colors: in fastedit-dark.toml, fastedit-light.toml and fastedit-ember.toml, change_modified equals that file's information, change_deleted its error, and change_added is its information with hue 130 degrees and the same saturation and lightness, within 1/255 per RGB channel"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test change_colors: EditorPalette::for_theme on a dark kiln theme whose [editor] sets change_added = \"#123456\" gives that colour with no warnings and the bundled change_modified and change_deleted; a theme without the keys gets all three from the bundled palette"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui decorations_tests::tints_resolve_through_the_theme: for an EditorTheme from from_palette with three sentinel change colours, tint_color maps Added to change_added, Modified to change_modified and Removed to change_deleted, while Error stays error and Information stays information; row_backgrounds_and_bars_cover_every_display_row expects theme.change_modified for the bar"
    required: true
    manual: false
  - id: "C5"
    text: "git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md; crates/fastedit/src/theme.rs is unchanged"
    required: true
    manual: false
---

# Add change_added, change_modified and change_deleted editor theme colours for the git change bar, set from the active kiln theme and defaulting to today's colours (Mac T485)

Filed on helium from Mac T485 (~/coordination/helium-cards/T485.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-p.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note (Mac note p): criteria saying nothing outside the scope changes exempt this lane's own docs/specs file. Title the host commit "... (Mac T<n>)".

# Theme slots for the git change bar's added, modified and deleted colours

## Problem

The user, 2026-10-12, on the git change indicator: "It looks cramped. Think of how we make it look better
visually." The lane and bar shapes are the change-lane card this one depends on; this card gives the
colours theme slots.

At main b6a3275f, `tint_color` in `crates/fe-gpui/src/element/decorations.rs` borrows diagnostic colours
for git changes: `Tint::Modified` is `theme.information`, `Tint::Removed` is `theme.error`, and
`Tint::Added` is `theme.information` with its hue set to 130° (the doc comment says "until the theme has
slots for them"). A kiln theme can't colour changes apart from diagnostics: recolouring `information`
recolours modified rows too.

Kiln theme -> editor colours is already wired (T35, Mac T246, commit 801e1afb): `crates/fastedit/src/theme.rs`
calls `EditorPalette::for_theme`, which lays a theme's `[editor]` table over the bundled palette for its
appearance (`crates/fe-config/src/theme.rs`), and `EditorTheme::set_colors` / `colored` in
`crates/fe-gpui/src/theme.rs` copy each `EditorColors` key into an `EditorTheme` field. A key the theme
leaves out takes the bundled palette's value. So new keys need only `EditorColors`, the bundled theme
files, `EditorTheme` and `tint_color`; `crates/fastedit/src/theme.rs` doesn't change.

T482 (jump overlay, lent to helium) adds three `EditorColors` keys (`jump_label`, `jump_label_preview`,
`jump_label_foreground`) in the same files and takes `EditorColors::KEYS` to 16. This card goes after it.

## Change

- `crates/fe-config/src/theme.rs`: add `change_added`, `change_modified` and `change_deleted` to the
  `colors!` `EditorColors` table, after T482's keys, each with a doc comment ("The gutter bar and row
  background of rows added since the git base", and so on). `KEYS` grows to 19.
- `crates/fe-config/themes/fastedit-dark.toml`, `fastedit-light.toml`, `fastedit-ember.toml`: set the three
  keys in `[editor]` to today's colours for that file: `change_modified` = its `information`,
  `change_deleted` = its `error`, `change_added` = its `information` with the hue set to 130° (same
  saturation and lightness), as `#rrggbb`. A comment says they match the old derived colours.
- `crates/fe-gpui/src/lib.rs`: `EditorTheme` gets `change_added`, `change_modified`, `change_deleted:
  Hsla`, documented. `crates/fe-gpui/src/theme.rs` (`colored`) fills them from the palette.
- `crates/fe-gpui/src/element/decorations.rs`: `tint_color` maps `Tint::Added` to `theme.change_added`,
  `Tint::Modified` to `theme.change_modified`, `Tint::Removed` to `theme.change_deleted`. `Tint::Error`
  stays `theme.error` and `Tint::Information` stays `theme.information`. Drop the hue-130 derivation and the
  "until the theme has slots" wording.
- Tests: new `crates/fe-config/tests/change_colors.rs`; `crates/fe-config/tests/editor_palette.rs`'s key
  count; `crates/fe-gpui/src/element/decorations_tests.rs`'s `tints_resolve_through_the_theme`, and the
  expected bar colour in `row_backgrounds_and_bars_cover_every_display_row` (now `theme.change_modified`).

Put design notes in code docs. Don't append an Implementation notes section to this spec.
