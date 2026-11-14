---
task_uid: "01m7ac1va76tbgw0nybxrbxep6"
task: "T343"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-config --test color_blind pass: off_by_default, on_replaces_added_passed_insert_and_removed_failed_error_in_yohen_and_hakuji, soft_grounds_follow, a_theme_without_the_table_is_unchanged, replacements_differ_in_oklab_l_by_at_least_0_05_and_clear_4_5_on_their_surfaces"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fastedit theme passes, including a case that the setting applies without restart"
    required: true
    manual: false
  - id: "C3"
    text: "UPDATE_GOLDEN=1 regenerates the settings schema and the generated settings.ftl and settings-template.ftl; cargo nextest run -p fe-config passes with them"
    required: true
    manual: false
---

# Colour-blind mode: workbench.color_blind_mode swaps the green and red signals for blue and orange in Yohen and Hakuji (Mac T1581)

# Colour-blind mode: blue and orange for green and red

Spec: `docs/design/design-system.md#colour` ("A colour-blind mode swaps green
and red for blue and orange as its own setting, rather than weakening the
default for everyone") and Accessibility ("offered next to the theme picker").

## Change

- `workbench.color_blind_mode` (bool, default false; user setting, applied live):
  `crates/fe-config/src/settings.rs`, schema, template, generated catalogue.
  The key spelling follows `editor.color_decorators`.
- When on, a theme's `[color_blind]` table overrides the signals that carry the
  green/red meaning: added, passed, INSERT (celadon) become blue, removed,
  failed, errors (shinsha) become orange, and their soft grounds follow. Yohen
  and Hakuji each carry their table (`crates/fe-config/themes/{yohen,hakuji}.toml`);
  a theme without one is unchanged. Warnings (kiseto) and modified (ruri)
  stay, and the two replacements keep a lightness difference of at least 0.05
  in OKLab L from each other and 4.5:1 text contrast on the surfaces they sit
  on. Every glyph (+, −, ✓, ✕, ▲) is unchanged.
- The switch sits under the theme cards in the Settings page (the page lists
  schema settings by itself; its row goes in Common after the theme).
  `crates/fastedit/src/theme.rs` applies the override where the palette is
  built.
- Chosen values: the table stays an open question for the user to set (Okabe-Ito
  blue `#0072B2` and vermillion `#D55E00` are the starting points, adjusted per
  theme for the contrast rule).

## Not in this card

Other colour-vision modes, changing syntax colours, a high-contrast theme.

Platform: **Linux-OK**.

## Implementation notes

- The table is `[color_blind]`, a fourth app table (`fe_config::theme::EXTENSIONS`).
  Its `editor` table replaces `[editor]` keys; its other tables are kiln token
  groups (`[color_blind.status]`), whose derived tokens follow (`vcs.added`,
  `vcs.untracked`, `vcs.deleted`). `fe_config::theme::color_blind` lays it over a
  `ThemeData` and takes it out, so laying it twice changes nothing.
- `crates/fastedit/src/theme.rs` swaps the dark and light pair kiln sets
  (`follow_color_blind`), so the chrome's tokens and the editor's palette change
  together, and puts kiln's own pair back when the mode goes off. kiln sets its
  pair again on a reload, which is swapped again; the mode switch and the
  contrast pass work on the swapped pair. No kiln change.
- Values (the open question, for the user to retune): Okabe-Ito's hues moved in
  OKLab L. Yohen blue `#58b1f6` (lighter than ruri, 0.065 in L) and orange
  `#eb7228`; Hakuji blue `#014e7c` (darker than ruri) and orange `#903d00`
  (0.055 below kiseto in L, as the design keeps warning and error apart). Soft
  grounds and diff-word alphas use the themes' own rules. REPLACE (shinsha) turns
  orange with errors; quick-scope, `markup.inserted` and the terminal's ANSI
  colours stay (syntax and the terminal aren't in this card). Yohen's orange sits
  near the hare's-fur accent (0.02 apart in L), which comes with orange errors in
  a rust-accent theme.
- It's not project-restricted, like `workbench.contrast`.
- The Settings page row is first in `COMMON`, under the theme cards, labelled
  "Colour-blind mode".
