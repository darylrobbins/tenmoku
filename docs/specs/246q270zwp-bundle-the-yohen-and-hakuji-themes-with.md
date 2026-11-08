---
task_uid: "01m6vvrvwgwh12k5246q270zwp"
task: "T1512"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test every_design_role_has_the_design_value (cargo nextest run -p fe-config --test yohen_hakuji_values) passes for every row of the Signals, Syntax and Surfaces tables and the three text steps, in both themes, against docs/design/design-system.md as retuned by this card (only the values named in the spec moved, by OKLab L only)"
    required: true
    manual: false
  - id: "C2"
    text: "Tests text_steps_clear_4_5_on_every_surface, yohen_code_clears_6_on_editor_and_cursor_line and hakuji_code_clears_4_5 (cargo nextest run -p fe-config --test yohen_hakuji_contrast) pass, WCAG 2 relative luminance, over chrome, tool window, editor, cursor line, hover, selected, focused selection and overlay"
    required: true
    manual: false
  - id: "C3"
    text: "Tests closest_signal_pair_is_at_least_5_delta_e and paired_signals_differ_in_lightness (same binary) pass: Yohen's closest pair of accent, agent, celadon, shinsha, kiseto, ruri and jun is \u003e= 5.0 OKLab dE; added/removed, warning/error and accent/agent differ in OKLab L by \u003e= 0.05 in both themes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-config passes (the every-bundled-theme suites accept both, counts updated; design_role_tokens still holds every pre-existing theme unchanged), and cargo nextest run -p fe-workbench theme_picker passes"
    required: true
    manual: false
  - id: "C5"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (setup_wizard_dark.png, setup_wizard_light.png)"
    required: true
    manual: false
---

# Bundle the Yohen and Hakuji themes with the design's values, and test their contrast and role distance

# Bundle the Yohen and Hakuji themes, with their contrast and role-distance tests

Spec: `docs/design/design-system.md#colour` (Signals, Syntax, Surfaces, text
steps) and `#editor` (indent guides `#2c241e` / `#eceef1`, find-match
`#5a4314`). Uses the role tokens of the design-role tokens card.

Two new complete themes (`kiln_theme = 1`, every token table, `[editor]` and
`[syntax]`), bundled but **not yet the default**.

## Change

- `crates/fe-config/themes/yohen.toml` (dark, "Yohen") and
  `crates/fe-config/themes/hakuji.toml` (light, "Hakuji"), every value from the
  design's tables. Where the design gives no value (soft grounds other than
  celadon-soft, hover tint for agent, diff word tints, minimap, sticky scroll,
  jump labels, quick-scope), derive it from the nearest role and say so in a
  comment. Vim modes: NORMAL accent, INSERT celadon, VISUAL jun, REPLACE
  shinsha, COMMAND kiseto. Brackets by depth: function, keyword, type colours.
  Change bar: modified ruri, added celadon, deleted shinsha; agent lane oil spot.
  Diagnostics: error shinsha, warning kiseto (one warning colour, not two).
- `crates/fe-config/src/themes.rs` `BUNDLED`: both, after the Tenmoku three.
- `crates/fe-config/settings-template.toml`: the bundled-title list.
- Counted tests: `minimap_theme_keys.rs` (11 → 13), `sticky_scroll_colors.rs`
  (its `want` array).
- The setup wizard's Appearance page lists bundled themes, so its two goldens
  gain the new tiles: regenerate them on the Mac (`UPDATE_SCREENSHOTS=1 cargo
  run -p fe-workbench --example screenshots`) and look at them.

## Tests (new files)

- `crates/fe-config/tests/yohen_hakuji_values.rs`: every role of the design's
  three tables resolves to the design's hex in each theme.
- `crates/fe-config/tests/yohen_hakuji_contrast.rs`, WCAG 2 relative
  luminance:
  - ink, muted and faint text ≥ 4.5:1 on chrome, tool window, editor,
    cursor line, hover, selected, focused selection and overlay, both themes;
  - every Yohen syntax colour ≥ 6:1 on editor and on cursor line; every Hakuji
    syntax colour ≥ 4.5:1 on both;
  - the closest pair of signal roles (accent, agent, celadon, shinsha, kiseto,
    ruri, jun) in Yohen ≥ 5.0 apart in OKLab ΔE (the design says 5.4);
  - colours the design says must be told apart (added/removed,
    warning/error, accent/agent) also differ in OKLab L by ≥ 0.05.
- Both themes pass the suites that iterate every bundled theme
  (`bundled_themes.rs`, `quick_scope_colors.rs`, `jump_label_contrast.rs`,
  `bracket_pair_colors_default.rs`, `template_theme_names.rs`).

## Not in this card

Making them the default; terminal ANSI colours (next card); italic comments and
bold definitions (the theme format has no font styles: a separate capability).

Platform: **Mac-only** (the two setup wizard goldens).

**Lead ruling (rev 2): retune the values, keep the rules.** C2 and C3 are the accessibility contract; where the design's
hex values break them, the values are wrong. Change as little as possible, keeping hue and chroma (move OKLab L only):
- Yohen faint `#978777` → `#a09080`; Yohen comment `#839c86` → `#88a18b`; Hakuji comment `#6a737d` → `#69727c`.
- Hakuji's three lightness pairs (added/removed = celadon/shinsha, warning/error = kiseto/shinsha, accent/agent =
  accent/oil spot): move the L of one colour of each pair (prefer darkening, which also helps contrast on Hakuji's light
  surfaces) by the least that reaches 0.05, re-checking every C2 contrast after the change. If any pair can't reach
  0.05 without breaking C2, stop and report the numbers.
- Write each changed value into `docs/design/design-system.md` (its Signals, Syntax and Surfaces tables), with one
  line under the palette saying which values rev 2 of T1512 lifted and why. C1 then checks the doc's new values.

Scope extras: `crates/fe-config/tests/design_role_tokens.rs` (its snapshot of pre-existing themes must not fail on new
theme files: check the BEFORE themes are all present and unchanged, rather than that the set equals BEFORE) and
`crates/fe-workbench/src/theme_picker_tests.rs` (Yohen and Hakuji rows after Tenmoku Ember).

## Implementation notes

- **Hakuji's lightness pairs (review round 1, F1).** Darkening kiseto `#776a00`
  leaves the sRGB gamut at once (its blue channel is already 0), so it can't
  move in L alone. Shinsha is the lighter colour of both of its pairs, so
  darkening it alone by the least that reaches 0.05 against both,
  `#c0362c` → `#a1100e` (L −0.089, a/b drift 0.0003, rounding only), satisfies
  added/removed (0.0506) and warning/error (0.0687). Celadon and kiseto keep
  the design's `#17773f` and `#776a00`. Shinsha's lowest contrast over the
  eight surfaces rises from 4.58 to 6.72. Oil spot `#3f6587` → `#3a5f81` is
  unchanged. `retuned_values_move_in_lightness_only` reads the doc's note and
  holds every retuned value's OKLab a/b to within 0.002 of the original.
- **Jump labels (F2).** `jump_label_contrast.rs` keeps a hard-coded theme list
  and is outside this card's scope, so `yohen_hakuji_contrast.rs`
  (`jump_labels_readable`) applies the same rules to both new themes.
